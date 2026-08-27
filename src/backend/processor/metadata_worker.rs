use std::sync::Arc;

use futures::stream::{self, StreamExt};
use rusqlite::{params, OptionalExtension};
use tracing::warn;

use crate::config::Config;
use crate::database::{queries, DbPool};
use crate::processor::ai::input::AiInputStorage;

pub async fn run(config: Arc<Config>, pool: DbPool) {
    let interval = std::time::Duration::from_secs(config.metadata_worker.poll_interval_seconds);
    loop {
        if let Err(error) = process_cycle(&config, &pool).await {
            warn!("metadata worker cycle failed: {error}");
        }
        tokio::time::sleep(interval).await;
    }
}

pub fn queue_incomplete(pool: &DbPool) -> Result<usize, rusqlite::Error> {
    let connection = pool
        .get()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    connection.execute(queries::metadata_jobs::QUEUE_INCOMPLETE, [])
}

pub fn status_counts(pool: &DbPool) -> Result<Vec<(String, i64)>, rusqlite::Error> {
    let connection = pool
        .get()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let counts = connection
        .prepare(queries::metadata_jobs::SELECT_STATUS_COUNTS)?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect();
    counts
}

pub fn reset_all(pool: &DbPool) -> Result<i64, rusqlite::Error> {
    let connection = pool
        .get()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let media_ids = connection
        .prepare(queries::metadata_jobs::SELECT_ALL_MEDIA_IDS)?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(queries::metadata_jobs::DELETE_TEXT, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_TEXT_INPUTS, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_AI_INPUTS, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_LLM_JOBS, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_SIMILARITY_CLUSTERS, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_SIMILARITY_BANDS, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_SIMILARITY_INDEX, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_SIMILARITY_DIRTY, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_FACE_GROUPING_RUNS, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_FACE_GROUPS, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_MEDIA_FACES, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_FACE_DETECTION_RESULTS, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_AESTHETICS, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_AESTHETIC_INPUTS, [])?;
    transaction.execute(
        queries::metadata_jobs::DELETE_SCREENSHOT_CLASSIFICATIONS,
        [],
    )?;
    transaction.execute(
        queries::metadata_jobs::DELETE_SCREENSHOT_CLASSIFICATION_INPUTS,
        [],
    )?;
    transaction.execute(queries::metadata_jobs::DELETE_DOCUMENT_CLASSIFICATIONS, [])?;
    transaction.execute(
        queries::metadata_jobs::DELETE_DOCUMENT_CLASSIFICATION_INPUTS,
        [],
    )?;
    transaction.execute(queries::metadata_jobs::DELETE_RTREE, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_METADATA_SOURCES, [])?;
    transaction.execute(queries::metadata_jobs::DELETE_METADATA, [])?;
    transaction.execute(queries::metadata_jobs::RESET_IMPORTED, [])?;
    transaction.commit()?;
    for media_id in &media_ids {
        let _ = std::fs::remove_dir_all(
            crate::constants::paths()
                .thumbnails
                .join(media_id.to_string()),
        );
        let _ = std::fs::remove_dir_all(
            crate::constants::paths()
                .previews
                .join("faces")
                .join(media_id.to_string()),
        );
        let _ = std::fs::remove_dir_all(
            crate::constants::paths()
                .thumbnails_tiny
                .join(media_id.to_string()),
        );
        let _ = std::fs::remove_dir_all(
            crate::constants::paths()
                .thumbnails_places
                .join(media_id.to_string()),
        );
        let _ = std::fs::remove_dir_all(
            crate::constants::paths()
                .previews
                .join("ai")
                .join(media_id.to_string()),
        );
    }
    connection.execute(queries::metadata_jobs::MARK_IMPORTED_DIRTY, [])?;
    Ok(media_ids.len() as i64)
}

async fn process_cycle(config: &Config, pool: &DbPool) -> Result<(), rusqlite::Error> {
    queue_incomplete(pool)?;
    reclaim_expired_leases(pool, config.metadata_worker.lease_seconds)?;
    let concurrency = config.metadata_worker.concurrency.max(1);
    stream::unfold(pool, |pool| async move {
        match claim_next_job(pool) {
            Ok(Some(media_id)) => Some((media_id, pool)),
            Ok(None) => None,
            Err(error) => {
                warn!("failed to claim next metadata job: {error}");
                None
            }
        }
    })
    .for_each_concurrent(concurrency, |media_id| async move {
        let outcome = crate::processor::metadata::generate_media_metadata(pool, media_id, config)
            .await
            .and_then(|()| verify_ai_inputs(pool, media_id, config));
        if let Err(error) = finish_job(pool, media_id, outcome, config.metadata_worker.max_attempts)
        {
            warn!("failed to persist metadata job {media_id} outcome: {error}");
        }
    })
    .await;
    Ok(())
}

fn verify_ai_inputs(pool: &DbPool, media_id: i64, config: &Config) -> Result<(), String> {
    if !config.llm.enabled {
        return Ok(());
    }
    let connection = pool.get().map_err(|error| error.to_string())?;
    let media_type = connection
        .query_row(
            queries::metadata::SELECT_IMPORTED_MEDIA,
            [media_id],
            |row| row.get::<_, String>(1),
        )
        .map_err(|error| error.to_string())?;
    let mut tasks = vec![
        "ocr",
        "image_tagging",
        "image_clustering",
        "face_detection",
        "image_aesthetics",
    ];
    if media_type == "image" {
        tasks.push("screenshot_detection");
        tasks.push("document_detection");
    }
    for task in tasks {
        let inputs = connection
            .prepare(queries::metadata_jobs::SELECT_INPUT_PATHS)
            .map_err(|error| error.to_string())?
            .query_map(rusqlite::params![media_id, task], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        if inputs.is_empty() {
            return Err(format!("missing prepared {task} AI inputs"));
        }
        for (storage_root, file_path) in inputs {
            let storage = AiInputStorage::parse(&storage_root)?;
            if storage.resolve_existing_sync(&file_path).is_err() {
                return Err(format!("prepared {task} AI input file is missing"));
            }
        }
    }
    Ok(())
}

pub fn claim_next_job(pool: &DbPool) -> Result<Option<i64>, rusqlite::Error> {
    let mut connection = pool
        .get()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let transaction = connection.transaction()?;
    let media_id = transaction
        .query_row(queries::metadata_jobs::CLAIM_NEXT_QUEUED, [], |row| {
            row.get(0)
        })
        .optional()?;
    transaction.commit()?;
    Ok(media_id)
}

pub fn reclaim_expired_leases(pool: &DbPool, lease_seconds: u64) -> Result<(), rusqlite::Error> {
    let connection = pool
        .get()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    connection.execute(
        queries::metadata_jobs::RECLAIM_EXPIRED,
        [format!("-{} seconds", lease_seconds)],
    )?;
    Ok(())
}

pub fn finish_job(
    pool: &DbPool,
    media_id: i64,
    outcome: Result<(), String>,
    max_attempts: u32,
) -> Result<(), rusqlite::Error> {
    let connection = pool
        .get()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    match outcome {
        Ok(()) => connection.execute(queries::metadata_jobs::MARK_COMPLETED, [media_id])?,
        Err(error) => connection.execute(
            queries::metadata_jobs::MARK_FAILED_OR_RETRY,
            params![max_attempts, max_attempts, error, media_id],
        )?,
    };
    Ok(())
}
