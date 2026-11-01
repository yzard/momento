mod ai;

use momento_api::database::queries;

use crate::test_utils::{create_test_db, create_test_media, create_test_user, grant_media_access};

#[test]
fn replayable_receipt_retirement_requires_complete_cleanup_and_active_job() {
    for (receipt, journal, outcome, reservation, job, staging, expected) in [
        (
            "discarded",
            "cleaned",
            "discarded",
            "released",
            "submitted",
            false,
            1,
        ),
        (
            "discarded",
            "rolled_back",
            "",
            "released",
            "submitted",
            false,
            1,
        ),
        (
            "receiving",
            "cleaned",
            "discarded",
            "released",
            "submitted",
            false,
            0,
        ),
        (
            "processing",
            "cleaned",
            "discarded",
            "released",
            "submitted",
            false,
            0,
        ),
        (
            "discarded",
            "cleanup_pending",
            "discarded",
            "released",
            "submitted",
            false,
            0,
        ),
        (
            "discarded",
            "cleaned",
            "completed",
            "released",
            "submitted",
            false,
            0,
        ),
        (
            "discarded",
            "cleaned",
            "discarded",
            "active",
            "submitted",
            false,
            0,
        ),
        (
            "discarded",
            "cleaned",
            "discarded",
            "released",
            "cancelled",
            false,
            0,
        ),
        (
            "discarded",
            "cleaned",
            "discarded",
            "released",
            "completed",
            false,
            0,
        ),
        (
            "discarded",
            "cleaned",
            "discarded",
            "released",
            "submitted",
            true,
            0,
        ),
    ] {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE llm_result_receipts(job_id TEXT, state TEXT, journal_group_id TEXT, sqlite_reservation_id TEXT); CREATE TABLE llm_jobs(id TEXT, status TEXT); CREATE TABLE file_operation_groups(id TEXT, state TEXT, completion_outcome TEXT); CREATE TABLE data_dir_space_reservations(id TEXT, state TEXT); CREATE TABLE llm_result_staging(job_id TEXT);").unwrap();
        connection
            .execute(
                "INSERT INTO llm_result_receipts VALUES ('job', ?, 'group', 'reservation')",
                [receipt],
            )
            .unwrap();
        connection
            .execute("INSERT INTO llm_jobs VALUES ('job', ?)", [job])
            .unwrap();
        connection
            .execute(
                "INSERT INTO file_operation_groups VALUES ('group', ?, ?)",
                [journal, outcome],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO data_dir_space_reservations VALUES ('reservation', ?)",
                [reservation],
            )
            .unwrap();
        if staging {
            connection
                .execute("INSERT INTO llm_result_staging VALUES ('job')", [])
                .unwrap();
        }
        let count = |job_id: Option<&str>| {
            connection
                .prepare(queries::file_operations::SELECT_REPLAYABLE_TERMINAL_RESULT_RECEIPTS)
                .unwrap()
                .query_map([job_id], |row| row.get::<_, String>(0))
                .unwrap()
                .count()
        };
        assert_eq!(count(Some("other")), 0);
        assert_eq!(count(Some("job")), expected);
        assert_eq!(count(None), expected);
        assert_eq!(
            connection
                .execute(
                    queries::file_operations::DELETE_REPLAYABLE_RESULT_RECEIPT_AFTER_TERMINATION,
                    ["group"]
                )
                .unwrap(),
            expected
        );
    }
}

#[test]
fn result_cleanup_plan_starts_from_active_reservations_not_terminal_receipts() {
    let pool = create_test_db();
    let connection = pool.get().unwrap();
    let plan = connection
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            queries::llm_callback::SELECT_RESULT_STAGING_CLEANUP
        ))
        .unwrap()
        .query_map([18], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    assert!(
        plan.contains(
            "SEARCH s USING COVERING INDEX idx_data_dir_space_reservations_active_sqlite"
        ),
        "{plan}"
    );
    assert!(
        plan.contains("SEARCH r USING COVERING INDEX idx_llm_result_receipts_cleanup_reservation"),
        "{plan}"
    );
    assert!(!plan.contains("MULTI-INDEX OR"), "{plan}");
}

#[test]
fn result_cleanup_preserves_state_guards_and_fifo_limit() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection.execute_batch("
        CREATE TABLE llm_result_receipts(job_id TEXT PRIMARY KEY, updated_at TEXT, state TEXT, sqlite_reservation_id TEXT, journal_group_id TEXT);
        CREATE TABLE data_dir_space_reservations(id TEXT PRIMARY KEY, class TEXT, state TEXT);
        CREATE TABLE file_operation_groups(id TEXT PRIMARY KEY, state TEXT);
        CREATE INDEX idx_llm_result_receipts_cleanup_reservation ON llm_result_receipts(sqlite_reservation_id,state,updated_at,job_id,journal_group_id) WHERE state IN ('file_cleanup_pending','cleaned','discarded','failed');
    ").unwrap();
    let mut expected = Vec::new();
    let mut index = 0;
    for receipt in [
        "received",
        "processing",
        "cleanup_pending",
        "file_cleanup_pending",
        "cleaned",
        "discarded",
        "failed",
    ] {
        for reservation in ["active", "released"] {
            for class in ["sqlite", "journal"] {
                for group in ["cleanup_pending", "cleaned", "rolled_back"] {
                    let id = format!("{index:04}");
                    index += 1;
                    connection
                        .execute(
                            "INSERT INTO llm_result_receipts VALUES (?1,'2026-01-01',?2,?1,?1)",
                            rusqlite::params![id, receipt],
                        )
                        .unwrap();
                    connection
                        .execute(
                            "INSERT INTO data_dir_space_reservations VALUES (?,?,?)",
                            rusqlite::params![id, class, reservation],
                        )
                        .unwrap();
                    connection
                        .execute(
                            "INSERT INTO file_operation_groups VALUES (?,?)",
                            rusqlite::params![id, group],
                        )
                        .unwrap();
                    let eligible = receipt == "cleanup_pending"
                        || (class == "sqlite"
                            && reservation == "active"
                            && matches!(
                                receipt,
                                "file_cleanup_pending" | "cleaned" | "discarded" | "failed"
                            )
                            && (receipt != "file_cleanup_pending"
                                || matches!(group, "cleaned" | "rolled_back"))
                            && !(receipt == "discarded" && group == "rolled_back"));
                    if eligible {
                        expected.push(id);
                    }
                }
            }
        }
    }
    for limit in [0, 1, 16, 256] {
        let actual = connection
            .prepare(queries::llm_callback::SELECT_RESULT_STAGING_CLEANUP)
            .unwrap()
            .query_map([limit], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            actual,
            expected
                .iter()
                .take(limit as usize)
                .cloned()
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn linked_released_reservation_uses_point_lookups_and_only_returns_its_owner() {
    let pool = create_test_db();
    let connection = pool.get().unwrap();
    let plan = connection
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            queries::file_operations::SELECT_LINKED_RELEASED_SQLITE_RESULT_RESERVATION
        ))
        .unwrap()
        .query_map(["group"], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    assert!(plan.contains("SEARCH r USING INDEX"), "{plan}");
    assert!(plan.contains("SEARCH s USING INDEX"), "{plan}");
    assert!(
        !plan.contains("SCAN") && !plan.contains("LIST SUBQUERY"),
        "{plan}"
    );

    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "
        CREATE TABLE llm_result_receipts(journal_group_id TEXT, sqlite_reservation_id TEXT);
        CREATE TABLE data_dir_space_reservations(id TEXT PRIMARY KEY, state TEXT);
        INSERT INTO llm_result_receipts VALUES ('group','owned');
        INSERT INTO data_dir_space_reservations VALUES ('other','released');
    ",
        )
        .unwrap();
    for state in [None, Some("active"), Some("released")] {
        if let Some(state) = state {
            connection.execute("INSERT INTO data_dir_space_reservations VALUES ('owned',?) ON CONFLICT(id) DO UPDATE SET state=excluded.state", [state]).unwrap();
        }
        for group in ["group", "missing"] {
            let actual = connection
                .prepare(queries::file_operations::SELECT_LINKED_RELEASED_SQLITE_RESULT_RESERVATION)
                .unwrap()
                .query_map([group], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let expected = if group == "group" && state == Some("released") {
                vec!["owned"]
            } else {
                vec![]
            };
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn import_recovery_preserves_active_owners_and_terminal_media() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "recover.jpg");
    let connection = pool.get().unwrap();
    let hash = "a".repeat(64);
    connection.execute("UPDATE media SET import_state='importing', import_source_root='imports', import_source_path='nested/recover.jpg', content_hash=? WHERE id=?", rusqlite::params![hash, media_id]).unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    connection
        .execute(
            queries::import::INSERT_CONTENT_HASH_CLAIM,
            rusqlite::params![hash, token, "local"],
        )
        .unwrap();
    assert_eq!(
        connection
            .execute(queries::import::REQUEUE_INTERRUPTED_MEDIA, [media_id])
            .unwrap(),
        0
    );
    connection
        .execute(queries::import::RECOVER_CONTENT_HASH_CLAIMS, [])
        .unwrap();
    assert_eq!(
        connection
            .execute(queries::import::REQUEUE_INTERRUPTED_MEDIA, [media_id])
            .unwrap(),
        1
    );
    let state: String = connection
        .query_row(queries::import::SELECT_RECOVERY_STATE, [media_id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(state, "recovering");
    assert_eq!(
        connection
            .execute(
                queries::import::MARK_FAILED,
                rusqlite::params!["unsupported source", media_id]
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .execute(queries::import::REQUEUE_INTERRUPTED_MEDIA, [media_id])
            .unwrap(),
        0
    );
    connection
        .execute(
            "UPDATE media SET import_state='imported' WHERE id=?",
            [media_id],
        )
        .unwrap();
    assert_eq!(
        connection
            .execute(queries::import::REQUEUE_INTERRUPTED_MEDIA, [media_id])
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .execute(
                queries::import::MARK_FAILED,
                rusqlite::params!["late failure", media_id]
            )
            .unwrap(),
        0
    );
}

#[test]
fn thumbnail_query_distinguishes_missing_media_from_missing_metadata() {
    use rusqlite::OptionalExtension;

    let pool = create_test_db();
    let media_id = create_test_media(&pool, "thumbnail-query.jpg");
    let connection = pool.get().expect("connection");
    let read_thumbnail = |id| {
        connection
            .query_row(queries::public::SELECT_MEDIA_THUMBNAIL, [id], |row| {
                row.get::<_, Option<String>>(0)
            })
            .optional()
            .expect("thumbnail query")
    };
    assert_eq!(read_thumbnail(-1), None);
    assert_eq!(read_thumbnail(media_id), Some(None));
    connection
        .execute("DELETE FROM media_metadata WHERE media_id = ?", [media_id])
        .expect("remove metadata");
    assert_eq!(read_thumbnail(media_id), Some(None));
    connection
        .execute(
            "INSERT INTO media_metadata (media_id, thumbnail_path) VALUES (?, 'ready.jpg')",
            [media_id],
        )
        .expect("publish thumbnail");
    assert_eq!(
        read_thumbnail(media_id),
        Some(Some("ready.jpg".to_string()))
    );
}

#[test]
fn visible_cluster_page_canonicalizes_user_specific_media_sets() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "query-viewer", "query-viewer@example.com");
    let first = create_test_media(&pool, "visible-first.jpg");
    let second = create_test_media(&pool, "visible-second.jpg");
    let hidden_first = create_test_media(&pool, "hidden-first.jpg");
    let hidden_second = create_test_media(&pool, "hidden-second.jpg");
    grant_media_access(&pool, first, user_id);
    grant_media_access(&pool, second, user_id);
    let connection = pool.get().expect("Failed to get connection");
    let mut cluster_ids = Vec::new();
    for members in [
        [first, second, hidden_first],
        [first, second, hidden_second],
    ] {
        connection
            .execute(
                queries::deduplicate::INSERT_CLUSTER,
                rusqlite::params!["near_duplicate", first],
            )
            .expect("Failed to create cluster");
        let cluster_id = connection.last_insert_rowid();
        cluster_ids.push(cluster_id);
        for media_id in members {
            connection
                .execute(
                    queries::deduplicate::INSERT_CLUSTER_MEMBER,
                    rusqlite::params![cluster_id, media_id, 1.0_f32, 0_u32],
                )
                .expect("Failed to create cluster member");
        }
    }

    let rows = connection
        .prepare(queries::deduplicate::SELECT_VISIBLE_CLUSTER_PAGE)
        .expect("Failed to prepare visible cluster page query")
        .query_map(rusqlite::params![user_id, 0_i64, 11_i64], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .expect("Failed to query visible cluster page")
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to collect visible cluster page");

    assert_eq!(rows, vec![(Some(cluster_ids[0]), 1, 2)]);
}

mod file_operations;
