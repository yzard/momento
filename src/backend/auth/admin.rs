use std::sync::{
    atomic::{AtomicBool, AtomicI64, Ordering},
    Arc,
};

use crate::auth::hash_password;
use crate::database::{execute_query, fetch_one, queries, DbPool};
use crate::error::{AppError, AppResult};

pub const TEMPORARY_ADMIN_USERNAME: &str = "admin";
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

pub fn ensure_default_admin(pool: &DbPool) -> AppResult<i64> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let existing = fetch_one(&connection, queries::users::CHECK_ADMIN, &[], |row| {
        row.get::<_, i64>(0)
    })?;
    if let Some(admin_id) = existing {
        return Ok(admin_id);
    }

    let password_hash = hash_password(TEMPORARY_ADMIN_PASSWORD)
        .map_err(|error| AppError::Internal(format!("Failed to hash admin password: {error}")))?;
    let email = format!("{TEMPORARY_ADMIN_USERNAME}@localhost");
    connection.execute(
        queries::users::INSERT_ADMIN,
        (TEMPORARY_ADMIN_USERNAME, &email, &password_hash),
    )?;
    Ok(connection.last_insert_rowid())
}

pub fn prepare_admin_password_reset(pool: &DbPool, admin_id: i64) -> AppResult<()> {
    let connection = pool.get().map_err(AppError::Pool)?;
    let admin_exists = fetch_one(
        &connection,
        queries::users::CHECK_ADMIN_BY_ID,
        &[&admin_id],
        |row| row.get::<_, i64>(0),
    )?
    .is_some();
    if !admin_exists {
        return Err(AppError::NotFound(
            "Administrator account not found".to_string(),
        ));
    }
    execute_query(
        &connection,
        queries::auth::REVOKE_ALL_USER_TOKENS,
        &[&admin_id],
    )?;
    Ok(())
}
