use momento_common::llm::{JobInputResult, JobResult, IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::config::MediaProcessConfig;
use crate::constants::{DOCUMENT_DETECTION_MODEL_TYPE, SCREENSHOT_DETECTION_MODEL_TYPE};
use crate::database::{queries, DbPool};
use crate::error::{AppError, AppResult};

struct QueuedResult {
    job_id: String,
    payload: String,
}

struct PreparedResultCompletion {
    job_id: String,
    prepared: AppResult<PreparedQueuedResult>,
}

enum PreparedQueuedResult {
    Result {
        job_id: String,
        request: Box<JobResult>,
        face: Option<crate::processor::face_detection::PreparedFaceDetectionResult>,
    },
    PermanentFailure {
        job_id: String,
        error: String,
    },
}

pub fn receive_result(pool: &DbPool, request: JobResult) -> AppResult<()> {
    let payload = serde_json::to_string(&request)?;
    let connection = pool.get()?;
    let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
    let Some((media_id, task, attempts, status)) = transaction
        .query_row(
            queries::llm_callback::SELECT_JOB,
            [&request.job_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
    else {
        tracing::warn!(
            job_id = request.job_id,
            "discarding result for an unknown Momento job"
        );
        transaction.commit()?;
        return Ok(());
    };
    if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
        transaction.commit()?;
        return Ok(());
    }
    let expected_attempt = if status == "submitted" {
        attempts
    } else {
        attempts + 1
    };
    if media_id != request.media_id
        || task != request.task
        || expected_attempt != i64::from(request.attempt)
    {
        let error = "received LLM result does not match the Momento job";
        transaction.execute(
            queries::llm_callback::MARK_RESULT_CORRELATION_FAILED,
            rusqlite::params![error, request.job_id],
        )?;
        transaction.commit()?;
        tracing::error!(
            job_id = request.job_id,
            media_id = request.media_id,
            task = request.task,
            attempt = request.attempt,
            "{error}"
        );
        return Ok(());
    }
    if matches!(status.as_str(), "queued" | "submitting")
        && transaction.execute(
            queries::llm_callback::MARK_UNACKNOWLEDGED_RESULT_SUBMITTED,
            rusqlite::params![request.attempt, request.job_id, request.attempt],
        )? != 1
    {
        return Err(AppError::Conflict(
            "Momento job changed while receiving the LLM result".to_string(),
        ));
    }
    transaction.execute(
        queries::llm_callback::INSERT_RECEIVED_RESULT,
        rusqlite::params![request.job_id, payload],
    )?;
    transaction.commit()?;
    Ok(())
}

struct RollingCpuWorkers<Input, Output> {
    input_sender: Option<SyncSender<Input>>,
    output_receiver: Receiver<Output>,
    workers: Vec<JoinHandle<()>>,
}

impl<Input: Send + 'static, Output: Send + 'static> RollingCpuWorkers<Input, Output> {
    fn new<ProcessInput>(
        concurrency: usize,
        worker_name: &str,
        process_input: ProcessInput,
    ) -> AppResult<Self>
    where
        ProcessInput: Fn(Input) -> Output + Send + Sync + 'static,
    {
        if concurrency == 0 {
            return Err(AppError::Validation(
                "Result processing concurrency must be positive".to_string(),
            ));
        }
        if worker_name.trim().is_empty() {
            return Err(AppError::Validation(
                "CPU worker name must not be empty".to_string(),
            ));
        }

        let (input_sender, input_receiver) = mpsc::sync_channel(concurrency);
        let (output_sender, output_receiver) = mpsc::channel();
        let input_receiver = Arc::new(Mutex::new(input_receiver));
        let process_input = Arc::new(process_input);
        let mut workers: Vec<JoinHandle<()>> = Vec::with_capacity(concurrency);
        for worker_index in 0..concurrency {
            let worker_input_receiver = Arc::clone(&input_receiver);
            let worker_output_sender = output_sender.clone();
            let worker_process_input = Arc::clone(&process_input);
            let thread_name = format!("{worker_name}-{worker_index}");
            let worker = match std::thread::Builder::new()
                .name(thread_name)
                .spawn(move || loop {
                    let input = {
                        let receiver = match worker_input_receiver.lock() {
                            Ok(receiver) => receiver,
                            Err(_) => return,
                        };
                        match receiver.recv() {
                            Ok(input) => input,
                            Err(_) => return,
                        }
                    };
                    if worker_output_sender
                        .send(worker_process_input(input))
                        .is_err()
                    {
                        return;
                    }
                }) {
                Ok(worker) => worker,
                Err(error) => {
                    drop(input_sender);
                    for worker in workers {
                        let _ = worker.join();
                    }
                    return Err(AppError::Internal(format!(
                        "failed to start CPU processing worker: {error}"
                    )));
                }
            };
            workers.push(worker);
        }
        drop(output_sender);
        Ok(Self {
            input_sender: Some(input_sender),
            output_receiver,
            workers,
        })
    }

    fn submit(&self, input: Input) -> AppResult<()> {
        self.input_sender
            .as_ref()
            .ok_or_else(|| AppError::Internal("CPU processing workers are closed".to_string()))?
            .send(input)
            .map_err(|_| {
                AppError::Internal("CPU processing workers stopped unexpectedly".to_string())
            })
    }

    fn receive(&self) -> AppResult<Output> {
        self.output_receiver.recv().map_err(|_| {
            AppError::Internal("CPU processing workers stopped unexpectedly".to_string())
        })
    }

    fn receive_timeout(&self, timeout: Duration) -> AppResult<Option<Output>> {
        match self.output_receiver.recv_timeout(timeout) {
            Ok(output) => Ok(Some(output)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(AppError::Internal(
                "CPU processing workers stopped unexpectedly".to_string(),
            )),
        }
    }
}

impl<Input, Output> Drop for RollingCpuWorkers<Input, Output> {
    fn drop(&mut self) {
        self.input_sender.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

struct ResultPipeline {
    pool: DbPool,
    concurrency: usize,
    cpu_workers: RollingCpuWorkers<QueuedResult, PreparedResultCompletion>,
    in_flight_job_ids: HashSet<String>,
    retry_after: HashMap<String, Instant>,
}

impl ResultPipeline {
    fn new(
        pool: DbPool,
        concurrency: usize,
        process_config: MediaProcessConfig,
    ) -> AppResult<Self> {
        let preparation_pool = pool.clone();
        let cpu_workers =
            RollingCpuWorkers::new(concurrency, "ai-result-cpu", move |queued: QueuedResult| {
                let job_id = queued.job_id.clone();
                let prepared = (|| {
                    let connection = preparation_pool.get()?;
                    prepare_queued_result(&connection, queued, &process_config)
                })();
                PreparedResultCompletion { job_id, prepared }
            })?;
        Ok(Self {
            pool,
            concurrency,
            cpu_workers,
            in_flight_job_ids: HashSet::new(),
            retry_after: HashMap::new(),
        })
    }

    fn refill(&mut self) -> AppResult<usize> {
        let now = Instant::now();
        self.retry_after.retain(|_, retry_at| *retry_at > now);
        let available_slots = self
            .concurrency
            .saturating_sub(self.in_flight_job_ids.len());
        if available_slots == 0 {
            return Ok(0);
        }
        let candidate_limit = self
            .in_flight_job_ids
            .len()
            .checked_add(self.retry_after.len())
            .and_then(|excluded_count| excluded_count.checked_add(available_slots))
            .ok_or_else(|| {
                AppError::Validation("LLM result candidate window is too large".to_string())
            })?;
        let candidate_limit = i64::try_from(candidate_limit).map_err(|_| {
            AppError::Validation("LLM result candidate window is too large".to_string())
        })?;
        let candidates = {
            let connection = self.pool.get()?;
            select_result_candidates(&connection, candidate_limit)?
        };

        let mut submitted = 0;
        for queued in candidates {
            if self.in_flight_job_ids.contains(&queued.job_id)
                || self.retry_after.contains_key(&queued.job_id)
            {
                continue;
            }
            let job_id = queued.job_id.clone();
            self.cpu_workers.submit(queued)?;
            self.in_flight_job_ids.insert(job_id);
            submitted += 1;
            if submitted == available_slots {
                break;
            }
        }
        Ok(submitted)
    }

    fn receive(&self) -> AppResult<PreparedResultCompletion> {
        self.cpu_workers.receive()
    }

    fn receive_timeout(&self, timeout: Duration) -> AppResult<Option<PreparedResultCompletion>> {
        self.cpu_workers.receive_timeout(timeout)
    }

    fn complete(&mut self, completion: PreparedResultCompletion) -> (String, AppResult<()>) {
        let job_id = completion.job_id;
        if !self.in_flight_job_ids.remove(&job_id) {
            return (
                job_id,
                Err(AppError::Internal(
                    "CPU worker completed a result that was not in flight".to_string(),
                )),
            );
        }
        let persisted = completion.prepared.and_then(|prepared| {
            let connection = self.pool.get()?;
            persist_prepared_result(&connection, prepared)
        });
        (job_id, persisted)
    }

    fn defer_retry(&mut self, job_id: String, retry_delay: Duration) {
        self.retry_after
            .insert(job_id, Instant::now() + retry_delay);
    }

    fn in_flight_count(&self) -> usize {
        self.in_flight_job_ids.len()
    }
}

pub fn run(
    pool: DbPool,
    interval: Duration,
    concurrency: usize,
    process_config: MediaProcessConfig,
) -> ! {
    let mut pipeline = ResultPipeline::new(pool, concurrency, process_config)
        .expect("validated LLM result CPU pipeline must start");
    loop {
        if let Err(error) = pipeline.refill() {
            tracing::warn!(error = %error, "Momento LLM result pipeline refill failed");
        }
        if pipeline.in_flight_count() == 0 {
            std::thread::sleep(interval);
            continue;
        }
        let completion = match pipeline.receive_timeout(interval) {
            Ok(Some(completion)) => completion,
            Ok(None) => continue,
            Err(error) => {
                tracing::error!(error = %error, "Momento LLM result CPU pipeline stopped");
                std::thread::sleep(interval);
                continue;
            }
        };
        let (job_id, persisted) = pipeline.complete(completion);
        if let Err(error) = persisted {
            tracing::warn!(
                job_id,
                error = %error,
                "Momento LLM result remains queued and will be retried"
            );
            pipeline.defer_retry(job_id, interval);
        }
    }
}

pub fn process_available_results(
    pool: &DbPool,
    process_config: MediaProcessConfig,
    concurrency: usize,
) -> AppResult<usize> {
    let mut pipeline = ResultPipeline::new(pool.clone(), concurrency, process_config)?;
    pipeline.refill()?;
    let mut processed = 0;
    while pipeline.in_flight_count() > 0 {
        let completion = pipeline.receive()?;
        let (_, persisted) = pipeline.complete(completion);
        persisted?;
        processed += 1;
        pipeline.refill()?;
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

fn select_result_candidates(
    connection: &Connection,
    candidate_limit: i64,
) -> AppResult<Vec<QueuedResult>> {
    connection
        .prepare(queries::llm_callback::SELECT_RESULT_CANDIDATES)?
        .query_map([candidate_limit], |row| {
            Ok(QueuedResult {
                job_id: row.get(0)?,
                payload: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
}

fn prepare_queued_result(
    connection: &Connection,
    queued: QueuedResult,
    process_config: &MediaProcessConfig,
) -> AppResult<PreparedQueuedResult> {
    let request = match serde_json::from_str::<JobResult>(&queued.payload) {
        Ok(request) => request,
        Err(error) => {
            return Ok(PreparedQueuedResult::PermanentFailure {
                job_id: queued.job_id,
                error: error.to_string(),
            });
        }
    };
    let face = match prepare_face_result(connection, &request, process_config) {
        Ok(face) => face,
        Err(error) if result_error_is_retryable(&error) => return Err(error),
        Err(error) => {
            return Ok(PreparedQueuedResult::PermanentFailure {
                job_id: queued.job_id,
                error: error.to_string(),
            });
        }
    };
    Ok(PreparedQueuedResult::Result {
        job_id: queued.job_id,
        request: Box::new(request),
        face,
    })
}

pub fn process_result(
    pool: &DbPool,
    process_config: &MediaProcessConfig,
    request: JobResult,
) -> AppResult<()> {
    let connection = pool.get()?;
    let face = prepare_face_result(&connection, &request, process_config)?;
    let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
    let face_file_changes = persist_result(&transaction, request, face)?;
    transaction.commit()?;
    if let Some(changes) = face_file_changes {
        changes.commit();
    }
    Ok(())
}

fn prepare_face_result(
    connection: &Connection,
    request: &JobResult,
    process_config: &MediaProcessConfig,
) -> AppResult<Option<crate::processor::face_detection::PreparedFaceDetectionResult>> {
    if request.status != "completed" || request.task != "face_detection" {
        return Ok(None);
    }
    let job = connection
        .query_row(
            queries::llm_callback::SELECT_JOB,
            [&request.job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((media_id, task, attempts, status)): Option<(i64, String, i64, String)> = job else {
        return Ok(None);
    };
    if status != "submitted"
        || media_id != request.media_id
        || task != request.task
        || attempts != i64::from(request.attempt)
    {
        return Ok(None);
    }
    let model_type = request
        .model_type
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("modelType is required".to_string()))?;
    let model_version = request
        .model_version
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("modelVersion is required".to_string()))?;
    crate::processor::face_detection::prepare_result(
        connection,
        &request.job_id,
        request.media_id,
        model_type,
        model_version,
        request.input_results.as_deref(),
        process_config,
    )
    .map(Some)
}

fn persist_prepared_result(
    connection: &Connection,
    prepared: PreparedQueuedResult,
) -> AppResult<()> {
    let mut transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
    let mut face_file_changes = None;
    let mut permanent_failure = None;
    match prepared {
        PreparedQueuedResult::PermanentFailure { job_id, error } => {
            fail_received_result(&transaction, &job_id, &error)?;
            permanent_failure = Some((job_id, error));
        }
        PreparedQueuedResult::Result {
            job_id,
            request,
            face,
        } => {
            let result = {
                let mut savepoint = transaction.savepoint()?;
                match persist_result(&savepoint, *request, face) {
                    Ok(changes) => {
                        savepoint.execute(queries::llm_callback::DELETE_RESULT, [&job_id])?;
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
                Ok(changes) => face_file_changes = changes,
                Err(error) if result_error_is_retryable(&error) => return Err(error),
                Err(error) => {
                    fail_received_result(&transaction, &job_id, &error.to_string())?;
                    permanent_failure = Some((job_id, error.to_string()));
                }
            }
        }
    }
    transaction.commit()?;
    if let Some(changes) = face_file_changes {
        changes.commit();
    }
    if let Some((job_id, error)) = permanent_failure {
        tracing::error!(
            job_id,
            error,
            "Momento LLM result processing failed permanently"
        );
    }
    Ok(())
}

fn fail_received_result(connection: &Connection, job_id: &str, error: &str) -> AppResult<()> {
    connection.execute(
        queries::llm_callback::MARK_RECEIVED_RESULT_FAILED,
        rusqlite::params![error, job_id],
    )?;
    connection.execute(queries::llm_callback::DELETE_RESULT, [job_id])?;
    Ok(())
}

fn persist_result(
    connection: &Connection,
    request: JobResult,
    mut prepared_face: Option<crate::processor::face_detection::PreparedFaceDetectionResult>,
) -> AppResult<Option<crate::processor::face_detection::FaceFileChanges>> {
    let job: (i64, String, i64, String) = connection
        .query_row(
            queries::llm_callback::SELECT_JOB,
            [&request.job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| AppError::NotFound("LLM job not found".to_string()))?;
    if job.0 != request.media_id || job.1 != request.task || job.2 != i64::from(request.attempt) {
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
    if !matches!(request.status.as_str(), "completed" | "failed") {
        return Err(AppError::BadRequest(
            "LLM result status must be completed or failed".to_string(),
        ));
    }
    let mut face_file_changes = None;
    if request.status == "completed" {
        let model_type = request
            .model_type
            .ok_or_else(|| AppError::BadRequest("modelType is required".to_string()))?;
        let model_version = request
            .model_version
            .ok_or_else(|| AppError::BadRequest("modelVersion is required".to_string()))?;
        let result = request
            .result
            .ok_or_else(|| AppError::BadRequest("result is required".to_string()))?;
        if request.task == "image_clustering" {
            persist_clustering_result(connection, request.media_id, &model_version, &result)?;
        } else if request.task == "image_aesthetics" {
            if model_type != "image_aesthetics" {
                return Err(AppError::BadRequest(
                    "aesthetics result modelType must be image_aesthetics".to_string(),
                ));
            }
            persist_aesthetics_results(
                connection,
                request.media_id,
                &request.job_id,
                &model_version,
                &result,
                request.input_results.as_deref(),
            )?;
        } else if matches!(request.task.as_str(), "ocr" | "image_tagging") {
            persist_text_results(
                connection,
                request.media_id,
                &model_type,
                &model_version,
                &result,
                request.input_results.as_deref(),
            )?;
        } else if matches!(
            request.task.as_str(),
            SCREENSHOT_DETECTION_MODEL_TYPE | DOCUMENT_DETECTION_MODEL_TYPE
        ) {
            if model_type != request.task {
                return Err(AppError::BadRequest(format!(
                    "{} result modelType must match the task",
                    request.task
                )));
            }
            persist_classification_results(
                connection,
                request.media_id,
                &request.job_id,
                &request.task,
                &model_version,
                &result,
                request.input_results.as_deref(),
            )?;
        } else if request.task == "face_detection" {
            let prepared = prepared_face.take().ok_or_else(|| {
                AppError::Internal("face detection result was not prepared".to_string())
            })?;
            face_file_changes = Some(crate::processor::face_detection::persist_prepared_result(
                connection, prepared,
            )?);
        } else {
            return Err(AppError::BadRequest(
                "completed result task is not supported".to_string(),
            ));
        }
        if connection.execute(
            queries::llm_callback::MARK_COMPLETED,
            rusqlite::params![request.job_id, request.attempt],
        )? != 1
        {
            return Err(AppError::Conflict(
                "LLM job changed during result persistence".to_string(),
            ));
        }
    } else if connection.execute(
        queries::llm_callback::MARK_FAILED,
        rusqlite::params![
            request
                .error
                .unwrap_or_else(|| "LLM inference failed".to_string()),
            request.job_id,
            request.attempt
        ],
    )? != 1
    {
        return Err(AppError::Conflict(
            "LLM job changed during result persistence".to_string(),
        ));
    }
    Ok(face_file_changes)
}

#[derive(Clone, Copy, PartialEq)]
struct ClassificationResult {
    detected: bool,
    confidence: f64,
}

fn persist_classification_results(
    connection: &Connection,
    media_id: i64,
    job_id: &str,
    task: &str,
    model_version: &str,
    result: &serde_json::Value,
    input_results: Option<&[JobInputResult]>,
) -> AppResult<()> {
    let aggregate = parse_classification_result(result, task)?;
    let input_results = input_results
        .filter(|results| !results.is_empty())
        .ok_or_else(|| AppError::BadRequest(format!("{task} inputResults are required")))?;
    let submitted_inputs = connection
        .prepare(queries::llm_callback::SELECT_JOB_INPUT_CORRELATION)?
        .query_map([job_id], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, Option<i64>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if submitted_inputs.len() != input_results.len() {
        return Err(AppError::BadRequest(format!(
            "{task} inputResults do not match submitted inputs"
        )));
    }

    let mut sequences = HashSet::with_capacity(input_results.len());
    let mut first_input_result = None;
    for (input_result, submitted_input) in input_results.iter().zip(&submitted_inputs) {
        if !sequences.insert(input_result.sequence) {
            return Err(AppError::BadRequest(format!(
                "{task} inputResults contain duplicate sequences"
            )));
        }
        if (input_result.sequence, input_result.frame_timestamp_ms) != *submitted_input {
            return Err(AppError::BadRequest(format!(
                "{task} inputResults do not match submitted inputs"
            )));
        }
        let classification = parse_classification_result(&input_result.result, task)?;
        first_input_result.get_or_insert(classification);
        let query = match task {
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
        connection.execute(
            query,
            rusqlite::params![
                media_id,
                input_result.sequence,
                input_result.frame_timestamp_ms,
                model_version,
                classification.detected,
                classification.confidence
            ],
        )?;
    }
    if first_input_result != Some(aggregate) {
        return Err(AppError::BadRequest(format!(
            "{task} aggregate must match the first input result"
        )));
    }
    let query = match task {
        SCREENSHOT_DETECTION_MODEL_TYPE => queries::llm_callback::UPSERT_SCREENSHOT_CLASSIFICATION,
        DOCUMENT_DETECTION_MODEL_TYPE => queries::llm_callback::UPSERT_DOCUMENT_CLASSIFICATION,
        _ => {
            return Err(AppError::BadRequest(
                "classification task is not supported".to_string(),
            ));
        }
    };
    connection.execute(
        query,
        rusqlite::params![
            media_id,
            model_version,
            aggregate.detected,
            aggregate.confidence
        ],
    )?;
    Ok(())
}

fn parse_classification_result(
    result: &serde_json::Value,
    task: &str,
) -> AppResult<ClassificationResult> {
    let detected = result
        .get("detected")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| AppError::BadRequest(format!("{task} detected is required")))?;
    let confidence = result
        .get("confidence")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| AppError::BadRequest(format!("{task} confidence is required")))?;
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Err(AppError::BadRequest(format!(
            "{task} confidence must be within [0, 1]"
        )));
    }
    Ok(ClassificationResult {
        detected,
        confidence,
    })
}

#[derive(Clone, Copy, PartialEq)]
struct AestheticScores {
    aesthetic: f64,
    scenic: f64,
    simplicity: f64,
    landscape: f64,
    technical_quality: f64,
}

fn persist_aesthetics_results(
    connection: &Connection,
    media_id: i64,
    job_id: &str,
    model_version: &str,
    result: &serde_json::Value,
    input_results: Option<&[JobInputResult]>,
) -> AppResult<()> {
    let aggregate = parse_aesthetic_scores(result)?;
    let input_results = input_results
        .filter(|results| !results.is_empty())
        .ok_or_else(|| AppError::BadRequest("aesthetics inputResults are required".to_string()))?;
    let submitted_inputs = connection
        .prepare(queries::llm_callback::SELECT_JOB_INPUT_CORRELATION)?
        .query_map([job_id], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, Option<i64>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if submitted_inputs.len() != input_results.len() {
        return Err(AppError::BadRequest(
            "aesthetics inputResults do not match submitted inputs".to_string(),
        ));
    }
    let mut sequences = HashSet::with_capacity(input_results.len());
    let mut first_input_scores = None;
    for (input_result, submitted_input) in input_results.iter().zip(&submitted_inputs) {
        if !sequences.insert(input_result.sequence) {
            return Err(AppError::BadRequest(
                "aesthetics inputResults contain duplicate sequences".to_string(),
            ));
        }
        if (input_result.sequence, input_result.frame_timestamp_ms) != *submitted_input {
            return Err(AppError::BadRequest(
                "aesthetics inputResults do not match submitted inputs".to_string(),
            ));
        }
        let scores = parse_aesthetic_scores(&input_result.result)?;
        first_input_scores.get_or_insert(scores);
        connection.execute(
            queries::llm_callback::UPSERT_AESTHETIC_INPUT,
            rusqlite::params![
                media_id,
                input_result.sequence,
                input_result.frame_timestamp_ms,
                model_version,
                scores.aesthetic,
                scores.scenic,
                scores.simplicity,
                scores.landscape,
                scores.technical_quality
            ],
        )?;
    }
    if first_input_scores != Some(aggregate) {
        return Err(AppError::BadRequest(
            "aesthetics aggregate must match the first input result".to_string(),
        ));
    }
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

fn parse_aesthetic_scores(result: &serde_json::Value) -> AppResult<AestheticScores> {
    Ok(AestheticScores {
        aesthetic: parse_bounded_score(result, "aestheticScore")?,
        scenic: parse_bounded_score(result, "scenicScore")?,
        simplicity: parse_bounded_score(result, "simplicityScore")?,
        landscape: parse_bounded_score(result, "landscapeScore")?,
        technical_quality: parse_bounded_score(result, "technicalQualityScore")?,
    })
}

fn parse_bounded_score(result: &serde_json::Value, field: &str) -> AppResult<f64> {
    let score = result
        .get(field)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| AppError::BadRequest(format!("aesthetics {field} is required")))?;
    if !score.is_finite() || !(0.0..=1.0).contains(&score) {
        return Err(AppError::BadRequest(format!(
            "aesthetics {field} must be within [0, 1]"
        )));
    }
    Ok(score)
}

fn persist_text_results(
    connection: &Connection,
    media_id: i64,
    model_type: &str,
    model_version: &str,
    result: &serde_json::Value,
    input_results: Option<&[JobInputResult]>,
) -> AppResult<()> {
    let text = input_results
        .filter(|results| !results.is_empty())
        .map(|results| {
            results
                .iter()
                .filter_map(|input_result| {
                    input_result
                        .result
                        .get("text")
                        .and_then(|value| value.as_str())
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| {
            result
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string()
        });
    connection.execute(
        queries::llm_callback::UPSERT_TEXT,
        rusqlite::params![media_id, model_type, model_version, text],
    )?;
    let Some(input_results) = input_results else {
        return Ok(());
    };
    for input_result in input_results {
        let input_text = input_result
            .result
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        connection.execute(
            queries::llm_callback::UPSERT_INPUT_TEXT,
            rusqlite::params![
                media_id,
                model_type,
                input_result.sequence,
                input_result.frame_timestamp_ms,
                model_version,
                input_text
            ],
        )?;
    }
    Ok(())
}

fn persist_clustering_result(
    connection: &Connection,
    media_id: i64,
    model_version: &str,
    result: &serde_json::Value,
) -> AppResult<()> {
    let embedding = result
        .get("embedding")
        .and_then(|value| value.as_str())
        .ok_or_else(|| AppError::BadRequest("clustering embedding is required".to_string()))?;
    let encoding = result
        .get("embeddingEncoding")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            AppError::BadRequest("clustering embeddingEncoding is required".to_string())
        })?;
    if encoding != "float32_le" {
        return Err(AppError::BadRequest(
            "clustering embedding must use float32_le encoding".to_string(),
        ));
    }
    let embedding_bytes =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, embedding).map_err(
            |error| AppError::BadRequest(format!("invalid clustering embedding: {error}")),
        )?;
    let dimensions = result
        .get("embeddingDimensions")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| {
            AppError::BadRequest("clustering embeddingDimensions is required".to_string())
        })? as usize;
    if dimensions != IMAGE_CLUSTERING_EMBEDDING_DIMENSIONS
        || embedding_bytes.len() != dimensions * std::mem::size_of::<f32>()
    {
        return Err(AppError::BadRequest(
            "clustering embedding has invalid dimensions".to_string(),
        ));
    }
    let perceptual_hash = result
        .get("perceptualHash")
        .and_then(|value| value.as_str())
        .ok_or_else(|| AppError::BadRequest("clustering perceptualHash is required".to_string()))?;
    let perceptual_hash = u64::from_str_radix(perceptual_hash, 16).map_err(|error| {
        AppError::BadRequest(format!("invalid clustering perceptualHash: {error}"))
    })?;
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

#[cfg(test)]
mod tests {
    use std::sync::{mpsc, Arc, Condvar, Mutex};
    use std::time::Duration;

    use super::RollingCpuWorkers;
    use crate::error::AppError;

    #[test]
    fn rolling_cpu_workers_return_fast_work_without_waiting_for_slow_work() {
        let slow_release = Arc::new((Mutex::new(false), Condvar::new()));
        let (slow_started_sender, slow_started_receiver) = mpsc::channel();
        let worker_slow_release = Arc::clone(&slow_release);
        let workers = RollingCpuWorkers::new(2, "test-cpu", move |input| {
            if input == "slow" {
                slow_started_sender.send(()).expect("slow-start signal");
                let (release_lock, release_condition) = &*worker_slow_release;
                let mut released = release_lock.lock().expect("slow release lock");
                while !*released {
                    released = release_condition.wait(released).expect("slow release wait");
                }
            }
            input
        })
        .expect("rolling CPU workers");

        workers.submit("slow").expect("submit slow work");
        slow_started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("slow work must start");
        workers.submit("fast").expect("submit fast work");

        let first_output = workers
            .receive_timeout(Duration::from_secs(1))
            .expect("receive fast work")
            .expect("fast work must complete");
        assert_eq!(first_output, "fast");

        workers
            .submit("fast-refill")
            .expect("refill the completed worker slot");
        let refilled_output = workers
            .receive_timeout(Duration::from_secs(1))
            .expect("receive refilled work")
            .expect("refilled work must complete before slow work");
        assert_eq!(refilled_output, "fast-refill");

        let (release_lock, release_condition) = &*slow_release;
        *release_lock.lock().expect("slow release lock") = true;
        release_condition.notify_all();
        assert_eq!(workers.receive().expect("receive slow work"), "slow");
    }

    #[test]
    fn rolling_cpu_workers_reject_zero_concurrency() {
        let error = RollingCpuWorkers::new(0, "test-cpu", |input: usize| input)
            .err()
            .expect("zero concurrency must fail");

        assert!(matches!(error, AppError::Validation(_)));
    }
}
