use std::time::Duration;

use axum::{
    body::Bytes,
    routing::{get, post},
    Router,
};
use momento_api::runtime::HttpIdleTimeouts;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn owned_http1_server_enforces_header_count_and_graceful_connection_shutdown() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = listener.local_addr().expect("listener address");
    let app = Router::new().route("/", get(|| async { "ok" }));
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let server_scheduler = scheduler.clone();
    let server = tokio::spawn(async move {
        momento_api::runtime::serve_http1(
            listener,
            app,
            server_scheduler,
            async move {
                let _ = shutdown_receiver.await;
            },
            Duration::from_secs(1),
            HttpIdleTimeouts::SOURCE_OWNED,
        )
        .await
        .expect("HTTP/1 server");
    });

    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("client connection");
    let mut request = String::from("GET / HTTP/1.1\r\nHost: localhost\r\n");
    for index in 0..128 {
        request.push_str(&format!("x-test-{index}: value\r\n"));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("request write");
    let mut response = vec![0_u8; 512];
    let bytes_read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut response))
        .await
        .expect("response timeout")
        .expect("response read");
    let response = String::from_utf8_lossy(&response[..bytes_read]);
    assert!(response.starts_with("HTTP/1.1 431"), "{response}");

    drop(stream);
    shutdown_sender.send(()).expect("shutdown signal");
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("server shutdown timeout")
        .expect("server task");
    assert_eq!(scheduler.active_connection_total(), 0);
    assert_eq!(
        scheduler.state(),
        momento_api::runtime::SchedulerState::Stopped
    );
}

#[tokio::test]
async fn owned_http1_server_aborts_a_stuck_request_at_the_shutdown_deadline() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = listener.local_addr().expect("listener address");
    let app = Router::new().route(
        "/stuck",
        get(|| async { std::future::pending::<&'static str>().await }),
    );
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let server_scheduler = scheduler.clone();
    let server = tokio::spawn(async move {
        momento_api::runtime::serve_http1(
            listener,
            app,
            server_scheduler,
            async move {
                let _ = shutdown_receiver.await;
            },
            Duration::from_millis(100),
            HttpIdleTimeouts::SOURCE_OWNED,
        )
        .await
    });

    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("client connection");
    stream
        .write_all(b"GET /stuck HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("request write");
    tokio::time::sleep(Duration::from_millis(25)).await;
    shutdown_sender.send(()).expect("shutdown signal");

    let error = tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .expect("bounded server shutdown")
        .expect("server task")
        .expect_err("stuck request must exhaust shutdown grace");
    assert!(error.to_string().contains("shutdown grace expired"));
    assert_eq!(scheduler.active_connection_total(), 0);
}

#[tokio::test]
async fn owned_http1_server_times_out_socket_idle_phases_but_pauses_for_handler_work() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = listener.local_addr().expect("listener address");
    let app = Router::new()
        .route("/body", post(|_body: Bytes| async { "received" }))
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(150)).await;
                "completed"
            }),
        );
    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
    let server_scheduler = scheduler.clone();
    let server = tokio::spawn(async move {
        momento_api::runtime::serve_http1(
            listener,
            app,
            server_scheduler,
            async move {
                let _ = shutdown_receiver.await;
            },
            Duration::from_secs(1),
            HttpIdleTimeouts::new(
                Duration::from_millis(75),
                Duration::from_millis(75),
                Duration::from_millis(75),
            ),
        )
        .await
        .expect("HTTP/1 server");
    });

    let mut partial_body = tokio::net::TcpStream::connect(address)
        .await
        .expect("partial-body connection");
    partial_body
        .write_all(b"POST /body HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nx")
        .await
        .expect("partial body write");
    let mut byte = [0_u8; 1];
    let bytes_read = tokio::time::timeout(Duration::from_millis(500), partial_body.read(&mut byte))
        .await
        .expect("body idle deadline")
        .expect("body idle close");
    assert_eq!(bytes_read, 0);

    let mut slow_request = tokio::net::TcpStream::connect(address)
        .await
        .expect("slow-handler connection");
    slow_request
        .write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("slow-handler request");
    let mut response = vec![0_u8; 512];
    let bytes_read = tokio::time::timeout(Duration::from_secs(1), slow_request.read(&mut response))
        .await
        .expect("slow-handler response timeout")
        .expect("slow-handler response");
    let response = String::from_utf8_lossy(&response[..bytes_read]);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");

    shutdown_sender.send(()).expect("shutdown signal");
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("server shutdown timeout")
        .expect("server task");
    assert_eq!(scheduler.active_connection_total(), 0);
}
