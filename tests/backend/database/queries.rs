use momento_api::database::queries;

use crate::test_utils::{create_test_db, create_test_media, create_test_user, grant_media_access};

#[test]
fn shared_ai_admission_materializes_active_jobs_and_preserves_retry_eligibility() {
    assert!(queries::ai_jobs::insert_eligible("unknown").is_none());
    for task in [
        "ocr",
        "image_tagging",
        "image_aesthetics",
        "screenshot_detection",
        "document_detection",
        "face_detection",
    ] {
        let pool = create_test_db();
        let connection = pool.get().unwrap();
        connection
            .execute(
                "INSERT INTO face_grouping_runs(id,status) VALUES (1,'running')",
                [],
            )
            .unwrap();
        let run_id = (task == "face_detection").then_some(1_i64);
        let plan = connection
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                queries::ai_jobs::insert_eligible(task).unwrap()
            ))
            .unwrap()
            .query_map(rusqlite::params![task, run_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let (_, parent, _) = plan
            .iter()
            .find(|(_, _, detail)| detail.contains("llm_jobs"))
            .unwrap();
        assert!(
            plan.iter()
                .any(|(id, _, detail)| id == parent && detail.contains("LIST SUBQUERY")),
            "{task}: {plan:?}"
        );
        for status in [
            "queued",
            "submitting",
            "submitted",
            "failed",
            "cancelled",
            "completed",
        ] {
            let media_id = create_test_media(&pool, &format!("{task}-{status}.jpg"));
            connection
                .execute(
                    "INSERT INTO media_metadata_jobs(media_id,status) VALUES (?,'completed')",
                    [media_id],
                )
                .unwrap();
            connection.execute("INSERT INTO media_ai_inputs(media_id,task,sequence,input_kind,storage_root,file_path,filename,mime_type,byte_size,content_hash) VALUES (?,?,0,'image','originals','test.jpg','test.jpg','image/jpeg',1,'hash')", rusqlite::params![media_id, task]).unwrap();
            connection.execute("INSERT INTO llm_jobs(id,media_id,task,status,face_grouping_run_id) VALUES (?,?,?,?,?)", rusqlite::params![format!("{media_id:032x}"), media_id, task, status, run_id]).unwrap();
        }
        assert_eq!(
            connection
                .execute(
                    &queries::ai_jobs::insert_eligible(task).unwrap(),
                    rusqlite::params![task, run_id]
                )
                .unwrap(),
            3,
            "{task}"
        );
        assert_eq!(
            connection
                .execute(
                    &queries::ai_jobs::insert_eligible(task).unwrap(),
                    rusqlite::params![task, run_id]
                )
                .unwrap(),
            0,
            "{task}"
        );
    }
}

#[test]
fn clustering_admission_materializes_existing_run_media_once() {
    let pool = create_test_db();
    let connection = pool.get().unwrap();
    let plan = connection
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            queries::ai_jobs::insert_eligible("image_clustering").unwrap()
        ))
        .unwrap()
        .query_map(rusqlite::params!["image_clustering", 1], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(3)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|(_, detail)| detail.contains("LIST SUBQUERY")),
        "{plan:?}"
    );
    // The llm_jobs scan must be inside the uncorrelated list, not a per-media lookup.
    let full_plan = connection
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            queries::ai_jobs::insert_eligible("image_clustering").unwrap()
        ))
        .unwrap()
        .query_map(rusqlite::params!["image_clustering", 1], |row| {
            Ok((row.get::<_, i64>(1)?, row.get::<_, String>(3)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let (parent, _) = full_plan
        .iter()
        .find(|(_, detail)| detail.contains("SCAN llm_jobs"))
        .unwrap();
    assert!(
        plan.iter()
            .any(|(id, detail)| id == parent && detail.contains("LIST SUBQUERY")),
        "{plan:?}"
    );
}

#[test]
fn clustering_admission_preserves_all_status_exclusions_and_other_run_eligibility() {
    let pool = create_test_db();
    let mut connection = pool.get().unwrap();
    connection.execute("INSERT INTO media_similarity_runs(id,trigger,status) VALUES (1,'manual','failed'),(2,'manual','running')", []).unwrap();
    let transaction = connection.transaction().unwrap();
    for (index, status) in [
        "queued",
        "submitting",
        "submitted",
        "completed",
        "failed",
        "cancelled",
    ]
    .iter()
    .enumerate()
    {
        let media_id = index as i64 + 1;
        transaction.execute("INSERT INTO media(id,filename,original_filename,file_path,media_type,import_state) VALUES (?,'test.jpg','test.jpg',?,'image','imported')", rusqlite::params![media_id, format!("{media_id}.jpg")]).unwrap();
        transaction
            .execute(
                "INSERT INTO media_metadata_jobs(media_id,status) VALUES (?,'completed')",
                [media_id],
            )
            .unwrap();
        transaction.execute("INSERT INTO media_ai_inputs(media_id,task,sequence,input_kind,storage_root,file_path,filename,mime_type,byte_size,content_hash) VALUES (?,'image_clustering',0,'image','originals',?,'test.jpg','image/jpeg',1,?)", rusqlite::params![media_id, format!("{media_id}.jpg"), "0".repeat(64)]).unwrap();
        transaction.execute("INSERT INTO llm_jobs(id,media_id,deduplicate_run_id,task,status) VALUES (?,?,1,'image_clustering',?)", rusqlite::params![format!("{media_id:032x}"),media_id,status]).unwrap();
    }
    assert_eq!(
        transaction
            .execute(
                &queries::ai_jobs::insert_eligible("image_clustering").unwrap(),
                rusqlite::params!["image_clustering", 1]
            )
            .unwrap(),
        0
    );
    assert_eq!(
        transaction
            .execute(
                &queries::ai_jobs::insert_eligible("image_clustering").unwrap(),
                rusqlite::params!["image_clustering", 2]
            )
            .unwrap(),
        6
    );
    assert_eq!(
        transaction
            .execute(
                &queries::ai_jobs::insert_eligible("image_clustering").unwrap(),
                rusqlite::params!["image_clustering", 2]
            )
            .unwrap(),
        0
    );
    transaction.rollback().unwrap();
}

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
fn ai_status_uses_latest_job_and_hides_superseded_failures() {
    let pool = create_test_db();
    let media = create_test_media(&pool, "latest.jpg");
    let connection = pool.get().unwrap();
    for (id, task, status) in [
        ("aa", "ocr", "failed"),
        ("bb", "ocr", "completed"),
        ("cc", "image_tagging", "failed"),
    ] {
        connection.execute("INSERT INTO llm_jobs(id,media_id,task,status,last_error) VALUES(?,?,?,?, 'test failure')",
            rusqlite::params![id, media, task, status]).unwrap();
    }
    let counts = connection
        .prepare(queries::ai_jobs::SELECT_LATEST_STATUS_COUNTS)
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        counts,
        vec![
            ("image_tagging".into(), "failed".into(), 1),
            ("ocr".into(), "completed".into(), 1)
        ]
    );
    let failures = connection
        .prepare(queries::ai_jobs::SELECT_LATEST_FAILURES)
        .unwrap()
        .query_map([], |row| row.get::<_, String>(2))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(failures, vec!["cc"]);
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
        plan.contains("SEARCH s USING INDEX idx_data_dir_space_reservations_state"),
        "{plan}"
    );
    assert!(
        plan.contains("SEARCH r USING INDEX sqlite_autoindex_llm_result_receipts_"),
        "{plan}"
    );
    assert!(!plan.contains("MULTI-INDEX OR"), "{plan}");
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
#[test]
fn submitted_reconciliation_guards_receipts_terminal_states_and_attempts() {
    use momento_api::database::queries::ai_jobs;
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE llm_jobs(id TEXT, media_id INTEGER, task TEXT, status TEXT, attempts INTEGER, state_version INTEGER, claimed_at TEXT, available_at TEXT, updated_at TEXT); CREATE TABLE llm_result_receipts(job_id TEXT, state TEXT);").unwrap();
    for (index, receipt) in [
        "receiving",
        "received",
        "processing",
        "cleanup_pending",
        "file_cleanup_pending",
        "failed",
        "discarded",
        "cleaned",
    ]
    .iter()
    .enumerate()
    {
        let id = format!("{index:032x}");
        connection.execute("INSERT INTO llm_jobs(id,media_id,task,status,attempts,state_version) VALUES (?,1,'ocr','submitted',2,5)", [&id]).unwrap();
        connection
            .execute(
                "INSERT INTO llm_result_receipts VALUES (?,?)",
                rusqlite::params![id, receipt],
            )
            .unwrap();
        assert_eq!(
            connection
                .execute(ai_jobs::REQUEUE_MISSING_SUBMITTED, rusqlite::params![id, 1])
                .unwrap(),
            0
        );
        let expected = usize::from(matches!(*receipt, "discarded" | "cleaned"));
        assert_eq!(
            connection
                .execute(ai_jobs::REQUEUE_MISSING_SUBMITTED, rusqlite::params![id, 2])
                .unwrap(),
            expected
        );
    }
    for status in ["cancelled", "completed", "failed", "submitting", "queued"] {
        connection.execute("INSERT INTO llm_jobs(id,media_id,task,status,attempts,state_version) VALUES (?,1,'ocr',?,2,5)", rusqlite::params![status,status]).unwrap();
        assert_eq!(
            connection
                .execute(
                    ai_jobs::REQUEUE_MISSING_SUBMITTED,
                    rusqlite::params![status, 2]
                )
                .unwrap(),
            0
        );
    }
    let count = connection
        .prepare(ai_jobs::SELECT_SUBMITTED_PAGE)
        .unwrap()
        .query_map([""], |r| r.get::<_, String>(0))
        .unwrap()
        .count();
    assert_eq!(count, 0);
}
