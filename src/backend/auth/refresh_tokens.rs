use crate::database::{queries, DbPool};
use crate::error::{AppError, AppResult};

pub fn cleanup_refresh_tokens(pool: &DbPool) -> AppResult<usize> {
    let connection = pool.get().map_err(AppError::Pool)?;
    connection
        .execute(queries::auth::DELETE_EXPIRED_OR_REVOKED_TOKENS, [])
        .map_err(AppError::from)
}
