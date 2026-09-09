use super::*;

// Drain ready work, never wait to fill a batch or jump past another command.
const MAX_COMMANDS: usize = 16;
const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn eligible(
    operation: &SqliteOperation,
    footprints: &crate::database::result_footprint::SqliteFootprintRegistry,
) -> bool {
    matches!(
        operation,
        SqliteOperation::StageLlmResultPage(_)
            | SqliteOperation::PersistPreparedLlmResult(_)
            | SqliteOperation::CleanupLlmResultStagingPage { .. }
    ) && matches!(
        operation.spec(footprints),
        Ok(SqliteOperationSpec {
            capacity: SqliteCapacitySource::DurableParent { .. },
            ..
        })
    )
}

pub(super) fn collect(
    first: SqliteCommand,
    receiver: &Receiver<SqliteCommand>,
    footprints: &crate::database::result_footprint::SqliteFootprintRegistry,
    capacity_wake: &Notify,
) -> (Vec<SqliteCommand>, Option<SqliteCommand>) {
    let mut input_bytes = first
        .operation
        .spec(footprints)
        .expect("eligible operation")
        .resources
        .maximum_input_bytes;
    let mut batch = vec![first];
    while batch.len() < MAX_COMMANDS {
        let Ok(next) = receiver.try_recv() else { break };
        capacity_wake.notify_one();
        if !eligible(&next.operation, footprints)
            || batch.iter().any(|command| {
                command.operation.durable_parent_job_id() == next.operation.durable_parent_job_id()
            })
        {
            return (batch, Some(next));
        }
        let next_bytes = next
            .operation
            .spec(footprints)
            .expect("eligible operation")
            .resources
            .maximum_input_bytes;
        if input_bytes.saturating_add(next_bytes) > MAX_INPUT_BYTES {
            return (batch, Some(next));
        }
        input_bytes += next_bytes;
        batch.push(next);
    }
    (batch, None)
}

pub(super) fn execute(commands: Vec<SqliteCommand>, context: &SqliteWorkerContext) {
    let started = Instant::now();
    let count = commands.len();
    let (operations, replies): (Vec<_>, Vec<_>) = commands
        .into_iter()
        .map(|command| (command.operation, command.reply))
        .unzip();
    let outcome = catch_unwind(AssertUnwindSafe(|| execute_batch(operations, context)));
    let results = match outcome {
        Ok(Ok(results)) => results,
        failure => {
            let error = match failure {
                Ok(Err(error)) => error,
                Err(_) => ExecutorError::new(
                    ExecutorErrorKind::WorkerPanic,
                    "sqlite_write_batch",
                    "SQLite write batch panicked and was rolled back",
                ),
                Ok(Ok(_)) => unreachable!(),
            };
            tracing::warn!(command_count = count, error = %error, "SQLite write batch failed; callers retain durable work for retry");
            (0..count)
                .map(|_| {
                    Err(ExecutorError::new(
                        error.kind,
                        error.operation,
                        error.detail.clone(),
                    ))
                })
                .collect()
        }
    };
    tracing::debug!(
        command_count = count,
        successful_commands = results.iter().filter(|result| result.is_ok()).count(),
        elapsed_ms = started.elapsed().as_millis(),
        "SQLite writer finished ready FIFO batch"
    );
    for (reply, result) in replies.into_iter().zip(results) {
        let _ = reply.send(result);
    }
}

fn execute_batch(
    operations: Vec<SqliteOperation>,
    context: &SqliteWorkerContext,
) -> Result<Vec<Result<SqliteOutput, ExecutorError>>, ExecutorError> {
    let name = "sqlite_write_batch";
    let growth = operations.iter().try_fold(0_u64, |total, operation| {
        let SqliteCapacitySource::DurableParent { max_growth_bytes } =
            operation.spec(&context.footprints)?.capacity
        else {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                name,
                "non-batchable operation",
            ));
        };
        total.checked_add(max_growth_bytes).ok_or_else(|| {
            ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                name,
                "batch growth overflow",
            )
        })
    })?;
    let mut connection = context
        .pool
        .get_timeout(SQLITE_CONNECTION_TIMEOUT)
        .map_err(|error| {
            ExecutorError::new(ExecutorErrorKind::DatabaseBusy, name, error.to_string())
        })?;
    connection
        .pragma_update(None, "query_only", false)
        .map_err(|error| map_sqlite_error(name, error))?;
    ensure_sqlite_wal_capacity(
        &mut connection,
        &context.database_path,
        context.space_budget.sqlite_wal_limit_bytes(),
        growth,
        name,
    )?;
    let deadline = Instant::now() + SQLITE_OPERATION_TIMEOUT;
    connection.progress_handler(
        SQLITE_PROGRESS_HANDLER_OPS,
        Some(move || Instant::now() >= deadline),
    );
    // Always remove the progress handler, including when a request panics.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(name, error))?;
        let mut results = Vec::with_capacity(operations.len());
        let mut reservations = Vec::new();
        for operation in operations {
            match completed_cleanup_output(
                &transaction,
                &operation,
                &context.space_budget,
                &context.database_path,
            ) {
                Ok(Some(output)) => {
                    results.push(Ok(output));
                    continue;
                }
                Err(error) => {
                    results.push(Err(error));
                    continue;
                }
                Ok(None) => {}
            }
            let operation_name = operation.name();
            let prepared = (|| {
                let job_id = operation.durable_parent_job_id().ok_or_else(|| {
                    ExecutorError::new(
                        ExecutorErrorKind::InvalidInput,
                        operation_name,
                        "missing result owner",
                    )
                })?;
                let record = load_result_sqlite_reservation(&transaction, job_id, operation_name)?;
                let SqliteCapacitySource::DurableParent { max_growth_bytes } =
                    operation.spec(&context.footprints)?.capacity
                else {
                    unreachable!()
                };
                if max_growth_bytes > record.reserved_peak_additional_bytes {
                    return Err(ExecutorError::new(
                        ExecutorErrorKind::DatabasePermanent,
                        operation_name,
                        "SQLite child footprint exceeds its durable result parent",
                    ));
                }
                let checkout =
                    context
                        .space_budget
                        .reacquire_durable(&record)
                        .map_err(|error| {
                            ExecutorError::new(
                                ExecutorErrorKind::DatabaseBusy,
                                operation_name,
                                error.to_string(),
                            )
                        })?;
                let capacity = operations::SqliteResultCapacityChild {
                    reservation_id: record.reservation_id.clone(),
                    expected_version: record.version,
                    max_growth_bytes,
                    cleanup_remaining_bytes: context
                        .footprints
                        .result_cleanup_recovery_max_growth_bytes,
                };
                Ok((record, checkout, capacity))
            })();
            let (record, checkout, capacity) = match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    results.push(Err(error));
                    continue;
                }
            };
            // Each implementation owns a savepoint. A bad request rolls back
            // only its own changes, never a previous request in this batch.
            let savepoint = transaction
                .savepoint()
                .map_err(|error| map_sqlite_error(operation_name, error))?;
            let result = execute_result_write(savepoint, operation, Some(&capacity));
            if result.is_ok() {
                let terminal_cleanup = matches!(&result, Ok(SqliteOutput::LlmResultStagingCleaned(outcome)) if outcome.complete);
                let refreshed = if terminal_cleanup {
                    record
                } else {
                    load_result_sqlite_reservation(&transaction, &record.owner_id, operation_name)?
                };
                reservations.push((results.len(), refreshed, checkout, terminal_cleanup));
            }
            results.push(result);
        }
        transaction
            .commit()
            .map_err(|error| map_sqlite_error(name, error))?;
        for (index, record, checkout, terminal_cleanup) in reservations {
            let publication = if terminal_cleanup {
                drop(checkout);
                context
                    .space_budget
                    .release_sqlite_after_terminal_commit(
                        &record.reservation_id,
                        &context.database_path,
                        name,
                    )
                    .map(|_| ())
            } else {
                checkout.publish_sqlite_child(&record, &context.database_path, name)
            };
            if let Err(error) = publication {
                results[index] = Err(ExecutorError::new(
                    ExecutorErrorKind::Internal,
                    name,
                    error.to_string(),
                ));
            }
        }
        Ok(results)
    }));
    connection.progress_handler(0, None::<fn() -> bool>);
    match outcome {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

pub(super) fn execute_result_write(
    savepoint: rusqlite::Savepoint<'_>,
    operation: SqliteOperation,
    capacity: Option<&operations::SqliteResultCapacityChild>,
) -> Result<SqliteOutput, ExecutorError> {
    let name = operation.name();
    match operation {
        SqliteOperation::CleanupLlmResultStagingPage { job_id, limit } => {
            operations::cleanup_llm_result_staging_page(savepoint, &job_id, i64::from(limit))
                .map(SqliteOutput::LlmResultStagingCleaned)
                .map_err(|error| map_sqlite_error(name, error))
        }
        SqliteOperation::StageLlmResultPage(request) => {
            let capacity = capacity.ok_or_else(|| {
                ExecutorError::new(
                    ExecutorErrorKind::Internal,
                    name,
                    "LLM staging is missing its durable SQLite capacity child",
                )
            })?;
            operations::stage_llm_result_page(savepoint, *request, capacity)
                .map(SqliteOutput::LlmResultPageStaged)
                .map_err(|error| map_sqlite_error(name, error))
        }
        SqliteOperation::PersistPreparedLlmResult(prepared) => {
            crate::processor::ai::result::persist_prepared_result(savepoint, prepared, capacity)
                .map(SqliteOutput::PreparedLlmResultPersisted)
                .map_err(|error| map_result_app_error(name, error))
        }
        _ => Err(ExecutorError::new(
            ExecutorErrorKind::InvalidInput,
            name,
            "operation cannot execute in a result write savepoint",
        )),
    }
}

#[cfg(test)]
#[path = "../../../../tests/backend/executor/sqlite/write_batch.rs"]
mod tests;
