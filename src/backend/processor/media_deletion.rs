use crate::database::{queries, DbConn};
use crate::error::AppResult;
use crate::processor::media_processor::delete_media_files;

pub fn permanently_delete_for_user(
    connection: &DbConn,
    media_id: i64,
    user_id: i64,
    file_path: &str,
    thumbnail_path: Option<&str>,
) -> AppResult<bool> {
    let transaction = connection.unchecked_transaction()?;
    let removed_access = transaction.execute(
        queries::trash::DELETE_ACCESS,
        rusqlite::params![media_id, user_id],
    )?;
    if removed_access == 0 {
        transaction.rollback()?;
        return Ok(false);
    }
    let access_count =
        transaction.query_row(queries::trash::CHECK_ACCESS_COUNT, [media_id], |row| {
            row.get::<_, i64>(0)
        })?;
    if access_count > 0 {
        transaction.commit()?;
        return Ok(false);
    }
    transaction.execute(queries::trash::DELETE_PERMANENTLY, [media_id])?;
    transaction.execute(queries::trash::DELETE_EMPTY_FACE_GROUPS, [])?;
    transaction.commit()?;
    delete_media_files(media_id, file_path, thumbnail_path);
    Ok(true)
}
