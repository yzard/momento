use crate::test_utils::{create_test_db, create_test_media};
use momento_api::database::queries;

#[test]
fn latest_ai_status_and_failures_use_history_index_and_preserve_latest_semantics() {
    let pool = create_test_db();
    let connection = pool.get().unwrap();
    connection
        .execute(
            "INSERT INTO media_similarity_runs(id,trigger,status) VALUES (1,'manual','running')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO face_grouping_runs(id,status) VALUES (1,'running')",
            [],
        )
        .unwrap();
    for task in [
        "ocr",
        "image_tagging",
        "image_aesthetics",
        "image_clustering",
        "face_detection",
        "screenshot_detection",
        "document_detection",
    ] {
        for status in [
            "queued",
            "submitting",
            "submitted",
            "completed",
            "cancelled",
            "failed",
        ] {
            let media_id = create_test_media(&pool, &format!("{task}-{status}.jpg"));
            for (suffix, job_status, error) in [
                ("old", "failed", Some("old failure")),
                ("new", status, Some("current failure")),
            ] {
                connection.execute("INSERT INTO llm_jobs(id,media_id,task,status,last_error,created_at,deduplicate_run_id,face_grouping_run_id) VALUES (?,?,?,?,?,'2000-01-01',?,?)", rusqlite::params![format!("{task}-{status}-{suffix}"),media_id,task,job_status,error,(task == "image_clustering").then_some(1_i64),(task == "face_detection").then_some(1_i64)]).unwrap();
            }
        }
    }
    let counts = |query: &str| {
        connection
            .prepare(query)
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
            .unwrap()
    };
    assert_eq!(counts(queries::ai::jobs::SELECT_LATEST_STATUS_COUNTS), counts("SELECT task,status,COUNT(*) FROM llm_jobs WHERE rowid IN (SELECT MAX(rowid) FROM llm_jobs GROUP BY media_id,task) GROUP BY task,status ORDER BY task,status"));
    let failures = connection
        .prepare(queries::ai::jobs::SELECT_LATEST_FAILURES)
        .unwrap()
        .query_map([], |row| row.get::<_, String>(2))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(failures.len(), 7);
    assert!(failures.iter().all(|id| id.ends_with("-failed-new")));
    connection
        .execute(
            "UPDATE llm_jobs SET last_error=NULL WHERE status='failed'",
            [],
        )
        .unwrap();
    assert!(connection
        .prepare(queries::ai::jobs::SELECT_LATEST_FAILURES)
        .unwrap()
        .query([])
        .unwrap()
        .next()
        .unwrap()
        .is_none());
    for query in [
        queries::ai::jobs::SELECT_LATEST_STATUS_COUNTS,
        queries::ai::jobs::SELECT_LATEST_FAILURES,
    ] {
        let plan = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
            .unwrap()
            .query_map([], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            plan.iter().any(|line| line
                .contains("SEARCH latest USING COVERING INDEX idx_llm_jobs_media_task_history")),
            "{plan:?}"
        );
        assert!(!plan.iter().any(|line| line == "SCAN llm_jobs"), "{plan:?}");
    }
}

#[test]
fn shared_ai_admission_materializes_active_jobs_and_preserves_retry_eligibility() {
    assert!(queries::ai::jobs::insert_eligible("unknown").is_none());
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
                queries::ai::jobs::insert_eligible(task).unwrap()
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
                    &queries::ai::jobs::insert_eligible(task).unwrap(),
                    rusqlite::params![task, run_id]
                )
                .unwrap(),
            3,
            "{task}"
        );
        assert_eq!(
            connection
                .execute(
                    &queries::ai::jobs::insert_eligible(task).unwrap(),
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
            queries::ai::jobs::insert_eligible("image_clustering").unwrap()
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
            queries::ai::jobs::insert_eligible("image_clustering").unwrap()
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
                &queries::ai::jobs::insert_eligible("image_clustering").unwrap(),
                rusqlite::params!["image_clustering", 1]
            )
            .unwrap(),
        0
    );
    assert_eq!(
        transaction
            .execute(
                &queries::ai::jobs::insert_eligible("image_clustering").unwrap(),
                rusqlite::params!["image_clustering", 2]
            )
            .unwrap(),
        6
    );
    assert_eq!(
        transaction
            .execute(
                &queries::ai::jobs::insert_eligible("image_clustering").unwrap(),
                rusqlite::params!["image_clustering", 2]
            )
            .unwrap(),
        0
    );
    transaction.rollback().unwrap();
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
        .prepare(queries::ai::jobs::SELECT_LATEST_STATUS_COUNTS)
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
        .prepare(queries::ai::jobs::SELECT_LATEST_FAILURES)
        .unwrap()
        .query_map([], |row| row.get::<_, String>(2))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(failures, vec!["cc"]);
}

#[test]
fn submitted_reconciliation_guards_receipts_terminal_states_and_attempts() {
    use momento_api::database::queries::ai::jobs;
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
                .execute(jobs::REQUEUE_MISSING_SUBMITTED, rusqlite::params![id, 1])
                .unwrap(),
            0
        );
        let expected = usize::from(matches!(*receipt, "discarded" | "cleaned"));
        assert_eq!(
            connection
                .execute(jobs::REQUEUE_MISSING_SUBMITTED, rusqlite::params![id, 2])
                .unwrap(),
            expected
        );
    }
    for status in ["cancelled", "completed", "failed", "submitting", "queued"] {
        connection.execute("INSERT INTO llm_jobs(id,media_id,task,status,attempts,state_version) VALUES (?,1,'ocr',?,2,5)", rusqlite::params![status,status]).unwrap();
        assert_eq!(
            connection
                .execute(
                    jobs::REQUEUE_MISSING_SUBMITTED,
                    rusqlite::params![status, 2]
                )
                .unwrap(),
            0
        );
    }
    let count = connection
        .prepare(jobs::SELECT_SUBMITTED_PAGE)
        .unwrap()
        .query_map([""], |r| r.get::<_, String>(0))
        .unwrap()
        .count();
    assert_eq!(count, 0);
}
