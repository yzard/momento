// Temporary MOV-preview backfill. Remove this module and restore the executor's
// direct operations::queue_incomplete_metadata call after existing MOVs are rebuilt.
pub(crate) fn queue_with_temporary_mov_preview_backfill(
    connection: &mut rusqlite::Connection,
) -> rusqlite::Result<usize> {
    let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let ordinary = super::operations::queue_incomplete_metadata(&transaction)?;
    let backfilled = transaction.execute(include_str!("temporary_mov_preview_backfill.sql"), [])?;
    transaction.commit()?;
    if backfilled > 0 {
        tracing::info!(queued_mov_previews = backfilled, "Temporary MOV preview backfill queued by Generate Metadata");
    }
    Ok(ordinary + backfilled)
}
