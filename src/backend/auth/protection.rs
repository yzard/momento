use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::HeaderMap;

use crate::config::SecurityConfig;
use crate::database::operations::{AuthAttemptDecision, ClearAuthAttempts, RegisterAuthAttempt};
use crate::error::{AppError, AppResult};
use crate::executor::{CpuExecutorHandle, SqliteExecutorHandle};

#[derive(Clone)]
pub struct AuthenticationProtection {
    inner: Arc<AuthenticationProtectionInner>,
}

struct AuthenticationProtectionInner {
    cpu: CpuExecutorHandle,
    sqlite: SqliteExecutorHandle,
    attempt_window_seconds: u64,
    identity_limit: u32,
    source_limit: u32,
    lockout_seconds: u64,
    trusted_proxy_ip_addresses: Vec<IpAddr>,
    dummy_password_hash: String,
}

impl AuthenticationProtection {
    pub fn new(
        config: &SecurityConfig,
        cpu: CpuExecutorHandle,
        sqlite: SqliteExecutorHandle,
        dummy_password_hash: String,
    ) -> Self {
        Self {
            inner: Arc::new(AuthenticationProtectionInner {
                cpu,
                sqlite,
                attempt_window_seconds: config.password_attempt_window_seconds,
                identity_limit: config.password_attempts_per_identity,
                source_limit: config.password_attempts_per_source,
                lockout_seconds: config.password_lockout_seconds,
                trusted_proxy_ip_addresses: config.trusted_proxy_ip_addresses.clone(),
                dummy_password_hash,
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

    pub async fn begin_password_attempt(&self, source: &str, identity: &str) -> AppResult<()> {
        let (source_key, identity_key) = self
            .inner
            .cpu
            .auth_attempt_digests_durable(source.to_string(), identity.to_string())
            .await
            .map_err(AppError::from)?;
        let decision = self
            .inner
            .sqlite
            .register_auth_attempt_request(RegisterAuthAttempt {
                source_key,
                identity_key,
                now_epoch_seconds: epoch_seconds()?,
                attempt_window_seconds: duration_to_i64(
                    self.inner.attempt_window_seconds,
                    "password attempt window",
                )?,
                identity_limit: self.inner.identity_limit,
                source_limit: self.inner.source_limit,
                lockout_seconds: duration_to_i64(self.inner.lockout_seconds, "password lockout")?,
            })
            .await
            .map_err(AppError::from)?;
        match decision {
            AuthAttemptDecision::Allowed => Ok(()),
            AuthAttemptDecision::RateLimited {
                retry_after_seconds,
            }
            | AuthAttemptDecision::CapacityExhausted {
                retry_after_seconds,
            } => Err(AppError::RateLimited {
                retry_after_seconds,
            }),
        }
    }

    pub async fn record_password_success(&self, source: &str, identity: &str) -> AppResult<()> {
        let (source_key, identity_key) = self
            .inner
            .cpu
            .auth_attempt_digests_durable(source.to_string(), identity.to_string())
            .await
            .map_err(AppError::from)?;
        self.inner
            .sqlite
            .clear_auth_attempts_request(ClearAuthAttempts {
                source_key,
                identity_key,
            })
            .await
            .map_err(AppError::from)
    }

    pub async fn verify_password(&self, password: &str, hash: Option<&str>) -> AppResult<bool> {
        self.inner
            .cpu
            .verify_password_durable(
                password.to_string(),
                hash.map(str::to_string),
                self.inner.dummy_password_hash.clone(),
            )
            .await
            .map_err(AppError::from)
    }

    pub async fn hash_password(&self, password: &str) -> AppResult<String> {
        self.inner
            .cpu
            .hash_password_durable(password.to_string())
            .await
            .map_err(AppError::from)
    }
}

fn epoch_seconds() -> AppResult<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::Internal(format!("system clock precedes Unix epoch: {error}")))?
        .as_secs();
    duration_to_i64(seconds, "system clock")
}

fn duration_to_i64(value: u64, name: &str) -> AppResult<i64> {
    i64::try_from(value)
        .map_err(|_| AppError::Internal(format!("{name} exceeds the supported duration")))
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
