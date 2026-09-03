use crate::error::AppResult;

pub async fn cleanup_refresh_tokens(
    sqlite: &crate::executor::SqliteExecutorHandle,
) -> AppResult<usize> {
    sqlite
        .cleanup_refresh_tokens_durable()
        .await
        .map_err(Into::into)
}
