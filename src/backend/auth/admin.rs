use std::sync::{
    atomic::{AtomicBool, AtomicI64, Ordering},
    Arc,
};

use crate::error::{AppError, AppResult};
use crate::runtime::ExecutorHandles;

pub const RESERVED_ADMIN_USERNAME: &str = "admin";
pub const TEMPORARY_ADMIN_USERNAME: &str = RESERVED_ADMIN_USERNAME;
pub const TEMPORARY_ADMIN_PASSWORD: &str = "admin";

#[derive(Clone, Default)]
pub struct AdminPasswordReset {
    user_id: Arc<AtomicI64>,
    credentials_active: Arc<AtomicBool>,
    reset_id: Arc<String>,
}

impl AdminPasswordReset {
    pub fn new(user_id: Option<i64>) -> Self {
        Self {
            user_id: Arc::new(AtomicI64::new(user_id.unwrap_or_default())),
            credentials_active: Arc::new(AtomicBool::new(user_id.is_some())),
            reset_id: Arc::new(
                user_id
                    .map(|_| uuid::Uuid::new_v4().to_string())
                    .unwrap_or_default(),
            ),
        }
    }

    fn configured_user_id(&self) -> Option<i64> {
        match self.user_id.load(Ordering::Acquire) {
            0 => None,
            user_id => Some(user_id),
        }
    }

    pub fn login(&self) -> Option<(i64, String)> {
        if !self.credentials_active.load(Ordering::Acquire) {
            return None;
        }
        self.configured_user_id()
            .map(|user_id| (user_id, self.reset_id.as_ref().clone()))
    }

    pub fn requires_password_change(&self, user_id: i64) -> bool {
        self.login()
            .is_some_and(|(reset_user_id, _)| reset_user_id == user_id)
    }

    pub fn accepts_temporary_token(&self, user_id: i64, reset_id: &str) -> bool {
        self.configured_user_id() == Some(user_id) && self.reset_id.as_str() == reset_id
    }

    pub fn complete(&self, user_id: i64) {
        if self.configured_user_id() == Some(user_id) {
            self.credentials_active.store(false, Ordering::Release);
        }
    }
}

pub async fn ensure_default_admin(executors: &ExecutorHandles) -> AppResult<i64> {
    if let Some(admin_id) = executors.sqlite.load_admin_id_durable().await? {
        return Ok(admin_id);
    }
    let password_hash = executors
        .cpu
        .hash_password_durable(TEMPORARY_ADMIN_PASSWORD.to_string())
        .await?;
    executors
        .sqlite
        .insert_default_admin_durable(password_hash)
        .await
        .map_err(AppError::from)
}

pub async fn prepare_admin_password_reset(
    executors: &ExecutorHandles,
    admin_id: i64,
) -> AppResult<()> {
    if !executors
        .sqlite
        .prepare_admin_password_reset_durable(admin_id)
        .await?
    {
        return Err(AppError::NotFound(
            "Administrator account not found".to_string(),
        ));
    }
    Ok(())
}
