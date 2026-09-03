use momento_common::llm::result_stream::{
    OwnedResultRecord, ResultInputCorrelation, ResultManifest, ResultRecordChunkDecoder,
    ResultRecordCollector, ResultRecordStreamValidator, ResultStatus, ValidatedResultInput,
    ValidatedResultStream, ValidatedResultValue,
};
use momento_common::llm::{
    ResultRecord, ResultRecordKind, IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS,
    RESULT_RECORD_HEADER_BYTES,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::config::MediaProcessConfig;
use crate::constants::{DOCUMENT_DETECTION_MODEL_TYPE, SCREENSHOT_DETECTION_MODEL_TYPE};
use crate::database::operations::{
    StageLlmResultPage, StageLlmResultPageOutcome, StagedLlmResultRecord,
};
use crate::database::queries;
use crate::error::{AppError, AppResult};
use crate::executor::SqliteExecutorHandle;
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::runtime::{DurableSourceId, SchedulerAdmissionKind};

pub(crate) enum QueuedResult {
    Journal {
        manifest: ResultManifest,
        inbox_path: String,
        next_record_sequence: u32,
        next_byte_offset: u64,
        result_product_version: i64,
        claim_token: String,
    },
}

impl QueuedResult {
    fn job_id(&self) -> &str {
        match self {
            Self::Journal { manifest, .. } => &manifest.job_id,
        }
    }

    fn claim_token(&self) -> Option<&str> {
        match self {
            Self::Journal { claim_token, .. } => Some(claim_token),
        }
    }
}

pub(crate) enum PreparedQueuedResult {
    Result {
        job_id: String,
        claim_token: Option<String>,
        request: PreparedResultRequest,
        face: Option<crate::processor::face_detection::PreparedFaceDetectionResult>,
    },
    PermanentFailure {
        job_id: String,
        claim_token: Option<String>,
        error: String,
    },
}

pub(crate) enum PreparedResultRequest {
    Streamed(Box<StreamedResult>),
}

pub(crate) struct StreamedResult {
    manifest: ResultManifest,
    result: ValidatedResultStream,
    product_version: i64,
}

impl PreparedResultRequest {
    fn job_id(&self) -> &str {
        match self {
            Self::Streamed(result) => &result.manifest.job_id,
        }
    }
}

impl PreparedQueuedResult {
    fn claim_identity(&self) -> (String, Option<String>) {
        match self {
            Self::Result {
                job_id,
                claim_token,
                ..
            }
            | Self::PermanentFailure {
                job_id,
                claim_token,
                ..
            } => (job_id.clone(), claim_token.clone()),
        }
    }

    pub(crate) fn durable_parent_job_id(&self) -> Option<&str> {
        match self {
            Self::Result {
                job_id,
                claim_token: Some(_),
                ..
            }
            | Self::PermanentFailure {
                job_id,
                claim_token: Some(_),
                ..
            } => Some(job_id),
            Self::Result { .. } | Self::PermanentFailure { .. } => None,
        }
    }

    pub(crate) fn durable_sqlite_growth_bound(
        &self,
        footprints: &crate::database::result_footprint::SqliteFootprintRegistry,
    ) -> Result<Option<u64>, crate::database::result_footprint::ResultFootprintError> {
        match self {
            Self::Result {
                claim_token: Some(_),
                request: PreparedResultRequest::Streamed(result),
                ..
            } => footprints
                .persistence(&result.manifest.task, result.manifest.record_count)
                .map(Some),
            Self::PermanentFailure {
                claim_token: Some(_),
                ..
            } => Ok(Some(footprints.result_rejection_max_growth_bytes)),
            Self::Result { .. } | Self::PermanentFailure { .. } => Ok(None),
        }
    }
}

pub async fn run(executors: crate::runtime::ExecutorHandles, process_config: MediaProcessConfig) {
    let scheduler = executors.scheduler.clone();
    let mut observed_version = scheduler.llm_result_work_version();
    loop {
        let processed =
            process_available_results_scheduled(&executors, &process_config, &scheduler).await;
        let retry_after_failure = match processed {
            Ok(processed) if processed > 0 => {
                scheduler.wake_ai_finalization();
                continue;
            }
            Ok(_) => false,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Momento LLM result remains queued and will be retried"
                );
                true
            }
        };
        let current_version = scheduler.llm_result_work_version();
        if current_version != observed_version {
            observed_version = current_version;
            continue;
        }
        if retry_after_failure {
            tokio::select! {
                version = scheduler.wait_for_llm_result_work(observed_version) => {
                    observed_version = version;
                }
                () = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
            }
        } else {
            observed_version = scheduler.wait_for_llm_result_work(observed_version).await;
        }
    }
}

async fn process_available_results_scheduled(
    executors: &crate::runtime::ExecutorHandles,
    process_config: &MediaProcessConfig,
    scheduler: &crate::runtime::SchedulerHandle,
) -> AppResult<usize> {
    let concurrency = scheduler.durable_capacity();
    let mut lanes = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let lane_executors = executors.clone();
        let lane_config = process_config.clone();
        let lane_scheduler = scheduler.clone();
        lanes.push(scheduler.spawn_control(async move {
            process_result_lane(&lane_executors, &lane_config, &lane_scheduler).await
        }));
    }
    let mut processed = 0;
    let mut first_error = None;
    for lane in lanes {
        match lane.await {
            Ok(Ok(lane_processed)) => processed += lane_processed,
            Ok(Err(error)) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(AppError::Internal(format!(
                        "LLM result worker lane panicked: {error}"
                    )));
                }
            }
        }
    }
    let cleanup_pages = process_staging_cleanup_pages(executors, scheduler).await?;
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(processed + cleanup_pages)
}

async fn process_result_lane(
    executors: &crate::runtime::ExecutorHandles,
    process_config: &MediaProcessConfig,
    scheduler: &crate::runtime::SchedulerHandle,
) -> AppResult<usize> {
    let mut processed = 0;
    loop {
        let admission = scheduler
            .acquire_durable(DurableSourceId::LlmResult, SchedulerAdmissionKind::NewClaim)
            .await
            .map_err(AppError::Internal)?;
        let queued_result = executors
            .sqlite
            .select_llm_result_candidates_durable(1)
            .await?
            .into_iter()
            .next();
        let Some(queued_result) = queued_result else {
            drop(admission);
            return Ok(processed);
        };
        let job_id = queued_result.job_id().to_string();
        let claim_token = queued_result.claim_token().map(str::to_string);
        let registration = if let Some(claim_token) = &claim_token {
            match scheduler.register_durable_claim(&admission, claim_token.clone()) {
                Ok(registration) => Some(registration),
                Err(error) => {
                    release_result_claim(&executors.sqlite, &job_id, Some(claim_token)).await?;
                    tracing::warn!(
                        job_id,
                        error,
                        "LLM result claim registration failed and was requeued"
                    );
                    return Err(AppError::Internal(error));
                }
            }
        } else {
            None
        };
        let prepared_result = match prepare_queued_result(executors, queued_result, process_config)
            .await
        {
            Ok(prepared_result) => prepared_result,
            Err(error) if result_error_is_retryable(&error) => {
                release_result_claim(&executors.sqlite, &job_id, claim_token.as_deref()).await?;
                tracing::warn!(
                    job_id,
                    error = %error,
                    "Momento LLM result preparation failed and was moved to the queue tail"
                );
                return Err(error);
            }
            Err(error) => PreparedQueuedResult::PermanentFailure {
                job_id,
                claim_token,
                error: error.to_string(),
            },
        };
        let claim_identity = prepared_result.claim_identity();
        let persistence = executors
            .sqlite
            .persist_prepared_llm_result_durable(prepared_result)
            .await;
        let replaced_crops = match persistence {
            Ok(replaced_crops) => replaced_crops,
            Err(error) => {
                release_result_claim(
                    &executors.sqlite,
                    &claim_identity.0,
                    claim_identity.1.as_deref(),
                )
                .await?;
                tracing::warn!(
                    job_id = claim_identity.0,
                    error = %error,
                    "Momento LLM result persistence failed and was moved to the queue tail"
                );
                return Err(error.into());
            }
        };
        scheduler.wake_journal_recovery();
        crate::processor::face_detection::retire_replaced_crops(executors, replaced_crops).await;
        drop(registration);
        drop(admission);
        processed += 1;
    }
}

pub async fn process_available_results(
    executors: &crate::runtime::ExecutorHandles,
    process_config: MediaProcessConfig,
) -> AppResult<usize> {
    let mut processed = 0;
    loop {
        let queued_result = executors
            .sqlite
            .select_llm_result_candidates_durable(1)
            .await?
            .into_iter()
            .next();
        let Some(queued_result) = queued_result else {
            let cleanup = executors
                .sqlite
                .select_llm_result_staging_cleanup_durable(1)
                .await?
                .into_iter()
                .next();
            let Some(job_id) = cleanup else {
                return Ok(processed);
            };
            let outcome = executors
                .sqlite
                .cleanup_llm_result_staging_page_durable(job_id.clone(), 256)
                .await?;
            if outcome.complete {
                executors
                    .sqlite
                    .finalize_llm_result_cleanup_durable(job_id)
                    .await?;
                executors.scheduler.wake_journal_recovery();
            }
            continue;
        };
        let job_id = queued_result.job_id().to_string();
        let claim_token = queued_result.claim_token().map(str::to_string);
        let prepared_result = match prepare_queued_result(executors, queued_result, &process_config)
            .await
        {
            Ok(prepared_result) => prepared_result,
            Err(error) => {
                release_result_claim(&executors.sqlite, &job_id, claim_token.as_deref()).await?;
                return Err(error);
            }
        };
        let claim_identity = prepared_result.claim_identity();
        let persistence = executors
            .sqlite
            .persist_prepared_llm_result_durable(prepared_result)
            .await;
        let replaced_crops = match persistence {
            Ok(replaced_crops) => replaced_crops,
            Err(error) => {
                release_result_claim(
                    &executors.sqlite,
                    &claim_identity.0,
                    claim_identity.1.as_deref(),
                )
                .await?;
                return Err(error.into());
            }
        };
        executors.scheduler.wake_journal_recovery();
        crate::processor::face_detection::retire_replaced_crops(executors, replaced_crops).await;
        processed += 1;
    }
}

async fn release_result_claim(
    sqlite: &SqliteExecutorHandle,
    job_id: &str,
    claim_token: Option<&str>,
) -> AppResult<()> {
    let Some(claim_token) = claim_token else {
        return Ok(());
    };
    let _released = sqlite
        .release_llm_result_claim_durable(job_id.to_string(), claim_token.to_string())
        .await?;
    Ok(())
}

async fn process_staging_cleanup_pages(
    executors: &crate::runtime::ExecutorHandles,
    scheduler: &crate::runtime::SchedulerHandle,
) -> AppResult<usize> {
    let candidates = {
        let _worker_permit = scheduler
            .acquire_durable(DurableSourceId::LlmResult, SchedulerAdmissionKind::NewClaim)
            .await
            .map_err(AppError::Internal)?;
        executors
            .sqlite
            .select_llm_result_staging_cleanup_durable(
                u16::try_from(scheduler.durable_capacity().min(256)).map_err(|_| {
                    AppError::ResourceLimit("LLM result cleanup limit overflowed".to_string())
                })?,
            )
            .await?
    };
    let mut processed = 0;
    for job_id in candidates {
        let _worker_permit = scheduler
            .acquire_durable(
                DurableSourceId::LlmResult,
                SchedulerAdmissionKind::ExistingClaimCompletion,
            )
            .await
            .map_err(AppError::Internal)?;
        let outcome = executors
            .sqlite
            .cleanup_llm_result_staging_page_durable(job_id.clone(), 256)
            .await?;
        if outcome.complete {
            executors
                .sqlite
                .finalize_llm_result_cleanup_durable(job_id)
                .await?;
            scheduler.wake_journal_recovery();
        }
        processed += 1;
    }
    Ok(processed)
}

fn result_error_is_retryable(error: &AppError) -> bool {
    matches!(
        error,
        AppError::DatabaseBusy
            | AppError::Database(_)
            | AppError::Pool(_)
            | AppError::Io(_)
            | AppError::Internal(_)
    )
}

pub(crate) fn select_result_candidates(
    connection: &mut Connection,
    candidate_limit: i64,
) -> AppResult<Vec<QueuedResult>> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let unclaimed = {
        let mut statement =
            transaction.prepare(queries::llm_callback::SELECT_JOURNAL_RESULT_CANDIDATES)?;
        let rows = statement
            .query_map([candidate_limit], |row| {
                let status = match row.get::<_, String>(4)?.as_str() {
                    "completed" => ResultStatus::Completed,
                    "failed" => ResultStatus::Failed,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                Ok((
                    ResultManifest {
                        job_id: row.get(0)?,
                        media_id: row.get(1)?,
                        task: row.get(2)?,
                        attempt: row.get(3)?,
                        status,
                        model_type: row.get(5)?,
                        model_version: row.get(6)?,
                        encoding: row.get(7)?,
                        record_count: row.get(8)?,
                        byte_size: row.get(9)?,
                        content_hash: row.get(10)?,
                    },
                    row.get::<_, String>(11)?,
                    row.get::<_, u32>(12)?,
                    row.get::<_, u64>(13)?,
                    row.get::<_, i64>(14)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut candidates = Vec::with_capacity(unclaimed.len());
    for (manifest, inbox_path, next_record_sequence, next_byte_offset, result_product_version) in
        unclaimed
    {
        let claim_token = uuid::Uuid::new_v4().to_string();
        if transaction.execute(
            queries::llm_callback::CLAIM_RESULT_RECEIPT,
            rusqlite::params![claim_token, manifest.job_id],
        )? == 1
        {
            candidates.push(QueuedResult::Journal {
                manifest,
                inbox_path,
                next_record_sequence,
                next_byte_offset,
                result_product_version,
                claim_token,
            });
        }
    }
    transaction.commit()?;
    Ok(candidates)
}

async fn prepare_queued_result(
    executors: &crate::runtime::ExecutorHandles,
    queued: QueuedResult,
    process_config: &MediaProcessConfig,
) -> AppResult<PreparedQueuedResult> {
    let (request, claim_token) = match queued {
        QueuedResult::Journal {
            manifest,
            inbox_path,
            next_record_sequence,
            next_byte_offset,
            result_product_version,
            claim_token,
        } => {
            let request = read_journal_result(
                executors,
                manifest,
                &inbox_path,
                next_record_sequence,
                next_byte_offset,
                &claim_token,
                result_product_version,
            )
            .await?;
            (
                PreparedResultRequest::Streamed(Box::new(request)),
                Some(claim_token),
            )
        }
    };
    let job_id = request.job_id().to_string();
    let face = match prepare_queued_face_result(
        executors,
        &request,
        claim_token.as_deref(),
        process_config,
    )
    .await
    {
        Ok(face) => face,
        Err(error) if result_error_is_retryable(&error) => return Err(error),
        Err(error) => {
            return Ok(PreparedQueuedResult::PermanentFailure {
                job_id,
                claim_token,
                error: error.to_string(),
            });
        }
    };
    Ok(PreparedQueuedResult::Result {
        job_id,
        claim_token,
        request,
        face,
    })
}

async fn read_journal_result(
    executors: &crate::runtime::ExecutorHandles,
    manifest: ResultManifest,
    inbox_path: &str,
    mut next_record_sequence: u32,
    mut next_byte_offset: u64,
    claim_token: &str,
    product_version: i64,
) -> AppResult<StreamedResult> {
    manifest.validate().map_err(AppError::BadRequest)?;
    let prepared_inputs = executors
        .sqlite
        .load_llm_prepared_inputs_durable(manifest.job_id.clone())
        .await?;
    let inputs = prepared_inputs
        .into_iter()
        .map(|input| {
            Ok(ResultInputCorrelation {
                sequence: u32::try_from(input.sequence).map_err(|_| {
                    AppError::BadRequest(
                        "LLM result input sequence is outside its bound".to_string(),
                    )
                })?,
                frame_timestamp_ms: input.frame_timestamp_ms,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let path = NormalizedStoragePath::parse(inbox_path)
        .map_err(|error| AppError::BadRequest(format!("invalid LLM result inbox path: {error}")))?;
    let (mut session, snapshot) = executors
        .file_io
        .open_storage_read_session_durable(StorageRootId::Journal, path)
        .await?;
    if snapshot.byte_size != manifest.byte_size {
        executors
            .file_io
            .close_storage_session_durable(session)
            .await?;
        return Err(AppError::BadRequest(
            "LLM result inbox size changed after receipt".to_string(),
        ));
    }
    let mut hash_session = executors.cpu.start_sha256_session_durable().await?;
    let mut decoder = ResultRecordChunkDecoder::new();
    let mut validator = ResultRecordStreamValidator::new(
        &manifest.task,
        manifest.status,
        &inputs,
        manifest.record_count,
        manifest.byte_size,
    )
    .map_err(AppError::BadRequest)?;
    let mut bytes_read = 0_u64;
    let mut record_offset = 0_u64;
    let mut staging_page = Vec::with_capacity(256);
    loop {
        let (next_session, bytes) = executors
            .file_io
            .read_storage_session_durable(session, crate::runtime::FILE_IO_CHUNK_BYTES as usize)
            .await?;
        session = next_session;
        if bytes.is_empty() {
            break;
        }
        bytes_read = bytes_read
            .checked_add(u64::try_from(bytes.len()).map_err(|_| {
                AppError::ResourceLimit("LLM result read size overflowed".to_string())
            })?)
            .ok_or_else(|| {
                AppError::ResourceLimit("LLM result read size overflowed".to_string())
            })?;
        let (next_hash_session, bytes) = executors
            .cpu
            .update_sha256_session_durable(hash_session, bytes)
            .await?;
        hash_session = next_hash_session;
        for decode_chunk in bytes.chunks(128 * RESULT_RECORD_HEADER_BYTES) {
            let decoded = decoder.push(decode_chunk, |record| {
                collect_staging_record(
                    &mut validator,
                    record,
                    &mut record_offset,
                    next_record_sequence,
                    next_byte_offset,
                    &mut staging_page,
                )
            });
            if let Err(error) = decoded {
                executors
                    .file_io
                    .close_storage_session_durable(session)
                    .await?;
                return Err(AppError::BadRequest(error));
            }
            let staging_payload_bytes = staging_page
                .iter()
                .map(|record| record.normalized_payload.len())
                .sum::<usize>();
            if staging_page.len() >= 128 || staging_payload_bytes >= 3 * 1024 * 1024 {
                stage_result_page(
                    executors,
                    &manifest,
                    &mut next_record_sequence,
                    &mut next_byte_offset,
                    &mut staging_page,
                    claim_token,
                )
                .await?;
            }
        }
    }
    executors
        .file_io
        .close_storage_session_durable(session)
        .await?;
    decoder.finish().map_err(AppError::BadRequest)?;
    if !staging_page.is_empty() {
        stage_result_page(
            executors,
            &manifest,
            &mut next_record_sequence,
            &mut next_byte_offset,
            &mut staging_page,
            claim_token,
        )
        .await?;
    }
    if bytes_read != manifest.byte_size {
        return Err(AppError::BadRequest(
            "LLM result inbox byte count changed after receipt".to_string(),
        ));
    }
    let content_hash = executors
        .cpu
        .finish_sha256_session_durable(hash_session)
        .await?;
    if !content_hash.eq_ignore_ascii_case(&manifest.content_hash) {
        return Err(AppError::BadRequest(
            "LLM result inbox hash changed after receipt".to_string(),
        ));
    }
    if next_record_sequence != manifest.record_count || next_byte_offset != manifest.byte_size {
        return Err(AppError::BadRequest(
            "LLM result staging cursor does not match its manifest".to_string(),
        ));
    }
    validator.finish().map_err(AppError::BadRequest)?;
    let result = collect_staged_result(executors, &manifest, &inputs, claim_token).await?;
    Ok(StreamedResult {
        manifest,
        result,
        product_version,
    })
}

fn collect_staging_record(
    validator: &mut ResultRecordStreamValidator,
    record: OwnedResultRecord,
    record_offset: &mut u64,
    next_record_sequence: u32,
    next_byte_offset: u64,
    staging_page: &mut Vec<StagedLlmResultRecord>,
) -> Result<(), String> {
    validator.push(record.as_borrowed())?;
    let encoded_size = RESULT_RECORD_HEADER_BYTES
        .checked_add(record.payload.len())
        .and_then(|size| u32::try_from(size).ok())
        .ok_or_else(|| "LLM result record size overflowed".to_string())?;
    if record.record_sequence < next_record_sequence {
        let end_offset = record_offset
            .checked_add(u64::from(encoded_size))
            .ok_or_else(|| "LLM result record offset overflowed".to_string())?;
        if end_offset > next_byte_offset {
            return Err("LLM result staging cursor splits an encoded record".to_string());
        }
        *record_offset = end_offset;
        return Ok(());
    }
    if record.record_sequence
        != next_record_sequence
            .checked_add(
                u32::try_from(staging_page.len())
                    .map_err(|_| "LLM result staging page length overflowed".to_string())?,
            )
            .ok_or_else(|| "LLM result staging sequence overflowed".to_string())?
        || *record_offset
            != next_byte_offset
                .checked_add(
                    staging_page
                        .iter()
                        .map(|staged| u64::from(staged.encoded_size))
                        .sum::<u64>(),
                )
                .ok_or_else(|| "LLM result staging offset overflowed".to_string())?
    {
        return Err("LLM result staging cursor does not align with the record stream".to_string());
    }
    if staging_page.len() >= 256 {
        return Err("LLM result staging page exceeded 256 records".to_string());
    }
    staging_page.push(StagedLlmResultRecord {
        record_sequence: record.record_sequence,
        input_sequence: (record.input_sequence != u32::MAX).then_some(record.input_sequence),
        kind: result_record_kind_name(record.kind).to_string(),
        byte_offset: *record_offset,
        encoded_size,
        normalized_payload: record.payload,
    });
    *record_offset = record_offset
        .checked_add(u64::from(encoded_size))
        .ok_or_else(|| "LLM result record offset overflowed".to_string())?;
    Ok(())
}

pub(crate) fn load_staging_page(
    connection: &Connection,
    job_id: &str,
    attempt: u32,
    after_record_sequence: Option<u32>,
    claim_token: &str,
    limit: i64,
) -> AppResult<Vec<StagedLlmResultRecord>> {
    let after = after_record_sequence.map_or(-1_i64, i64::from);
    connection
        .prepare(queries::llm_callback::SELECT_RESULT_STAGING_PAGE)?
        .query_map(
            rusqlite::params![job_id, attempt, after, claim_token, limit],
            |row| {
                Ok(StagedLlmResultRecord {
                    record_sequence: row.get(0)?,
                    input_sequence: row.get(1)?,
                    kind: row.get(2)?,
                    byte_offset: row.get(3)?,
                    encoded_size: row.get(4)?,
                    normalized_payload: row.get(5)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
}

async fn collect_staged_result(
    executors: &crate::runtime::ExecutorHandles,
    manifest: &ResultManifest,
    inputs: &[ResultInputCorrelation],
    claim_token: &str,
) -> AppResult<ValidatedResultStream> {
    let mut collector = ResultRecordCollector::new(
        &manifest.task,
        manifest.status,
        inputs,
        manifest.record_count,
        manifest.byte_size,
    )
    .map_err(AppError::BadRequest)?;
    let mut after_record_sequence = None;
    let mut expected_record_sequence = 0_u32;
    let mut expected_byte_offset = 0_u64;
    loop {
        let page = executors
            .sqlite
            .load_llm_result_staging_page_durable(
                manifest.job_id.clone(),
                manifest.attempt,
                after_record_sequence,
                claim_token.to_string(),
                4,
            )
            .await?;
        if page.is_empty() {
            break;
        }
        let page_len = page.len();
        for record in page {
            if record.record_sequence != expected_record_sequence
                || record.byte_offset != expected_byte_offset
                || record.encoded_size as usize
                    != RESULT_RECORD_HEADER_BYTES + record.normalized_payload.len()
            {
                return Err(AppError::BadRequest(
                    "staged LLM result record ordering is invalid".to_string(),
                ));
            }
            let kind = parse_result_record_kind(&record.kind)?;
            collector
                .push(ResultRecord {
                    kind,
                    flags: 0,
                    record_sequence: record.record_sequence,
                    input_sequence: record.input_sequence.unwrap_or(u32::MAX),
                    payload: &record.normalized_payload,
                })
                .map_err(AppError::BadRequest)?;
            expected_record_sequence =
                expected_record_sequence.checked_add(1).ok_or_else(|| {
                    AppError::ResourceLimit("staged LLM record cursor overflowed".to_string())
                })?;
            expected_byte_offset = expected_byte_offset
                .checked_add(u64::from(record.encoded_size))
                .ok_or_else(|| {
                    AppError::ResourceLimit("staged LLM byte cursor overflowed".to_string())
                })?;
            after_record_sequence = Some(record.record_sequence);
        }
        if page_len < 4 {
            break;
        }
    }
    if expected_record_sequence != manifest.record_count
        || expected_byte_offset != manifest.byte_size
    {
        return Err(AppError::BadRequest(
            "staged LLM result does not match its manifest".to_string(),
        ));
    }
    collector.finish().map_err(AppError::BadRequest)
}

async fn stage_result_page(
    executors: &crate::runtime::ExecutorHandles,
    manifest: &ResultManifest,
    next_record_sequence: &mut u32,
    next_byte_offset: &mut u64,
    staging_page: &mut Vec<StagedLlmResultRecord>,
    claim_token: &str,
) -> AppResult<()> {
    let page_record_count = u32::try_from(staging_page.len())
        .map_err(|_| AppError::ResourceLimit("LLM result staging page overflowed".to_string()))?;
    let page_byte_size = staging_page.iter().try_fold(0_u64, |total, record| {
        total.checked_add(u64::from(record.encoded_size))
    });
    let records = std::mem::replace(staging_page, Vec::with_capacity(256));
    let outcome = executors
        .sqlite
        .stage_llm_result_page_durable(StageLlmResultPage {
            job_id: manifest.job_id.clone(),
            attempt: manifest.attempt,
            claim_token: claim_token.to_string(),
            expected_record_sequence: *next_record_sequence,
            expected_byte_offset: *next_byte_offset,
            records,
        })
        .await?;
    if outcome != StageLlmResultPageOutcome::Staged {
        return Err(AppError::Internal(
            "LLM result receipt changed during staging".to_string(),
        ));
    }
    *next_record_sequence = next_record_sequence
        .checked_add(page_record_count)
        .ok_or_else(|| {
            AppError::ResourceLimit("LLM result staging cursor overflowed".to_string())
        })?;
    *next_byte_offset = next_byte_offset
        .checked_add(page_byte_size.ok_or_else(|| {
            AppError::ResourceLimit("LLM result staging byte cursor overflowed".to_string())
        })?)
        .ok_or_else(|| {
            AppError::ResourceLimit("LLM result staging byte cursor overflowed".to_string())
        })?;
    Ok(())
}

fn result_record_kind_name(kind: ResultRecordKind) -> &'static str {
    match kind {
        ResultRecordKind::Failure => "failure",
        ResultRecordKind::InputStarted => "input_started",
        ResultRecordKind::OcrText => "ocr_text",
        ResultRecordKind::ImageTags => "image_tags",
        ResultRecordKind::ImageClustering => "image_clustering",
        ResultRecordKind::ImageAesthetics => "image_aesthetics",
        ResultRecordKind::Face => "face",
        ResultRecordKind::ScreenshotDetection => "screenshot_detection",
        ResultRecordKind::DocumentDetection => "document_detection",
        ResultRecordKind::InputFinished => "input_finished",
        ResultRecordKind::OcrTextContinuation => "ocr_text_continuation",
        ResultRecordKind::ImageTagsContinuation => "image_tags_continuation",
    }
}

fn parse_result_record_kind(kind: &str) -> AppResult<ResultRecordKind> {
    match kind {
        "failure" => Ok(ResultRecordKind::Failure),
        "input_started" => Ok(ResultRecordKind::InputStarted),
        "ocr_text" => Ok(ResultRecordKind::OcrText),
        "image_tags" => Ok(ResultRecordKind::ImageTags),
        "image_clustering" => Ok(ResultRecordKind::ImageClustering),
        "image_aesthetics" => Ok(ResultRecordKind::ImageAesthetics),
        "face" => Ok(ResultRecordKind::Face),
        "screenshot_detection" => Ok(ResultRecordKind::ScreenshotDetection),
        "document_detection" => Ok(ResultRecordKind::DocumentDetection),
        "input_finished" => Ok(ResultRecordKind::InputFinished),
        "ocr_text_continuation" => Ok(ResultRecordKind::OcrTextContinuation),
        "image_tags_continuation" => Ok(ResultRecordKind::ImageTagsContinuation),
        _ => Err(AppError::BadRequest(
            "staged LLM result record kind is invalid".to_string(),
        )),
    }
}

async fn prepare_queued_face_result(
    executors: &crate::runtime::ExecutorHandles,
    request: &PreparedResultRequest,
    claim_token: Option<&str>,
    process_config: &MediaProcessConfig,
) -> AppResult<Option<crate::processor::face_detection::PreparedFaceDetectionResult>> {
    let PreparedResultRequest::Streamed(streamed) = request;
    if streamed.manifest.status != ResultStatus::Completed
        || streamed.manifest.task != "face_detection"
    {
        return Ok(None);
    }
    let model_type = streamed
        .manifest
        .model_type
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("modelType is required".to_string()))?;
    let model_version = streamed
        .manifest
        .model_version
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("modelVersion is required".to_string()))?;
    let claim_token = claim_token.ok_or_else(|| {
        AppError::Internal("streamed face result is missing its claim token".to_string())
    })?;
    let context = executors
        .sqlite
        .load_face_preparation_context_durable(
            streamed.manifest.job_id.clone(),
            streamed.manifest.media_id,
        )
        .await?;
    crate::processor::face_detection::prepare_typed_result(
        executors,
        crate::processor::face_detection::TypedFaceResultPreparationRequest {
            context,
            job_id: &streamed.manifest.job_id,
            media_id: streamed.manifest.media_id,
            model_type,
            model_version,
            input_results: &streamed.result.inputs,
            claim_token,
            product_version: streamed.product_version,
            process_config,
        },
    )
    .await
    .map(Some)
}

pub(crate) fn persist_prepared_result(
    connection: &Connection,
    prepared: PreparedQueuedResult,
    capacity: Option<&crate::database::operations::SqliteResultCapacityChild>,
) -> AppResult<Vec<crate::io::file::NormalizedStoragePath>> {
    let capacity_job_id = prepared.durable_parent_job_id().map(str::to_string);
    let mut transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let mut replaced_crop_paths = None;
    let mut permanent_failure = None;
    match prepared {
        PreparedQueuedResult::PermanentFailure {
            job_id,
            claim_token,
            error,
        } => {
            verify_result_claim(&transaction, &job_id, claim_token.as_deref())?;
            fail_received_result(&transaction, &job_id, &error)?;
            queue_result_receipt_cleanup(&transaction, &job_id)?;
            permanent_failure = Some((job_id, error));
        }
        PreparedQueuedResult::Result {
            job_id,
            claim_token,
            request,
            face,
        } => {
            verify_result_claim(&transaction, &job_id, claim_token.as_deref())?;
            let result = {
                let mut savepoint = transaction.savepoint()?;
                let PreparedResultRequest::Streamed(streamed) = request;
                let persistence = persist_streamed_result(&savepoint, *streamed, face);
                match persistence {
                    Ok(changes) => {
                        queue_result_receipt_cleanup(&savepoint, &job_id)?;
                        savepoint.commit()?;
                        Ok(changes)
                    }
                    Err(error) => {
                        savepoint.rollback()?;
                        Err(error)
                    }
                }
            };
            match result {
                Ok(paths) => replaced_crop_paths = paths,
                Err(error) if result_error_is_retryable(&error) => return Err(error),
                Err(error) => {
                    fail_received_result(&transaction, &job_id, &error.to_string())?;
                    queue_result_receipt_cleanup(&transaction, &job_id)?;
                    permanent_failure = Some((job_id, error.to_string()));
                }
            }
        }
    }
    if let Some(capacity) = capacity {
        let job_id = capacity_job_id.as_deref().ok_or_else(|| {
            AppError::Internal(
                "unclaimed LLM result persistence unexpectedly received durable capacity"
                    .to_string(),
            )
        })?;
        crate::database::operations::shrink_llm_result_sqlite_reservation_to_cleanup(
            &transaction,
            job_id,
            capacity,
        )?;
    }
    transaction.commit()?;
    if let Some((job_id, error)) = permanent_failure {
        tracing::error!(
            job_id,
            error,
            "Momento LLM result processing failed permanently"
        );
    }
    Ok(replaced_crop_paths.unwrap_or_default())
}

fn verify_result_claim(
    connection: &Connection,
    job_id: &str,
    claim_token: Option<&str>,
) -> AppResult<()> {
    let Some(claim_token) = claim_token else {
        return Ok(());
    };
    if connection
        .query_row(
            queries::llm_callback::VERIFY_RESULT_RECEIPT_CLAIM,
            rusqlite::params![job_id, claim_token],
            |_| Ok(()),
        )
        .optional()?
        .is_none()
    {
        return Err(AppError::Conflict(
            "LLM result claim changed before persistence".to_string(),
        ));
    }
    Ok(())
}

fn queue_result_receipt_cleanup(connection: &Connection, job_id: &str) -> AppResult<()> {
    connection.execute(
        queries::llm_callback::QUEUE_RESULT_RECEIPT_CLEANUP,
        [job_id],
    )?;
    connection.execute(
        queries::llm_callback::MARK_RESULT_RECEIPT_CLEANUP_PENDING,
        [job_id],
    )?;
    Ok(())
}

fn fail_received_result(connection: &Connection, job_id: &str, error: &str) -> AppResult<()> {
    connection.execute(
        queries::llm_callback::MARK_RECEIVED_RESULT_FAILED,
        rusqlite::params![error, job_id],
    )?;
    Ok(())
}

fn persist_streamed_result(
    connection: &Connection,
    streamed: StreamedResult,
    prepared_face: Option<crate::processor::face_detection::PreparedFaceDetectionResult>,
) -> AppResult<Option<Vec<crate::io::file::NormalizedStoragePath>>> {
    let StreamedResult {
        manifest,
        result,
        product_version: _,
    } = streamed;
    let job: (i64, String, i64, String) = connection
        .query_row(
            queries::llm_callback::SELECT_JOB,
            [&manifest.job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| AppError::NotFound("LLM job not found".to_string()))?;
    if job.0 != manifest.media_id || job.1 != manifest.task || job.2 != i64::from(manifest.attempt)
    {
        return Err(AppError::Conflict(
            "LLM result does not match submitted job".to_string(),
        ));
    }
    if matches!(job.3.as_str(), "completed" | "failed" | "cancelled") {
        return Ok(None);
    }
    if job.3 != "submitted" {
        return Err(AppError::Conflict(
            "LLM job is not awaiting a result".to_string(),
        ));
    }

    if result.status == ResultStatus::Failed {
        if connection.execute(
            queries::llm_callback::MARK_FAILED,
            rusqlite::params![
                result
                    .failure
                    .unwrap_or_else(|| "LLM inference failed".to_string()),
                manifest.job_id,
                manifest.attempt
            ],
        )? != 1
        {
            return Err(AppError::Conflict(
                "LLM job changed during result persistence".to_string(),
            ));
        }
        return Ok(None);
    }

    let model_type = manifest
        .model_type
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("modelType is required".to_string()))?;
    let model_version = manifest
        .model_version
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("modelVersion is required".to_string()))?;
    let replaced_crop_paths = match manifest.task.as_str() {
        "ocr" | "image_tagging" => {
            persist_typed_text_results(
                connection,
                manifest.media_id,
                model_type,
                model_version,
                &result.inputs,
            )?;
            None
        }
        "image_clustering" => {
            persist_typed_clustering_result(
                connection,
                manifest.media_id,
                model_version,
                &result.inputs,
            )?;
            None
        }
        "image_aesthetics" => {
            if model_type != "image_aesthetics" {
                return Err(AppError::BadRequest(
                    "aesthetics result modelType must be image_aesthetics".to_string(),
                ));
            }
            persist_typed_aesthetics_results(
                connection,
                manifest.media_id,
                model_version,
                &result.inputs,
            )?;
            None
        }
        SCREENSHOT_DETECTION_MODEL_TYPE | DOCUMENT_DETECTION_MODEL_TYPE => {
            if model_type != manifest.task {
                return Err(AppError::BadRequest(format!(
                    "{} result modelType must match the task",
                    manifest.task
                )));
            }
            persist_typed_classification_results(
                connection,
                manifest.media_id,
                &manifest.task,
                model_version,
                &result.inputs,
            )?;
            None
        }
        "face_detection" => {
            let prepared = prepared_face.ok_or_else(|| {
                AppError::Internal("face detection result was not prepared".to_string())
            })?;
            Some(crate::processor::face_detection::persist_prepared_result(
                connection, prepared,
            )?)
        }
        _ => {
            return Err(AppError::BadRequest(
                "completed result task is not supported".to_string(),
            ));
        }
    };
    if connection.execute(
        queries::llm_callback::MARK_COMPLETED,
        rusqlite::params![manifest.job_id, manifest.attempt],
    )? != 1
    {
        return Err(AppError::Conflict(
            "LLM job changed during result persistence".to_string(),
        ));
    }
    Ok(replaced_crop_paths)
}

fn persist_typed_text_results(
    connection: &Connection,
    media_id: i64,
    model_type: &str,
    model_version: &str,
    inputs: &[ValidatedResultInput],
) -> AppResult<()> {
    let mut aggregate = Vec::with_capacity(inputs.len());
    for input in inputs {
        let text = match &input.value {
            ValidatedResultValue::Ocr(text) => text.clone(),
            ValidatedResultValue::ImageTags(tags) => tags.join("\n"),
            _ => {
                return Err(AppError::BadRequest(
                    "text result contains a different task payload".to_string(),
                ));
            }
        };
        connection.execute(
            queries::llm_callback::UPSERT_INPUT_TEXT,
            rusqlite::params![
                media_id,
                model_type,
                input.sequence,
                input.frame_timestamp_ms,
                model_version,
                text
            ],
        )?;
        aggregate.push(text);
    }
    connection.execute(
        queries::llm_callback::UPSERT_TEXT,
        rusqlite::params![media_id, model_type, model_version, aggregate.join("\n")],
    )?;
    Ok(())
}

fn persist_typed_clustering_result(
    connection: &Connection,
    media_id: i64,
    model_version: &str,
    inputs: &[ValidatedResultInput],
) -> AppResult<()> {
    let [input] = inputs else {
        return Err(AppError::BadRequest(
            "image_clustering requires exactly one input result".to_string(),
        ));
    };
    let ValidatedResultValue::ImageClustering(payload) = &input.value else {
        return Err(AppError::BadRequest(
            "image_clustering result contains a different task payload".to_string(),
        ));
    };
    if payload.embedding.len() != IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS
        || payload.embedding.iter().any(|value| !value.is_finite())
    {
        return Err(AppError::BadRequest(
            "clustering embedding has invalid dimensions or values".to_string(),
        ));
    }
    let mut embedding = Vec::with_capacity(payload.embedding.len() * 4);
    for value in &payload.embedding {
        embedding.extend_from_slice(&value.to_le_bytes());
    }
    persist_clustering_components(
        connection,
        media_id,
        model_version,
        embedding,
        payload.perceptual_hash,
    )
}

#[derive(Clone, Copy)]
struct AestheticScores {
    aesthetic: f64,
    scenic: f64,
    simplicity: f64,
    landscape: f64,
    technical_quality: f64,
}

fn persist_typed_aesthetics_results(
    connection: &Connection,
    media_id: i64,
    model_version: &str,
    inputs: &[ValidatedResultInput],
) -> AppResult<()> {
    let mut aggregate = None;
    for input in inputs {
        let ValidatedResultValue::ImageAesthetics(payload) = input.value else {
            return Err(AppError::BadRequest(
                "image_aesthetics result contains a different task payload".to_string(),
            ));
        };
        let scores = AestheticScores {
            aesthetic: f64::from(payload.aesthetic),
            scenic: f64::from(payload.scenic),
            simplicity: f64::from(payload.simplicity),
            landscape: f64::from(payload.landscape),
            technical_quality: f64::from(payload.technical_quality),
        };
        if [
            scores.aesthetic,
            scores.scenic,
            scores.simplicity,
            scores.landscape,
            scores.technical_quality,
        ]
        .iter()
        .any(|score| !score.is_finite() || !(0.0..=1.0).contains(score))
        {
            return Err(AppError::BadRequest(
                "aesthetics scores must be within [0, 1]".to_string(),
            ));
        }
        aggregate.get_or_insert(scores);
        connection.execute(
            queries::llm_callback::UPSERT_AESTHETIC_INPUT,
            rusqlite::params![
                media_id,
                input.sequence,
                input.frame_timestamp_ms,
                model_version,
                scores.aesthetic,
                scores.scenic,
                scores.simplicity,
                scores.landscape,
                scores.technical_quality
            ],
        )?;
    }
    let aggregate = aggregate
        .ok_or_else(|| AppError::BadRequest("aesthetics input results are required".to_string()))?;
    connection.execute(
        queries::llm_callback::UPSERT_AESTHETICS,
        rusqlite::params![
            media_id,
            model_version,
            aggregate.aesthetic,
            aggregate.scenic,
            aggregate.simplicity,
            aggregate.landscape,
            aggregate.technical_quality
        ],
    )?;
    Ok(())
}

fn persist_typed_classification_results(
    connection: &Connection,
    media_id: i64,
    task: &str,
    model_version: &str,
    inputs: &[ValidatedResultInput],
) -> AppResult<()> {
    let input_query = match task {
        SCREENSHOT_DETECTION_MODEL_TYPE => {
            queries::llm_callback::UPSERT_SCREENSHOT_CLASSIFICATION_INPUT
        }
        DOCUMENT_DETECTION_MODEL_TYPE => {
            queries::llm_callback::UPSERT_DOCUMENT_CLASSIFICATION_INPUT
        }
        _ => {
            return Err(AppError::BadRequest(
                "classification task is not supported".to_string(),
            ));
        }
    };
    let mut aggregate = None;
    for input in inputs {
        let payload = match (&input.value, task) {
            (
                ValidatedResultValue::ScreenshotDetection(payload),
                SCREENSHOT_DETECTION_MODEL_TYPE,
            )
            | (ValidatedResultValue::DocumentDetection(payload), DOCUMENT_DETECTION_MODEL_TYPE) => {
                *payload
            }
            _ => {
                return Err(AppError::BadRequest(
                    "classification result contains a different task payload".to_string(),
                ));
            }
        };
        if !payload.confidence.is_finite() || !(0.0..=1.0).contains(&payload.confidence) {
            return Err(AppError::BadRequest(
                "classification confidence must be within [0, 1]".to_string(),
            ));
        }
        aggregate.get_or_insert(payload);
        connection.execute(
            input_query,
            rusqlite::params![
                media_id,
                input.sequence,
                input.frame_timestamp_ms,
                model_version,
                payload.detected,
                payload.confidence
            ],
        )?;
    }
    let aggregate = aggregate
        .ok_or_else(|| AppError::BadRequest(format!("{task} input results are required")))?;
    let aggregate_query = match task {
        SCREENSHOT_DETECTION_MODEL_TYPE => queries::llm_callback::UPSERT_SCREENSHOT_CLASSIFICATION,
        DOCUMENT_DETECTION_MODEL_TYPE => queries::llm_callback::UPSERT_DOCUMENT_CLASSIFICATION,
        _ => unreachable!("task was validated before input persistence"),
    };
    connection.execute(
        aggregate_query,
        rusqlite::params![
            media_id,
            model_version,
            aggregate.detected,
            aggregate.confidence
        ],
    )?;
    Ok(())
}

fn persist_clustering_components(
    connection: &Connection,
    media_id: i64,
    model_version: &str,
    embedding_bytes: Vec<u8>,
    perceptual_hash: u64,
) -> AppResult<()> {
    let (content_hash, capture_time_seconds): (String, Option<i64>) = connection.query_row(
        queries::llm_callback::SELECT_CLUSTER_MEDIA,
        [media_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    connection.execute(
        queries::llm_callback::UPSERT_SIMILARITY_INDEX,
        rusqlite::params![
            media_id,
            content_hash,
            model_version,
            embedding_bytes,
            perceptual_hash as i64,
            capture_time_seconds
        ],
    )?;
    connection.execute(queries::llm_callback::DELETE_HASH_BANDS, [media_id])?;
    for band_index in 0..4_i64 {
        connection.execute(
            queries::llm_callback::INSERT_HASH_BAND,
            rusqlite::params![
                media_id,
                band_index,
                ((perceptual_hash >> (band_index * 16)) & 0xffff) as i64
            ],
        )?;
    }
    connection.execute(queries::llm_callback::UPSERT_DIRTY, [media_id])?;
    Ok(())
}
