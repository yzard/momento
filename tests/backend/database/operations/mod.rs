use crate::test_utils::{create_test_db, create_test_media, create_test_user, grant_media_access};
use momento_api::config::{FaceGroupConfig, ThreadPoolConfig};
use momento_api::database::operations::{
    DeleteTrashMedia, DeleteTrashPage, FinishLlmSubmission, TrashDeletionOutcome,
};
use momento_api::processor::face_detection;
use momento_api::runtime::{ExecutorHandles, ExecutorRuntime, RuntimeSizing};

#[tokio::test]
async fn thumbnail_reconciliation_rejects_other_roots_and_unbounded_batches() {
    use momento_api::io::file::{NormalizedStoragePath, StorageRootId};
    let handles = crate::test_utils::test_executor_handles(create_test_db());
    let path = NormalizedStoragePath::parse("file.jpg").unwrap();
    for root in [StorageRootId::Originals, StorageRootId::Previews] {
        assert!(handles
            .sqlite
            .queue_unreferenced_thumbnails_durable(root, vec![path.clone()])
            .await
            .is_err());
    }
    assert!(handles
        .sqlite
        .queue_unreferenced_thumbnails_durable(StorageRootId::Thumbnails, vec![path; 65])
        .await
        .is_err());
}

#[tokio::test]
async fn metadata_replacement_atomically_transfers_old_publication_claims_to_cleanup() {
    use momento_api::database::operations::{MetadataValuesWrite, PersistMetadataGeneration};
    use momento_api::io::file::NormalizedStoragePath;
    for (valid_version, published) in [(false, true), (true, true), (true, false)] {
        let should_commit = valid_version && published;
        let pool = create_test_db();
        let media_id = create_test_media(&pool, "retirement.jpg");
        let token = uuid::Uuid::new_v4().to_string();
        let old_path = "media/1/old/thumbnail.jpg";
        let conn = pool.get().unwrap();
        conn.execute("INSERT INTO media_metadata(media_id, thumbnail_path, preview_path) VALUES (?, ?, 'media/1/old/preview.jpg') ON CONFLICT(media_id) DO UPDATE SET thumbnail_path=excluded.thumbnail_path, preview_path=excluded.preview_path", rusqlite::params![media_id, old_path]).unwrap();
        conn.execute("INSERT INTO media_metadata_jobs(media_id,status,claim_token) VALUES (?,'processing',?) ON CONFLICT(media_id) DO UPDATE SET status='processing',claim_token=excluded.claim_token", rusqlite::params![media_id, token]).unwrap();
        conn.execute("INSERT INTO file_operation_groups(id,kind,owner_kind,owner_id,state,completion_outcome,entry_count) VALUES ('old','metadata_artifacts','metadata_generation',?,'cleanup_pending','published',1)", [media_id.to_string()]).unwrap();
        if !published {
            conn.execute("UPDATE file_operation_groups SET cancel_requested=1,completion_outcome='discarded' WHERE id='old'", []).unwrap();
        }
        conn.execute("INSERT INTO file_operation_entries(group_id,sequence,action,storage_root,temporary_path,destination_path,state) VALUES ('old',0,'publish','thumbnails','media/1/old/tmp',?,'committed')", [old_path]).unwrap();
        conn.execute("INSERT INTO file_operation_path_claims(group_id,sequence,storage_root,relative_path,path_key,mode,scope,role) VALUES ('old',0,'thumbnails',?,?,'write','exact','metadata_artifact_destination')", rusqlite::params![old_path, NormalizedStoragePath::parse(old_path).unwrap().path_key()]).unwrap();
        conn.execute("INSERT INTO file_operation_groups(id,kind,owner_kind,owner_id,claim_token,state,product_target,product_version,entry_count) VALUES ('new','metadata_artifacts','metadata_generation',?,?,'files_committed','metadata_artifacts',2,1)", rusqlite::params![media_id.to_string(), token]).unwrap();
        drop(conn);
        let (_, runtime, handles) = start_runtime(pool.clone());
        let result = handles
            .sqlite
            .persist_metadata_generation_durable(PersistMetadataGeneration {
                media_id,
                claim_token: token,
                artifact_group_id: "new".into(),
                artifact_group_version: if valid_version { 1 } else { 2 },
                artifact_version: 2,
                thumbnail_path: "media/1/new/thumbnail.jpg".into(),
                preview_path: None,
                content_hash: "a".repeat(64),
                geohash: None,
                sources: vec![],
                ai_inputs: vec![momento_api::database::operations::MetadataAiInputWrite {
                    task: "ocr".into(),
                    sequence: 0,
                    input_kind: "image".into(),
                    storage_root: "originals".into(),
                    file_path: "retirement.jpg".into(),
                    filename: "retirement.jpg".into(),
                    mime_type: "image/jpeg".into(),
                    byte_size: 1,
                    content_hash: "a".repeat(64),
                    frame_timestamp_ms: None,
                }],
                metadata: MetadataValuesWrite {
                    width: Some(100),
                    height: Some(100),
                    date_taken: None,
                    gps_latitude: None,
                    gps_longitude: None,
                    gps_altitude: None,
                    camera_make: None,
                    camera_model: None,
                    lens_make: None,
                    lens_model: None,
                    iso: None,
                    exposure_time: None,
                    f_number: None,
                    focal_length: None,
                    focal_length_35mm: None,
                    location_city: None,
                    location_state: None,
                    location_country: None,
                    video_codec: None,
                    keywords: None,
                    duration_seconds: None,
                },
            })
            .await;
        assert_eq!(result.is_ok(), should_commit, "{result:?}");
        let conn = pool.get().unwrap();
        let claims: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM file_operation_path_claims WHERE group_id='old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let cleanup: i64 = conn.query_row("SELECT COUNT(*) FROM file_operation_entries WHERE group_id='metadata-retire-new' AND action='cleanup'", [], |row| row.get(0)).unwrap();
        let path: String = conn
            .query_row(
                "SELECT thumbnail_path FROM media_metadata WHERE media_id=?",
                [media_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(claims, if should_commit { 0 } else { 1 });
        assert_eq!(cleanup, if should_commit { 3 } else { 0 });
        assert_eq!(
            path,
            if should_commit {
                "media/1/new/thumbnail.jpg"
            } else {
                old_path
            }
        );
        drop(conn);
        runtime.shutdown().await.unwrap();
    }
}

fn start_runtime(
    pool: momento_api::database::DbPool,
) -> (std::path::PathBuf, ExecutorRuntime, ExecutorHandles) {
    let database_path = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name = 'main'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(std::path::PathBuf::from)
        .expect("database path");
    let directory = database_path
        .parent()
        .expect("database parent")
        .to_path_buf();
    let config_path = directory.join("config.toml");
    std::fs::write(&config_path, "# test config\n").expect("test config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("config identity")
        .identity;
    let sizing = RuntimeSizing::validate_worker_counts(
        &ThreadPoolConfig {
            cpu_workers: 1,
            network_io_workers: 2,
            storage_io_workers: 2,
            sqlite_workers: 2,
        },
        4 * 1024 * 1024 * 1024,
    )
    .expect("runtime sizing");
    let (runtime, handles) =
        ExecutorRuntime::start(&sizing, pool, identity, directory.clone(), None)
            .expect("executor runtime");
    (directory, runtime, handles)
}

fn trash_media(pool: &momento_api::database::DbPool, media_id: i64, user_id: i64) {
    pool.get()
        .expect("database")
        .execute(
            "UPDATE media_access SET deleted_at = datetime('now') WHERE media_id = ? AND user_id = ?",
            rusqlite::params![media_id, user_id],
        )
        .expect("move media to trash");
}

#[tokio::test]
async fn repeated_queue_capacity_deferrals_do_not_consume_submission_attempts() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "deferred.jpg");
    let job_id = "queue-capacity-deferred-job";
    pool.get()
        .expect("database")
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status, attempts, claimed_at, last_error) VALUES (?, ?, 'ocr', 'submitting', 4, datetime('now'), 'old error')",
            rusqlite::params![job_id, media_id],
        )
        .expect("LLM job");
    let (_directory, runtime, handles) = start_runtime(pool.clone());

    for deferral in 0..8 {
        handles
            .sqlite
            .finish_llm_submission_durable(FinishLlmSubmission::Deferred {
                job_id: job_id.to_string(),
                retry_after_seconds: 30,
            })
            .await
            .expect("defer submission");
        if deferral < 7 {
            pool.get()
                .expect("database")
                .execute(
                    "UPDATE llm_jobs SET status = 'submitting', claimed_at = datetime('now') WHERE id = ?",
                    [job_id],
                )
                .expect("reclaim deferred job fixture");
        }
    }

    let (status, attempts, last_error, completed_at, delay_seconds): (
        String,
        i64,
        Option<String>,
        Option<String>,
        i64,
    ) = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT status, attempts, last_error, completed_at, unixepoch(available_at) - unixepoch('now') FROM llm_jobs WHERE id = ?",
            [job_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("deferred state");
    assert_eq!(status, "queued");
    assert_eq!(attempts, 4);
    assert_eq!(last_error, None);
    assert_eq!(completed_at, None);
    assert!((29..=30).contains(&delay_seconds));

    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn prepared_input_loader_matches_the_shared_1024_input_manifest_bound() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "many-inputs.jpg");
    let mut connection = pool.get().expect("database");
    connection
        .execute(
            "INSERT INTO llm_jobs (id, media_id, task, status) VALUES ('many-inputs-job', ?, 'ocr', 'submitting')",
            [media_id],
        )
        .expect("LLM job");
    let transaction = connection.transaction().expect("input transaction");
    for sequence in 0..momento_common::llm::MAX_LLM_INPUTS_PER_JOB {
        transaction
            .execute(
                "INSERT INTO llm_job_inputs (job_id, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES ('many-inputs-job', ?, 'image', 'originals', ?, ?, 'image/jpeg', 1, ?)",
                rusqlite::params![
                    sequence as i64,
                    format!("many/{sequence}.jpg"),
                    format!("{sequence}.jpg"),
                    "0".repeat(64)
                ],
            )
            .expect("LLM input");
    }
    transaction.commit().expect("input commit");
    drop(connection);
    let (_directory, runtime, handles) = start_runtime(pool);

    let inputs = handles
        .sqlite
        .load_llm_prepared_inputs_durable("many-inputs-job".to_string())
        .await
        .expect("prepared inputs");
    assert_eq!(inputs.len(), momento_common::llm::MAX_LLM_INPUTS_PER_JOB);
    assert_eq!(inputs.first().expect("first input").sequence, 0);
    assert_eq!(inputs.last().expect("last input").sequence, 1023);
    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn permanent_delete_commits_database_and_cleanup_journal_atomically() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "owner", "owner@example.com");
    let media_id = create_test_media(&pool, "delete.jpg");
    grant_media_access(&pool, media_id, user_id);
    trash_media(&pool, media_id, user_id);
    let relative_original = format!("test/media/delete-{media_id}.jpg");
    pool.get()
        .expect("database")
        .execute(
            "UPDATE media SET file_path = ? WHERE id = ?",
            rusqlite::params![relative_original, media_id],
        )
        .expect("set file path");
    let (directory, runtime, handles) = start_runtime(pool.clone());
    let original_path = directory.join("originals").join(&relative_original);
    std::fs::create_dir_all(original_path.parent().expect("original parent"))
        .expect("original directory");
    std::fs::write(&original_path, b"original").expect("original file");
    let sidecar_path = directory
        .join("originals")
        .join(format!("{relative_original}.supplemental-metadata.json"));
    std::fs::write(&sidecar_path, b"{}").expect("sidecar file");
    let crop_directory = directory.join("previews/faces").join(media_id.to_string());
    std::fs::create_dir_all(&crop_directory).expect("crop directory");
    for index in 0..300 {
        std::fs::write(crop_directory.join(format!("face-{index}.jpg")), b"face")
            .expect("crop file");
    }

    let connection = pool.get().expect("database");
    connection
        .execute(
            momento_api::database::queries::media::INSERT_RTREE,
            rusqlite::params![media_id, 40.0, 40.0, -74.0, -74.0],
        )
        .expect("rtree row");
    connection
        .execute(
            "INSERT INTO media_similarity_index (media_id, content_hash, model_version, preprocessing_version, embedding, perceptual_hash) VALUES (?, ?, 'model', 'preprocess', X'00000000', 1)",
            rusqlite::params![media_id, format!("hash_{media_id}")],
        )
        .expect("similarity row");
    drop(connection);

    let outcome = handles
        .sqlite
        .delete_trash_media_request(DeleteTrashMedia {
            user_id,
            media_ids: vec![media_id],
        })
        .await
        .expect("delete operation");
    assert_eq!(
        outcome,
        TrashDeletionOutcome::Deleted {
            affected_count: 1,
            cleanup_groups: 1,
            has_more: false,
        }
    );
    let connection = pool.get().expect("database");
    for table in ["media", "media_rtree", "media_similarity_index"] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("row count");
        assert_eq!(count, 0, "{table} was not cleaned");
    }
    assert_eq!(
        connection
            .query_row(
                "SELECT state FROM file_operation_groups WHERE id = ?",
                [format!("media-delete-{media_id}")],
                |row| row.get::<_, String>(0),
            )
            .expect("journal state"),
        "cleanup_pending"
    );
    drop(connection);

    for _ in 0..4 {
        momento_api::io::recovery::recover_generic_file_operations(&handles)
            .await
            .expect("journal recovery");
        let state = pool
            .get()
            .expect("database")
            .query_row(
                "SELECT state FROM file_operation_groups WHERE id = ?",
                [format!("media-delete-{media_id}")],
                |row| row.get::<_, String>(0),
            )
            .expect("journal state");
        if state == "cleaned" {
            break;
        }
    }
    assert!(!original_path.exists());
    assert!(!sidecar_path.exists());
    let remaining_crops = std::fs::read_dir(&crop_directory)
        .map(|entries| entries.count())
        .unwrap_or(0);
    let journal_debug = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT state, version FROM file_operation_groups WHERE id = ?",
            [format!("media-delete-{media_id}")],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .expect("journal debug state");
    assert!(
        !crop_directory.exists(),
        "crop directory remains with {remaining_crops} files; journal={journal_debug:?}"
    );
    assert_eq!(
        pool.get()
            .expect("database")
            .query_row(
                "SELECT state FROM file_operation_groups WHERE id = ?",
                [format!("media-delete-{media_id}")],
                |row| row.get::<_, String>(0),
            )
            .expect("journal state"),
        "cleaned"
    );

    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn trash_page_delete_reserves_bulk_sqlite_capacity() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "bulk-trash", "bulk-trash@example.com");
    let media_id = create_test_media(&pool, "bulk-trash.jpg");
    grant_media_access(&pool, media_id, user_id);
    trash_media(&pool, media_id, user_id);
    let (_directory, runtime, handles) = start_runtime(pool.clone());
    let writer = pool.get().expect("database writer");
    writer
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold writer lock");
    let delete_handles = handles.clone();
    let deletion = tokio::spawn(async move {
        delete_handles
            .sqlite
            .delete_trash_page_request(DeleteTrashPage {
                user_id,
                limit: 128,
            })
            .await
    });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
    let reserved = loop {
        let outstanding = handles
            .file_io
            .space_budget_snapshot()
            .expect("space budget")
            .sqlite_outstanding_bytes;
        if outstanding > 0 {
            break outstanding;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "trash deletion did not reserve SQLite capacity"
        );
        tokio::task::yield_now().await;
    };
    assert_eq!(reserved, 32 * 1024 * 1024);
    writer
        .execute_batch("ROLLBACK")
        .expect("release writer lock");
    let outcome = deletion
        .await
        .expect("trash deletion task")
        .expect("bulk trash page deletion");
    assert_eq!(
        outcome,
        TrashDeletionOutcome::Deleted {
            affected_count: 1,
            cleanup_groups: 1,
            has_more: false,
        }
    );
    assert_eq!(
        pool.get()
            .expect("database")
            .query_row(
                "SELECT COUNT(*) FROM media WHERE id = ?",
                [media_id],
                |row| row.get::<_, i64>(0),
            )
            .expect("media count"),
        0
    );
    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn deleting_cluster_member_marks_remaining_member_dirty() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "owner2", "owner2@example.com");
    let deleted_media_id = create_test_media(&pool, "deleted.jpg");
    let remaining_media_id = create_test_media(&pool, "remaining.jpg");
    grant_media_access(&pool, deleted_media_id, user_id);
    trash_media(&pool, deleted_media_id, user_id);
    let connection = pool.get().expect("database");
    connection
        .execute("DELETE FROM media_similarity_dirty", [])
        .expect("reset dirty rows");
    connection
        .execute(
            "INSERT INTO media_similarity_clusters (kind, representative_media_id) VALUES ('burst', ?)",
            [deleted_media_id],
        )
        .expect("cluster");
    let cluster_id = connection.last_insert_rowid();
    for media_id in [deleted_media_id, remaining_media_id] {
        connection
            .execute(
                "INSERT INTO media_similarity_cluster_members (cluster_id, media_id, cosine_similarity, perceptual_hash_distance) VALUES (?, ?, 1.0, 0)",
                rusqlite::params![cluster_id, media_id],
            )
            .expect("cluster member");
    }
    drop(connection);
    let (_directory, runtime, handles) = start_runtime(pool.clone());

    handles
        .sqlite
        .delete_trash_media_request(DeleteTrashMedia {
            user_id,
            media_ids: vec![deleted_media_id],
        })
        .await
        .expect("delete operation");

    let connection = pool.get().expect("database");
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM media_similarity_clusters",
                [],
                |row| row.get::<_, i64>(0)
            )
            .expect("cluster count"),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM media_similarity_dirty WHERE media_id = ?",
                [remaining_media_id],
                |row| row.get::<_, i64>(0),
            )
            .expect("dirty count"),
        1
    );
    drop(connection);
    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn permanent_delete_requires_trashed_access() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "active-owner", "active-owner@example.com");
    let media_id = create_test_media(&pool, "active.jpg");
    grant_media_access(&pool, media_id, user_id);
    let (_directory, runtime, handles) = start_runtime(pool.clone());

    assert_eq!(
        handles
            .sqlite
            .delete_trash_media_request(DeleteTrashMedia {
                user_id,
                media_ids: vec![media_id],
            })
            .await
            .expect("delete operation"),
        TrashDeletionOutcome::Deleted {
            affected_count: 0,
            cleanup_groups: 0,
            has_more: false,
        }
    );
    assert_eq!(
        pool.get()
            .expect("database")
            .query_row(
                "SELECT COUNT(*) FROM media WHERE id = ?",
                [media_id],
                |row| row.get::<_, i64>(0)
            )
            .expect("media count"),
        1
    );
    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn deleting_one_users_access_preserves_shared_media_and_creates_no_cleanup() {
    let pool = create_test_db();
    let deleting_user_id = create_test_user(&pool, "shared-a", "shared-a@example.com");
    let remaining_user_id = create_test_user(&pool, "shared-b", "shared-b@example.com");
    let media_id = create_test_media(&pool, "shared.jpg");
    grant_media_access(&pool, media_id, deleting_user_id);
    grant_media_access(&pool, media_id, remaining_user_id);
    trash_media(&pool, media_id, deleting_user_id);
    let connection = pool.get().expect("database");
    connection
        .execute(
            "INSERT INTO media_similarity_index (media_id, content_hash, model_version, preprocessing_version, embedding, perceptual_hash) VALUES (?, ?, 'model', 'preprocess', X'00000000', 1)",
            rusqlite::params![media_id, format!("hash_{media_id}")],
        )
        .expect("similarity row");
    drop(connection);
    let (_directory, runtime, handles) = start_runtime(pool.clone());

    assert_eq!(
        handles
            .sqlite
            .delete_trash_media_request(DeleteTrashMedia {
                user_id: deleting_user_id,
                media_ids: vec![media_id],
            })
            .await
            .expect("delete operation"),
        TrashDeletionOutcome::Deleted {
            affected_count: 1,
            cleanup_groups: 0,
            has_more: false,
        }
    );
    let connection = pool.get().expect("database");
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM media_similarity_index WHERE media_id = ?",
                [media_id],
                |row| row.get::<_, i64>(0),
            )
            .expect("similarity count"),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM file_operation_groups", [], |row| row
                .get::<_, i64>(
                0
            ))
            .expect("journal count"),
        0
    );
    drop(connection);
    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn face_representatives_are_recomputed_in_bounded_pages() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "representative.jpg");
    let connection = pool.get().expect("database");
    for index in 0..129_i64 {
        connection
            .execute(
                "INSERT INTO media_faces (media_id, input_sequence, face_index, x, y, width, height, confidence, face_size_score, frontality_score, visibility_score, feature_clarity_score, embedding, crop_path) VALUES (?, 0, ?, 0.4, 0.4, 0.2, 0.2, ?, 0.5, 0.5, 0.5, 0.5, X'00000000', ?)",
                rusqlite::params![media_id, index, (index % 65) as f64 / 64.0, format!("faces/{index}.jpg")],
            )
            .expect("face");
    }
    connection
        .execute("INSERT INTO face_groups (manual_curated) VALUES (0)", [])
        .expect("group");
    for face_id in 1..=65_i64 {
        connection
            .execute(
                "INSERT INTO face_group_members (face_group_id, face_id, manual_anchor) VALUES (1, ?, 0)",
                [face_id],
            )
            .expect("member");
    }
    for face_id in 66..=129_i64 {
        connection
            .execute("INSERT INTO face_groups (manual_curated) VALUES (0)", [])
            .expect("group");
        let group_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO face_group_members (face_group_id, face_id, manual_anchor) VALUES (?, ?, 0)",
                rusqlite::params![group_id, face_id],
            )
            .expect("member");
    }
    drop(connection);
    let (_directory, runtime, handles) = start_runtime(pool.clone());
    face_detection::recompute_face_representatives(
        &handles,
        &FaceGroupConfig {
            confidence_weight: 1.0,
            face_size_weight: 0.0,
            center_proximity_weight: 0.0,
            frontality_weight: 0.0,
            visibility_weight: 0.0,
            feature_clarity_weight: 0.0,
            ..FaceGroupConfig::default()
        },
    )
    .await
    .expect("representative recomputation");

    let representatives = pool
        .get()
        .expect("database")
        .prepare("SELECT representative_face_id FROM face_groups ORDER BY id")
        .expect("query")
        .query_map([], |row| row.get::<_, i64>(0))
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("representatives");
    assert_eq!(representatives, (65..=129_i64).collect::<Vec<_>>());
    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}
