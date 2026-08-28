use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use crate::auth::{hash_password, verify_password_or_dummy};
use crate::config::SecurityConfig;
use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub struct AuthenticationProtection {
    inner: Arc<AuthenticationProtectionInner>,
}

struct AuthenticationProtectionInner {
    attempts: Mutex<AttemptState>,
    attempt_window: Duration,
    identity_limit: u32,
    source_limit: u32,
    lockout: Duration,
    password_hash_slots: Arc<Semaphore>,
    trusted_proxy_ip_addresses: Vec<IpAddr>,
}

#[derive(Default)]
struct AttemptState {
    buckets: HashMap<String, AttemptBucket>,
}

struct AttemptBucket {
    window_started: Instant,
    last_seen: Instant,
    attempts: u32,
    locked_until: Option<Instant>,
}

impl AuthenticationProtection {
    pub fn new(config: &SecurityConfig) -> Self {
        Self {
            inner: Arc::new(AuthenticationProtectionInner {
                attempts: Mutex::new(AttemptState::default()),
                attempt_window: Duration::from_secs(config.password_attempt_window_seconds),
                identity_limit: config.password_attempts_per_identity,
                source_limit: config.password_attempts_per_source,
                lockout: Duration::from_secs(config.password_lockout_seconds),
                password_hash_slots: Arc::new(Semaphore::new(config.password_hash_max_concurrent)),
                trusted_proxy_ip_addresses: config.trusted_proxy_ip_addresses.clone(),
            }),
        }
    }

    pub fn client_source(&self, headers: &HeaderMap, peer_address: Option<SocketAddr>) -> String {
        client_source(
            headers,
            peer_address,
            &self.inner.trusted_proxy_ip_addresses,
        )
    }

    pub fn begin_password_attempt(&self, source: &str, identity: &str) -> AppResult<()> {
        let now = Instant::now();
        let mut state = self.inner.attempts.lock().map_err(|_| {
            AppError::Internal("Authentication attempt state is unavailable".into())
        })?;
        state.prune(now, self.inner.attempt_window, self.inner.lockout);

        let source_retry = state.register(
            attempt_key("source", source),
            self.inner.source_limit,
            now,
            self.inner.attempt_window,
            self.inner.lockout,
        );
        let identity_retry = state.register(
            attempt_key("identity", &identity.to_lowercase()),
            self.inner.identity_limit,
            now,
            self.inner.attempt_window,
            self.inner.lockout,
        );
        match source_retry.into_iter().chain(identity_retry).max() {
            Some(retry_after_seconds) => Err(AppError::RateLimited {
                retry_after_seconds,
            }),
            None => Ok(()),
        }
    }

    pub fn record_password_success(&self, source: &str, identity: &str) {
        let Ok(mut state) = self.inner.attempts.lock() else {
            return;
        };
        state
            .buckets
            .remove(&attempt_key("identity", &identity.to_lowercase()));
        state.buckets.remove(&attempt_key("source", source));
    }

    pub async fn verify_password(&self, password: &str, hash: Option<&str>) -> AppResult<bool> {
        let permit = Arc::clone(&self.inner.password_hash_slots)
            .acquire_owned()
            .await
            .map_err(|_| AppError::Internal("Password verification is unavailable".into()))?;
        let password = password.to_string();
        let hash = hash.map(str::to_string);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            verify_password_or_dummy(&password, hash.as_deref())
        })
        .await
        .map_err(|error| AppError::Internal(format!("Password verification failed: {error}")))
    }

    pub async fn hash_password(&self, password: &str) -> AppResult<String> {
        let permit = Arc::clone(&self.inner.password_hash_slots)
            .acquire_owned()
            .await
            .map_err(|_| AppError::Internal("Password hashing is unavailable".into()))?;
        let password = password.to_string();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            hash_password(&password)
        })
        .await
        .map_err(|error| AppError::Internal(format!("Password hashing failed: {error}")))?
        .map_err(|error| AppError::Internal(format!("Failed to hash password: {error}")))
    }
}

impl AttemptState {
    fn register(
        &mut self,
        key: String,
        limit: u32,
        now: Instant,
        attempt_window: Duration,
        lockout: Duration,
    ) -> Option<u64> {
        let bucket = self.buckets.entry(key).or_insert(AttemptBucket {
            window_started: now,
            last_seen: now,
            attempts: 0,
            locked_until: None,
        });
        bucket.last_seen = now;

        if let Some(locked_until) = bucket.locked_until {
            if locked_until > now {
                return Some(remaining_seconds(locked_until, now));
            }
            bucket.locked_until = None;
            bucket.window_started = now;
            bucket.attempts = 0;
        }
        if now.duration_since(bucket.window_started) >= attempt_window {
            bucket.window_started = now;
            bucket.attempts = 0;
        }
        if bucket.attempts >= limit {
            let locked_until = now + lockout;
            bucket.locked_until = Some(locked_until);
            return Some(remaining_seconds(locked_until, now));
        }
        bucket.attempts += 1;
        None
    }

    fn prune(&mut self, now: Instant, attempt_window: Duration, lockout: Duration) {
        let retention = attempt_window.max(lockout).saturating_mul(2);
        self.buckets.retain(|_, bucket| {
            bucket.locked_until.is_some_and(|until| until > now)
                || now.duration_since(bucket.last_seen) < retention
        });
    }
}

fn attempt_key(namespace: &str, value: &str) -> String {
    let digest = Sha256::digest(format!("{namespace}\0{value}").as_bytes());
    format!("{digest:x}")
}

fn remaining_seconds(deadline: Instant, now: Instant) -> u64 {
    deadline.duration_since(now).as_secs().max(1)
}

fn client_source(
    headers: &HeaderMap,
    peer_address: Option<SocketAddr>,
    trusted_proxy_ip_addresses: &[IpAddr],
) -> String {
    let Some(peer_address) = peer_address else {
        return "unknown".to_string();
    };
    let peer_ip = peer_address.ip();
    if !trusted_proxy_ip_addresses.contains(&peer_ip) {
        return peer_ip.to_string();
    }

    if let Some(forwarded_for) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        let parsed_addresses = forwarded_for
            .split(',')
            .map(str::trim)
            .map(str::parse::<IpAddr>)
            .collect::<Result<Vec<_>, _>>();
        if let Ok(addresses) = parsed_addresses {
            if let Some(client_ip) = addresses
                .iter()
                .rev()
                .find(|address| !trusted_proxy_ip_addresses.contains(address))
                .or_else(|| addresses.first())
            {
                return client_ip.to_string();
            }
        }
    }
    if let Some(real_ip) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<IpAddr>().ok())
    {
        return real_ip.to_string();
    }
    peer_ip.to_string()
}
