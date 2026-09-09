use std::time::Duration;

use momento_api::database::operations::BinaryMediaQuery;

#[tokio::test]
async fn metadata_queue_preserves_completed_videos_without_previews() {
    use crate::test_utils::{create_test_db, create_test_media, test_executor_handles};

    let pool = create_test_db();
    let mut cases = Vec::new();
    for (status, preview, expected) in [
        ("completed", None, "completed"),
        ("completed", Some(""), "completed"),
        ("completed", Some("video/preview.mp4"), "completed"),
        ("failed", None, "queued"),
        ("cancelled", None, "queued"),
        ("queued", None, "queued"),
    ] {
        let id = create_test_media(&pool, "video.mov");
        let connection = pool.get().unwrap();
        connection
            .execute("UPDATE media SET media_type='video' WHERE id=?", [id])
            .unwrap();
        connection
            .execute(
                "UPDATE media_metadata SET preview_path=? WHERE media_id=?",
                rusqlite::params![preview, id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO media_metadata_jobs(media_id,status) VALUES (?,?)",
                rusqlite::params![id, status],
            )
            .unwrap();
        cases.push((id, expected));
    }
    let missing = create_test_media(&pool, "new.mov");
    pool.get()
        .unwrap()
        .execute("DELETE FROM media_metadata WHERE media_id=?", [missing])
        .unwrap();
    let handles = test_executor_handles(pool.clone());
    assert_eq!(
        handles
            .sqlite
            .queue_incomplete_metadata_request()
            .await
            .unwrap(),
        3
    );
    cases.push((missing, "queued"));
    let connection = pool.get().unwrap();
    for (id, expected) in cases {
        let status: String = connection
            .query_row(
                "SELECT status FROM media_metadata_jobs WHERE media_id=?",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, expected);
    }
}

#[tokio::test]
async fn result_cleanup_with_no_remaining_receipt_is_idempotent() {
    let handles = crate::test_utils::test_executor_handles(crate::test_utils::create_test_db());
    for _ in 0..2 {
        let outcome = handles
            .sqlite
            .cleanup_llm_result_staging_page_durable("aa1234".into(), 256)
            .await
            .unwrap();
        assert!(outcome.complete);
        assert_eq!(outcome.deleted, 0);
    }
}

#[tokio::test]
async fn one_writer_drains_its_queue_while_readers_remain_available() {
    use momento_api::config::ThreadPoolConfig;
    use momento_api::database::{create_pool_at, init_database};
    use momento_api::processor::import::{CreateImportJobOutcome, ImportSource};
    use momento_api::runtime::{ExecutorRuntime, RuntimeSizing};
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    let directory = crate::temporary::tempdir().unwrap();
    let sizing = RuntimeSizing::validate_worker_counts(
        &ThreadPoolConfig {
            cpu_workers: 1,
            network_io_workers: 2,
            storage_io_workers: 2,
            sqlite_workers: 4,
        },
        4 * 1024 * 1024 * 1024,
    )
    .unwrap();
    let pool = create_pool_at(
        &directory.path().join("database.sqlite"),
        sizing.sqlite_workers,
    )
    .unwrap();
    init_database(&pool.get().unwrap()).unwrap();
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# SQLite concurrency test\n").unwrap();
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .unwrap()
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool.clone(),
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .unwrap();

    let writers = Arc::new(Mutex::new(HashSet::new()));
    let mut connections = (0..pool.max_size())
        .map(|_| pool.get().unwrap())
        .collect::<Vec<_>>();
    for connection in &mut connections {
        let writers = Arc::clone(&writers);
        connection.update_hook(Some(
            move |_: rusqlite::hooks::Action, _: &str, _: &str, _: i64| {
                writers
                    .lock()
                    .unwrap()
                    .insert(std::thread::current().name().unwrap().to_string());
            },
        ));
    }
    drop(connections);
    let lock = pool.get().unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let sources = [
        ImportSource::Local,
        ImportSource::Webdav,
        ImportSource::MobileBackup,
    ];
    let writes = futures::future::join_all(
        (0..24).map(|index| handles.sqlite.create_import_job_request(sources[index % 3])),
    );
    tokio::pin!(writes);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut writes)
            .await
            .is_err()
    );

    // More writes than the writer FIFO can hold must not occupy reader workers.
    let reads =
        futures::future::join_all((0..12).map(|sequence| handles.sqlite.probe_durable(sequence)));
    for result in tokio::time::timeout(Duration::from_secs(2), reads)
        .await
        .unwrap()
    {
        assert!(result.unwrap().1.starts_with("momento-sqlite-reader-"));
    }
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    let results = tokio::time::timeout(Duration::from_secs(5), writes)
        .await
        .unwrap();
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Ok(CreateImportJobOutcome::Created(_))))
            .count(),
        1
    );
    assert!(results.into_iter().all(|result| result.is_ok()));
    assert_eq!(
        *writers.lock().unwrap(),
        HashSet::from(["momento-sqlite-writer".to_string()])
    );
    // No pooled connection may retain query_only after a read finishes.
    let connections = (0..pool.max_size())
        .map(|_| pool.get().unwrap())
        .collect::<Vec<_>>();
    for connection in &connections {
        assert_eq!(
            connection
                .pragma_query_value(None, "query_only", |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    drop(connections);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn media_read_waits_through_connection_contention_then_returns_its_result() {
    let pool = crate::test_utils::create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool.clone());
    let mut connections = Vec::new();
    for _ in 0..pool.max_size() {
        connections.push(pool.get().unwrap());
    }
    let lookup = executors
        .sqlite
        .load_binary_media_request(BinaryMediaQuery {
            user_id: 1,
            media_id: 999,
            deleted: false,
        });
    tokio::pin!(lookup);
    // Longer than the executor's connection checkout deadline: the request
    // must retry asynchronously instead of returning DatabaseBusy/503.
    assert!(
        tokio::time::timeout(Duration::from_millis(5300), &mut lookup)
            .await
            .is_err()
    );
    drop(connections);
    assert!(tokio::time::timeout(Duration::from_secs(5), lookup)
        .await
        .unwrap()
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn media_read_does_not_retry_invalid_queries() {
    let pool = crate::test_utils::create_test_db();
    let executors = crate::test_utils::test_executor_handles(pool);
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        executors.sqlite.load_active_share_request(String::new()),
    )
    .await
    .unwrap();
    assert_eq!(
        result.unwrap_err().kind,
        momento_api::executor::ExecutorErrorKind::InvalidInput
    );
}
