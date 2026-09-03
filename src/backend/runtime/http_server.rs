use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::Router;
use hyper::body::{Body as HttpBody, Frame, Incoming, SizeHint};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Notify};
use tokio::task::JoinSet;
use tower::ServiceExt;

use super::{ConnectionAdmission, SchedulerHandle};

pub const HTTP1_MAX_READ_BUFFER_BYTES: usize = 128 * 1024;
pub const HTTP1_MAX_HEADERS: usize = 128;
pub const HTTP1_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(15);
pub const HTTP_BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
pub const HTTP_KEEP_ALIVE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
pub const HTTP_RESPONSE_WRITE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy)]
pub struct HttpIdleTimeouts {
    body: Duration,
    keep_alive: Duration,
    response_write: Duration,
}

impl HttpIdleTimeouts {
    pub const SOURCE_OWNED: Self = Self {
        body: HTTP_BODY_IDLE_TIMEOUT,
        keep_alive: HTTP_KEEP_ALIVE_IDLE_TIMEOUT,
        response_write: HTTP_RESPONSE_WRITE_IDLE_TIMEOUT,
    };

    pub const fn new(body: Duration, keep_alive: Duration, response_write: Duration) -> Self {
        Self {
            body,
            keep_alive,
            response_write,
        }
    }
}

pub async fn serve_http1<Shutdown>(
    listener: TcpListener,
    app: Router,
    scheduler: SchedulerHandle,
    shutdown: Shutdown,
    shutdown_grace: Duration,
    idle_timeouts: HttpIdleTimeouts,
) -> io::Result<()>
where
    Shutdown: Future<Output = ()>,
{
    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                scheduler.begin_quiescing().map_err(io::Error::other)?;
                break;
            }
            accepted = listener.accept() => {
                let (stream, peer_address) = accepted?;
                let admission = match scheduler.try_acquire_connection() {
                    Ok(admission) => admission,
                    Err(_) => continue,
                };
                connections.spawn(serve_connection(
                    stream,
                    peer_address,
                    app.clone(),
                    admission,
                    shutdown_receiver.clone(),
                    idle_timeouts,
                ));
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                report_connection_completion(completed);
            }
        }
    }

    drop(listener);
    let _ = shutdown_sender.send(true);
    let shutdown_result = scheduler
        .finish_shutdown(shutdown_grace)
        .await
        .map_err(io::Error::other);
    if shutdown_result.is_err() {
        connections.abort_all();
    }
    while let Some(completed) = connections.join_next().await {
        report_connection_completion(Some(completed));
    }
    shutdown_result
}

async fn serve_connection(
    stream: TcpStream,
    peer_address: SocketAddr,
    app: Router,
    _admission: ConnectionAdmission,
    mut shutdown: watch::Receiver<bool>,
    idle_timeouts: HttpIdleTimeouts,
) {
    let activity = Arc::new(ConnectionActivity::new());
    let tracked_stream = TrackedIo::new(stream, Arc::clone(&activity));
    let idle_timeout = monitor_connection_idle(Arc::clone(&activity), idle_timeouts);
    tokio::pin!(idle_timeout);
    let service =
        service_fn(move |mut request: hyper::Request<Incoming>| {
            request.extensions_mut().insert(ConnectInfo(peer_address));
            let app = app.clone();
            let activity = Arc::clone(&activity);
            async move {
                activity.set_phase(ConnectionIdlePhase::Paused);
                let response = app
                    .oneshot(request.map(|body| {
                        Body::new(TrackedRequestBody::new(body, Arc::clone(&activity)))
                    }))
                    .await
                    .unwrap_or_else(|error| match error {});
                Ok::<_, Infallible>(
                    response.map(|body| Body::new(TrackedResponseBody::new(body, activity))),
                )
            }
        });
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(HTTP1_HEADER_READ_TIMEOUT)
        .max_headers(HTTP1_MAX_HEADERS)
        .max_buf_size(HTTP1_MAX_READ_BUFFER_BYTES)
        .pipeline_flush(false)
        .half_close(false)
        .keep_alive(true);
    let connection = builder.serve_connection(TokioIo::new(tracked_stream), service);
    tokio::pin!(connection);

    tokio::select! {
        result = &mut connection => {
            if let Err(error) = result {
                tracing::debug!(%peer_address, %error, "HTTP/1 connection closed with an error");
            }
        }
        phase = &mut idle_timeout => {
            tracing::debug!(%peer_address, ?phase, "HTTP/1 connection idle deadline expired");
        }
        changed = shutdown.changed() => {
            if changed.is_ok() && *shutdown.borrow() {
                connection.as_mut().graceful_shutdown();
                tokio::select! {
                    result = &mut connection => {
                        if let Err(error) = result {
                            tracing::debug!(%peer_address, %error, "HTTP/1 connection failed during graceful shutdown");
                        }
                    }
                    phase = &mut idle_timeout => {
                        tracing::debug!(%peer_address, ?phase, "HTTP/1 connection idle deadline expired during graceful shutdown");
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum ConnectionIdlePhase {
    KeepAlive = 0,
    Paused = 1,
    RequestBody = 2,
    ResponseWrite = 3,
}

impl ConnectionIdlePhase {
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::KeepAlive,
            1 => Self::Paused,
            2 => Self::RequestBody,
            3 => Self::ResponseWrite,
            _ => unreachable!("connection idle phase is written only from the closed enum"),
        }
    }

    fn timeout(self, timeouts: HttpIdleTimeouts) -> Option<Duration> {
        match self {
            Self::KeepAlive => Some(timeouts.keep_alive),
            Self::Paused => None,
            Self::RequestBody => Some(timeouts.body),
            Self::ResponseWrite => Some(timeouts.response_write),
        }
    }
}

struct ConnectionActivity {
    phase: AtomicU8,
    generation: AtomicU64,
    changed: Notify,
}

impl ConnectionActivity {
    fn new() -> Self {
        Self {
            phase: AtomicU8::new(ConnectionIdlePhase::KeepAlive as u8),
            generation: AtomicU64::new(0),
            changed: Notify::new(),
        }
    }

    fn phase(&self) -> ConnectionIdlePhase {
        ConnectionIdlePhase::from_u8(self.phase.load(Ordering::Acquire))
    }

    fn set_phase(&self, phase: ConnectionIdlePhase) {
        if self.phase.swap(phase as u8, Ordering::AcqRel) != phase as u8 {
            self.record_activity();
        }
    }

    fn record_activity(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.changed.notify_one();
    }
}

async fn monitor_connection_idle(
    activity: Arc<ConnectionActivity>,
    timeouts: HttpIdleTimeouts,
) -> ConnectionIdlePhase {
    loop {
        let changed = activity.changed.notified();
        let generation = activity.generation.load(Ordering::Acquire);
        let phase = activity.phase();
        let Some(timeout) = phase.timeout(timeouts) else {
            changed.await;
            continue;
        };
        if tokio::time::timeout(timeout, changed).await.is_err()
            && generation == activity.generation.load(Ordering::Acquire)
            && phase == activity.phase()
        {
            return phase;
        }
    }
}

struct TrackedIo {
    inner: TcpStream,
    activity: Arc<ConnectionActivity>,
}

impl TrackedIo {
    fn new(inner: TcpStream, activity: Arc<ConnectionActivity>) -> Self {
        Self { inner, activity }
    }
}

impl AsyncRead for TrackedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let filled_before = buffer.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(context, buffer);
        if matches!(result, Poll::Ready(Ok(()))) && buffer.filled().len() > filled_before {
            self.activity.record_activity();
        }
        result
    }
}

impl AsyncWrite for TrackedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let result = Pin::new(&mut self.inner).poll_write(context, buffer);
        if matches!(result, Poll::Ready(Ok(written)) if written > 0) {
            self.activity.record_activity();
        }
        result
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

struct TrackedRequestBody {
    inner: Incoming,
    activity: Arc<ConnectionActivity>,
}

impl TrackedRequestBody {
    fn new(inner: Incoming, activity: Arc<ConnectionActivity>) -> Self {
        Self { inner, activity }
    }
}

impl HttpBody for TrackedRequestBody {
    type Data = hyper::body::Bytes;
    type Error = hyper::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.activity.set_phase(ConnectionIdlePhase::RequestBody);
        let result = Pin::new(&mut self.inner).poll_frame(context);
        if result.is_ready() {
            self.activity.set_phase(ConnectionIdlePhase::Paused);
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

struct TrackedResponseBody {
    inner: Body,
    activity: Arc<ConnectionActivity>,
}

impl TrackedResponseBody {
    fn new(inner: Body, activity: Arc<ConnectionActivity>) -> Self {
        Self { inner, activity }
    }
}

impl HttpBody for TrackedResponseBody {
    type Data = hyper::body::Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.activity.set_phase(ConnectionIdlePhase::Paused);
        let result = Pin::new(&mut self.inner).poll_frame(context);
        match &result {
            Poll::Ready(Some(Ok(frame))) if frame.is_data() => {
                self.activity.set_phase(ConnectionIdlePhase::ResponseWrite);
            }
            Poll::Ready(None) => self.activity.set_phase(ConnectionIdlePhase::KeepAlive),
            _ => {}
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

fn report_connection_completion(completed: Option<Result<(), tokio::task::JoinError>>) {
    if let Some(Err(error)) = completed {
        if !error.is_cancelled() {
            tracing::error!(%error, "HTTP/1 connection task panicked");
        }
    }
}
