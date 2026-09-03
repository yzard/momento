use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;

use momento_api::config::ThreadPoolConfig;
use momento_api::database::{create_pool_at, init_database};
use momento_api::io::file::{NormalizedStoragePath, PathClaimMode, PathClaimScope, StorageRootId};
use momento_api::io::journal::{
    FileEntryAction, FileEntryPlan, FileOperationPlan, FilePathClaimPlan,
    JournalSpaceReservationPlan, PrepareJournalOutcome,
};
use momento_api::runtime::{ExecutorRuntime, RuntimeSizing};

mod control_json;

fn journal_reservation(
    file_io: &momento_api::executor::FileIoExecutorHandle,
    reservation_id: &str,
) -> JournalSpaceReservationPlan {
    let token = file_io
        .reserve_journal_space(reservation_id.to_string(), 4096)
        .expect("space admission")
        .into_result()
        .expect("journal capacity");
    JournalSpaceReservationPlan::new(token).expect("journal reservation")
}

#[tokio::test]
async fn operations_run_on_their_named_execution_domains() {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 2,
        io_workers: 4,
        sqlite_workers: 2,
    })
    .expect("runtime sizing");
    let pool = create_pool_at(
        &directory.path().join("database.sqlite"),
        sizing.sqlite_workers,
    )
    .expect("database pool");
    init_database(&pool.get().expect("schema connection")).expect("database schema");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config")
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool,
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .expect("executor runtime");

    let (_, cpu_thread) = handles.cpu.probe_durable(1).await.expect("CPU probe");
    let (_, file_thread) = handles.file_io.probe_durable(2).await.expect("file probe");
    let (_, sqlite_thread) = handles.sqlite.probe_durable(3).await.expect("SQLite probe");
    let (timer_sender, timer_receiver) = tokio::sync::oneshot::channel();
    handles
        .scheduler
        .spawn_scheduler_control(async move {
            let name = std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_string();
            let _ = timer_sender.send(name);
        })
        .expect("scheduler timer registration");
    let timer_thread = timer_receiver.await.expect("scheduler timer probe");

    assert!(cpu_thread.starts_with("momento-cpu-"), "{cpu_thread}");
    assert!(file_thread.starts_with("momento-io-file-"), "{file_thread}");
    assert!(
        sqlite_thread.starts_with("momento-sqlite-"),
        "{sqlite_thread}"
    );
    assert_eq!(timer_thread, "momento-scheduler");
    assert_eq!(
        handles
            .cpu
            .initialize_reverse_geocoder_durable()
            .await
            .expect("reverse geocoder initialization"),
        235_408
    );
    let location = handles
        .cpu
        .derive_media_location_durable(Some(40.759), Some(-73.9859))
        .await
        .expect("reverse geocoder lookup");
    assert_eq!(location.city.as_deref(), Some("Times Square"));

    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn startup_rolls_back_prepared_file_operations_before_listener_publication() {
    let pool = crate::test_utils::create_test_db();
    let (handles, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let group_id = format!("restart-prepared-{}", uuid::Uuid::new_v4());
    let temporary = NormalizedStoragePath::parse(&format!("{group_id}.tmp")).unwrap();
    let destination = NormalizedStoragePath::parse(&format!("{group_id}.bin")).unwrap();
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: "restart_prepared_test".to_string(),
        owner_kind: "test".to_string(),
        owner_id: group_id.clone(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::Journal,
            source_path: None,
            temporary_path: Some(temporary.clone()),
            destination_path: Some(destination),
            tombstone_path: None,
            expected_size: Some(1),
            expected_sha256: None,
            expected_version: None,
        }],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: temporary.clone(),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "temporary".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: NormalizedStoragePath::parse(&format!("{group_id}.bin")).unwrap(),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "destination".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: Some(journal_reservation(&handles.file_io, &group_id)),
    };
    assert_eq!(
        handles
            .sqlite
            .prepare_file_operation_durable(plan)
            .await
            .expect("prepare interrupted operation"),
        PrepareJournalOutcome::Prepared
    );
    let session = handles
        .file_io
        .open_storage_write_session_durable(StorageRootId::Journal, temporary.clone(), 0)
        .await
        .expect("open interrupted temporary");
    let (session, written) = handles
        .file_io
        .write_storage_session_durable(session, vec![1])
        .await
        .expect("write interrupted temporary");
    assert_eq!(written, 1);
    handles
        .file_io
        .commit_storage_session_durable(session)
        .await
        .expect("sync interrupted temporary");

    assert_eq!(
        momento_api::io::recovery::rollback_prepared_file_operations_after_restart(&handles)
            .await
            .expect("request startup rollback"),
        1
    );
    momento_api::io::recovery::recover_generic_file_operations(&handles)
        .await
        .expect("finish startup rollback");

    assert!(!data_directory
        .join("journal")
        .join(temporary.relative_path())
        .exists());
    let state: String = pool
        .get()
        .expect("database connection")
        .query_row(
            "SELECT state FROM file_operation_groups WHERE id = ?",
            [&group_id],
            |row| row.get(0),
        )
        .expect("prepared group state");
    assert_eq!(state, "rolled_back");
}

#[tokio::test]
async fn cpu_json_validation_rejects_structural_amplification_and_duplicate_fields() {
    let pool = crate::test_utils::create_test_db();
    let handles = crate::test_utils::test_executor_handles(pool);

    let valid = br#"{"items":[1,2,3],"name":"bounded"}"#.to_vec();
    assert_eq!(
        handles
            .cpu
            .validate_json_durable(valid.clone())
            .await
            .expect("bounded JSON"),
        valid
    );
    let duplicate = handles
        .cpu
        .validate_json_durable(br#"{"field":1,"field":2}"#.to_vec())
        .await
        .expect_err("duplicate JSON field must fail");
    assert!(duplicate.detail.contains("duplicate field"));

    let mut nested = String::new();
    nested.push_str(&"[".repeat(34));
    nested.push('0');
    nested.push_str(&"]".repeat(34));
    let depth_error = handles
        .cpu
        .validate_json_durable(nested.into_bytes())
        .await
        .expect_err("deep JSON must fail");
    assert!(depth_error.detail.contains("nesting"));

    let oversized_text = format!("\"{}\"", "x".repeat(256 * 1024 + 1));
    let text_error = handles
        .cpu
        .validate_json_durable(oversized_text.into_bytes())
        .await
        .expect_err("oversized decoded text must fail");
    assert!(text_error.detail.contains("decoded text"));

    let too_many_items = format!(
        "[{}]",
        std::iter::repeat_n("0", 8_193)
            .collect::<Vec<_>>()
            .join(",")
    );
    let item_error = handles
        .cpu
        .validate_json_durable(too_many_items.into_bytes())
        .await
        .expect_err("oversized JSON collection must fail");
    assert!(item_error.detail.contains("8192 collection items"));
}

#[tokio::test]
async fn metadata_json_parsers_return_typed_bounded_values_and_exclude_unrequested_exif_fields() {
    let pool = crate::test_utils::create_test_db();
    let handles = crate::test_utils::test_executor_handles(pool);

    let supplemental = handles
        .cpu
        .parse_supplemental_metadata_durable(
            br#"{"photoTakenTime":{"timestamp":"1530569813"},"geoData":{"latitude":40.759,"longitude":-73.9859},"description":"Times Square"}"#.to_vec(),
        )
        .await
        .expect("supplemental metadata");
    assert_eq!(supplemental.gps_latitude, Some(40.759));
    assert_eq!(supplemental.description.as_deref(), Some("Times Square"));
    assert_eq!(
        supplemental.payload_json,
        r#"{"photoTakenTime":{"timestamp":"1530569813"},"geoData":{"latitude":40.759,"longitude":-73.9859},"description":"Times Square"}"#
    );

    let duplicate_field = handles
        .cpu
        .parse_supplemental_metadata_durable(
            br#"{"description":"first","description":"second"}"#.to_vec(),
        )
        .await
        .expect_err("duplicate metadata fields must fail");
    assert!(duplicate_field.detail.contains("duplicate field"));

    let deep_metadata = format!("{}0{}", "[".repeat(17), "]".repeat(17));
    let nesting_error = handles
        .cpu
        .parse_supplemental_metadata_durable(deep_metadata.into_bytes())
        .await
        .expect_err("metadata deeper than sixteen levels must fail");
    assert!(nesting_error.detail.contains("16 levels"));

    let many_fields = format!(
        "{{{}}}",
        (0..4_097)
            .map(|index| format!(r#""field{index}":0"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    let field_error = handles
        .cpu
        .parse_supplemental_metadata_durable(many_fields.into_bytes())
        .await
        .expect_err("metadata with more than 4096 fields must fail");
    assert!(field_error.detail.contains("4096 fields"));

    let oversized_text = format!(r#"{{"description":"{}"}}"#, "x".repeat(256 * 1024 + 1));
    let text_error = handles
        .cpu
        .parse_supplemental_metadata_durable(oversized_text.into_bytes())
        .await
        .expect_err("metadata decoded text over 256 KiB must fail");
    assert!(text_error.detail.contains("decoded text"));

    let oversized_source = vec![b' '; 4 * 1024 * 1024 + 1];
    let source_error = handles
        .cpu
        .parse_supplemental_metadata_durable(oversized_source)
        .await
        .expect_err("metadata source over 4 MiB must fail");
    assert!(source_error.detail.contains("4194304"));

    let exif = handles
        .cpu
        .parse_exif_metadata_durable(
            br#"[{"Make":"Canon","ImageWidth":6000,"ImageHeight":4000,"BinaryPreview":"base64-payload"}]"#.to_vec(),
        )
        .await
        .expect("exif metadata");
    assert_eq!(exif.camera_make.as_deref(), Some("Canon"));
    assert_eq!((exif.width, exif.height), (Some(6000), Some(4000)));
    assert!(!exif.payload_json.contains("BinaryPreview"));
    assert!(!exif.payload_json.contains("base64-payload"));

    let numeric_lens_id = handles
        .cpu
        .parse_exif_metadata_durable(br#"[{"LensID":65535}]"#.to_vec())
        .await
        .expect("numeric exiftool lens identifier");
    assert_eq!(numeric_lens_id.lens_model.as_deref(), Some("65535"));
    assert!(numeric_lens_id.payload_json.contains(r#""LensID":65535"#));
    assert!(!numeric_lens_id.payload_json.contains(r#""LensID":"65535""#));

    let text_lens_id = handles
        .cpu
        .parse_exif_metadata_durable(br#"[{"LensID":"RF24-70mm"}]"#.to_vec())
        .await
        .expect("text exiftool lens identifier");
    assert_eq!(text_lens_id.lens_model.as_deref(), Some("RF24-70mm"));

    let mixed_scalar_metadata = handles
        .cpu
        .parse_exif_metadata_durable(
            br#"[{"Make":7,"Model":true,"HostComputer":"camera-host","LensModel":65535,"ISO":"6400","FNumber":"3.5","ExposureTime":"1/200","ImageWidth":"6000.0","ImageHeight":"4000","Keywords":[1,"portrait",true]}]"#.to_vec(),
        )
        .await
        .expect("heterogeneous exiftool scalar representations");
    assert_eq!(mixed_scalar_metadata.camera_make.as_deref(), Some("7"));
    assert_eq!(
        mixed_scalar_metadata.camera_model.as_deref(),
        Some("camera-host")
    );
    assert_eq!(mixed_scalar_metadata.lens_model.as_deref(), Some("65535"));
    assert_eq!(mixed_scalar_metadata.iso, Some(6400));
    assert_eq!(mixed_scalar_metadata.f_number, Some(3.5));
    assert_eq!(
        mixed_scalar_metadata.exposure_time.as_deref(),
        Some("1/200")
    );
    assert_eq!(
        (mixed_scalar_metadata.width, mixed_scalar_metadata.height),
        (Some(6000), Some(4000))
    );
    assert_eq!(
        mixed_scalar_metadata.keywords.as_deref(),
        Some("1,portrait")
    );

    let malformed_optional_fields = handles
        .cpu
        .parse_exif_metadata_durable(
            br#"[{"DateTimeOriginal":{"raw":"invalid"},"GPSLatitude":false,"ISO":"Auto","FNumber":[2.8],"ImageWidth":1.5,"ExifImageWidth":"6000","ImageHeight":{"value":4000},"SourceImageHeight":"4000","MIMEType":42,"Keywords":["portrait",{"invalid":true}],"ExposureTime":1e-320,"ShutterSpeed":"1/125"}]"#.to_vec(),
        )
        .await
        .expect("invalid optional EXIF fields must not reject otherwise usable metadata");
    assert_eq!(malformed_optional_fields.date_taken, None);
    assert_eq!(malformed_optional_fields.gps_latitude, None);
    assert_eq!(malformed_optional_fields.iso, None);
    assert_eq!(malformed_optional_fields.f_number, None);
    assert_eq!(malformed_optional_fields.width, Some(6000));
    assert_eq!(malformed_optional_fields.height, Some(4000));
    assert_eq!(malformed_optional_fields.mime_type, None);
    assert_eq!(
        malformed_optional_fields.keywords.as_deref(),
        Some("portrait")
    );
    assert_eq!(
        malformed_optional_fields.exposure_time.as_deref(),
        Some("1/125")
    );

    let offset_timestamp = handles
        .cpu
        .parse_exif_metadata_durable(
            br#"[{"DateTimeOriginal":"2024:06:30 14:15:16.125-04:00"}]"#.to_vec(),
        )
        .await
        .expect("EXIF timestamp with subsecond precision and offset");
    assert_eq!(
        offset_timestamp
            .date_taken
            .expect("normalized EXIF timestamp")
            .to_rfc3339(),
        "2024-06-30T18:15:16.125+00:00"
    );

    let ffprobe = handles
        .cpu
        .parse_ffprobe_metadata_durable(
            br#"{"streams":[{"codec_type":"audio"},{"codec_type":"video","codec_name":"hevc","width":3840,"height":2160}],"format":{"duration":"42.5","tags":{"location":"+40.7590-073.9859/"}}}"#.to_vec(),
        )
        .await
        .expect("ffprobe metadata");
    assert_eq!(ffprobe.video_codec.as_deref(), Some("hevc"));
    assert_eq!(ffprobe.duration_seconds, Some(42.5));
    assert_eq!(ffprobe.gps_latitude, Some(40.759));
}

#[tokio::test]
async fn cpu_hashing_is_bounded_and_returns_the_expected_digest() {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let pool = create_pool_at(
        &directory.path().join("database.sqlite"),
        sizing.sqlite_workers,
    )
    .expect("database pool");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config")
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool,
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .expect("executor runtime");

    let input = b"momento".to_vec();
    let expected: [u8; 32] = Sha256::digest(&input).into();
    assert_eq!(
        handles
            .cpu
            .sha256_durable(input)
            .await
            .expect("hash operation"),
        expected
    );
    let error = handles
        .cpu
        .sha256_durable(vec![0; 1024 * 1024 + 1])
        .await
        .expect_err("oversized hash input");
    assert_eq!(
        error.kind,
        momento_api::executor::ExecutorErrorKind::InvalidInput
    );
    let session = handles
        .cpu
        .start_sha256_session_request()
        .await
        .expect("start incremental hash");
    let (session, first) = handles
        .cpu
        .update_sha256_session_request(session, b"mo".to_vec())
        .await
        .expect("first incremental hash chunk");
    assert_eq!(first, b"mo");
    let (session, second) = handles
        .cpu
        .update_sha256_session_request(session, b"mento".to_vec())
        .await
        .expect("second incremental hash chunk");
    assert_eq!(second, b"mento");
    assert_eq!(
        handles
            .cpu
            .finish_sha256_session_request(session)
            .await
            .expect("finish incremental hash"),
        format!("{:x}", Sha256::digest(b"momento"))
    );

    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn sqlite_executor_atomically_prepares_journal_and_rejects_path_conflicts() {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let pool = create_pool_at(&directory.path().join("database.sqlite"), 1).expect("database pool");
    init_database(&pool.get().expect("schema connection")).expect("database schema");
    let native_maximum_pages: u64 = pool
        .get()
        .expect("native-capacity connection")
        .query_row("PRAGMA max_page_count", [], |row| row.get(0))
        .expect("native SQLite max page count");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config")
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool.clone(),
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .expect("executor runtime");

    let plan = |suffix: &str| FileOperationPlan {
        group_id: format!("group-{suffix}"),
        kind: "test_publish".to_string(),
        owner_kind: "test".to_string(),
        owner_id: format!("owner-{suffix}"),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::Originals,
            source_path: None,
            temporary_path: Some(
                NormalizedStoragePath::parse(&format!("staging/{suffix}")).expect("temporary path"),
            ),
            destination_path: Some(
                NormalizedStoragePath::parse("album/photo.jpg").expect("destination path"),
            ),
            tombstone_path: None,
            expected_size: Some(10),
            expected_sha256: Some([1; 32]),
            expected_version: None,
        }],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::Originals,
                path: NormalizedStoragePath::parse(&format!("staging/{suffix}"))
                    .expect("temporary claim path"),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "temporary".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::Originals,
                path: NormalizedStoragePath::parse("album").expect("claim path"),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Subtree,
                role: "destination".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: Some(journal_reservation(
            &handles.file_io,
            &format!("reservation-{suffix}"),
        )),
    };
    assert_eq!(
        handles
            .sqlite
            .prepare_file_operation_request(plan("one"))
            .await
            .expect("first prepare"),
        PrepareJournalOutcome::Prepared
    );
    assert_eq!(
        handles
            .sqlite
            .prepare_file_operation_request(plan("two"))
            .await
            .expect("conflicting prepare"),
        PrepareJournalOutcome::PathConflict
    );
    let group_count: i64 = pool
        .get()
        .expect("connection")
        .query_row("SELECT COUNT(*) FROM file_operation_groups", [], |row| {
            row.get(0)
        })
        .expect("group count");
    assert_eq!(group_count, 1);
    let capacity = handles
        .file_io
        .space_budget_snapshot()
        .expect("space budget after SQLite writes");
    assert_eq!(capacity.sqlite_outstanding_bytes, 0);
    assert!(capacity.sqlite_allocated_bytes > 0);
    let maximum_pages: u64 = pool
        .get()
        .expect("capacity connection")
        .query_row("PRAGMA max_page_count", [], |row| row.get(0))
        .expect("SQLite max page count");
    assert_eq!(maximum_pages, native_maximum_pages);

    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn file_executor_requires_an_exclusive_generation_checked_journal_lease() {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let pool = create_pool_at(&directory.path().join("database.sqlite"), 1).expect("database pool");
    init_database(&pool.get().expect("schema connection")).expect("database schema");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config")
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool.clone(),
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .expect("executor runtime");
    std::fs::write(directory.path().join("journal/staged.bin"), b"payload").expect("staged file");
    let stale_directory = directory.path().join("journal/stale.bin");
    std::fs::create_dir(&stale_directory).expect("stale directory");
    for index in 0..300 {
        std::fs::write(stale_directory.join(format!("entry-{index}")), b"old")
            .expect("stale tree entry");
    }

    let plan = FileOperationPlan {
        group_id: "group-one".to_string(),
        kind: "test_publish".to_string(),
        owner_kind: "test".to_string(),
        owner_id: "owner-one".to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![
            FileEntryPlan {
                action: FileEntryAction::Publish,
                storage_root: StorageRootId::Journal,
                source_path: None,
                temporary_path: Some(
                    NormalizedStoragePath::parse("staged.bin").expect("temporary path"),
                ),
                destination_path: Some(
                    NormalizedStoragePath::parse("published.bin").expect("destination path"),
                ),
                tombstone_path: None,
                expected_size: Some(7),
                expected_sha256: Some(Sha256::digest(b"payload").into()),
                expected_version: None,
            },
            FileEntryPlan {
                action: FileEntryAction::Cleanup,
                storage_root: StorageRootId::Journal,
                source_path: Some(NormalizedStoragePath::parse("stale.bin").expect("cleanup path")),
                temporary_path: None,
                destination_path: None,
                tombstone_path: None,
                expected_size: None,
                expected_sha256: None,
                expected_version: None,
            },
        ],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: NormalizedStoragePath::parse("staged.bin").expect("temporary claim path"),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Subtree,
                role: "temporary".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: NormalizedStoragePath::parse("published.bin").expect("claim path"),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "destination".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: NormalizedStoragePath::parse("stale.bin").expect("cleanup claim path"),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "cleanup".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: Some(journal_reservation(&handles.file_io, "reservation-one")),
    };
    assert_eq!(
        handles
            .sqlite
            .prepare_file_operation_request(plan)
            .await
            .expect("prepare journal"),
        PrepareJournalOutcome::Prepared
    );
    let ticket = handles
        .file_io
        .reserve_journal_mutation("group-one", 2)
        .expect("reserve first lease");
    let grant = handles
        .sqlite
        .begin_file_operation_publication_durable(&ticket, 1)
        .await
        .expect("begin publication")
        .expect("owned prepared version");

    let mut lease = ticket.acquire(grant).expect("first lease");
    assert_eq!(
        handles
            .file_io
            .publish_journal_entry_durable(&mut lease, 99)
            .await
            .expect_err("unrecorded journal entry must not be mutable")
            .kind,
        momento_api::executor::ExecutorErrorKind::InvalidInput
    );
    assert_eq!(
        handles
            .file_io
            .publish_journal_entry_durable(&mut lease, 0,)
            .await
            .expect("publish"),
        momento_api::executor::PublishJournalOutcome::Published
    );
    drop(lease);

    let recovery_ticket = handles
        .file_io
        .reserve_journal_mutation("group-one", 2)
        .expect("reserve recovery lease");
    let recovery_grant = handles
        .sqlite
        .verify_file_operation_publication_durable(&recovery_ticket)
        .await
        .expect("verify recovery publication")
        .expect("still-publishing version");
    let mut recovery_lease = recovery_ticket
        .acquire(recovery_grant)
        .expect("recovery lease");
    assert_eq!(
        handles
            .file_io
            .publish_journal_entry_durable(&mut recovery_lease, 0,)
            .await
            .expect("idempotent recovery"),
        momento_api::executor::PublishJournalOutcome::AlreadyPublished
    );

    let checkpoint = handles
        .sqlite
        .record_file_entry_published_durable("group-one".to_string(), 2, 0)
        .await
        .expect("record publication")
        .expect("owned group version");
    assert!(checkpoint.phase_complete);
    assert_eq!(checkpoint.version, 3);
    drop(recovery_lease);
    assert_eq!(
        handles
            .sqlite
            .complete_no_product_file_operation_durable("group-one".to_string(), 3)
            .await
            .expect("complete operation"),
        momento_api::io::journal::JournalCheckpointOutcome::Advanced { version: 4 }
    );
    assert_eq!(
        handles
            .file_io
            .space_budget_snapshot()
            .expect("space budget after product commit")
            .journal_outstanding_bytes,
        0
    );
    let cleanup_ticket = handles
        .file_io
        .reserve_journal_mutation("group-one", 4)
        .expect("reserve cleanup lease");
    let cleanup_grant = handles
        .sqlite
        .verify_file_operation_cleanup_durable(&cleanup_ticket)
        .await
        .expect("verify cleanup")
        .expect("cleanup-pending version");
    let mut cleanup_lease = cleanup_ticket
        .acquire(cleanup_grant)
        .expect("cleanup lease");
    assert_eq!(
        handles
            .file_io
            .cleanup_journal_entry_durable(&mut cleanup_lease, 1,)
            .await
            .expect("first bounded cleanup continuation"),
        momento_api::executor::CleanupJournalOutcome::ProgressPending
    );
    drop(cleanup_lease);
    assert!(stale_directory.exists());
    let cleanup_ticket = handles
        .file_io
        .reserve_journal_mutation("group-one", 4)
        .expect("reserve second cleanup lease");
    let cleanup_grant = handles
        .sqlite
        .verify_file_operation_cleanup_durable(&cleanup_ticket)
        .await
        .expect("verify second cleanup")
        .expect("second cleanup-pending version");
    let mut cleanup_lease = cleanup_ticket
        .acquire(cleanup_grant)
        .expect("second cleanup lease");
    assert_eq!(
        handles
            .file_io
            .cleanup_journal_entry_durable(&mut cleanup_lease, 1)
            .await
            .expect("finish bounded cleanup"),
        momento_api::executor::CleanupJournalOutcome::Removed
    );
    let cleanup_checkpoint = handles
        .sqlite
        .record_file_entry_cleaned_durable("group-one".to_string(), 4, 1)
        .await
        .expect("record cleanup")
        .expect("owned cleanup version");
    assert!(!cleanup_checkpoint.phase_complete);
    assert_eq!(cleanup_checkpoint.version, 5);
    drop(cleanup_lease);
    let publish_cleanup_ticket = handles
        .file_io
        .reserve_journal_mutation("group-one", 5)
        .expect("reserve publish cleanup lease");
    let publish_cleanup_grant = handles
        .sqlite
        .verify_file_operation_cleanup_durable(&publish_cleanup_ticket)
        .await
        .expect("verify publish temporary cleanup")
        .expect("remaining cleanup version");
    let mut publish_cleanup_lease = publish_cleanup_ticket
        .acquire(publish_cleanup_grant)
        .expect("publish cleanup lease");
    assert_eq!(
        handles
            .file_io
            .cleanup_journal_entry_durable(&mut publish_cleanup_lease, 0)
            .await
            .expect("idempotent published temporary cleanup"),
        momento_api::executor::CleanupJournalOutcome::AlreadyAbsent
    );
    let publish_cleanup_checkpoint = handles
        .sqlite
        .record_file_entry_cleaned_durable("group-one".to_string(), 5, 0)
        .await
        .expect("record publish cleanup")
        .expect("owned publish cleanup version");
    assert!(publish_cleanup_checkpoint.phase_complete);
    assert_eq!(publish_cleanup_checkpoint.version, 6);
    drop(publish_cleanup_lease);

    for (group_id, action, source_name, destination_name) in [
        (
            "move-group",
            FileEntryAction::Move,
            "move-source.bin",
            "move-destination.bin",
        ),
        (
            "tombstone-group",
            FileEntryAction::Tombstone,
            "delete-source.bin",
            "delete-tombstone.bin",
        ),
    ] {
        std::fs::write(directory.path().join("journal").join(source_name), b"move")
            .expect("rename source");
        let source_path = NormalizedStoragePath::parse(source_name).expect("source path");
        let (source_session, source_snapshot) = handles
            .file_io
            .open_storage_read_session_durable(StorageRootId::Journal, source_path.clone())
            .await
            .expect("snapshot rename source");
        handles
            .file_io
            .close_storage_session_durable(source_session)
            .await
            .expect("close rename source snapshot");
        let source_version = source_snapshot.identity_version();
        let destination_path =
            NormalizedStoragePath::parse(destination_name).expect("destination path");
        let plan = FileOperationPlan {
            group_id: group_id.to_string(),
            kind: "test_rename".to_string(),
            owner_kind: "test".to_string(),
            owner_id: format!("owner-{group_id}"),
            claim_token: None,
            product_target: None,
            product_version: None,
            entries: vec![FileEntryPlan {
                action,
                storage_root: StorageRootId::Journal,
                source_path: Some(source_path.clone()),
                temporary_path: None,
                destination_path: (action == FileEntryAction::Move)
                    .then_some(destination_path.clone()),
                tombstone_path: (action == FileEntryAction::Tombstone)
                    .then_some(destination_path.clone()),
                expected_size: Some(4),
                expected_sha256: Some(Sha256::digest(b"move").into()),
                expected_version: Some(source_version.clone()),
            }],
            claims: vec![
                FilePathClaimPlan {
                    storage_root: StorageRootId::Journal,
                    path: source_path,
                    mode: PathClaimMode::Write,
                    scope: PathClaimScope::Exact,
                    role: "source".to_string(),
                    expected_version: Some(source_version),
                },
                FilePathClaimPlan {
                    storage_root: StorageRootId::Journal,
                    path: destination_path,
                    mode: PathClaimMode::Write,
                    scope: PathClaimScope::Exact,
                    role: "destination".to_string(),
                    expected_version: None,
                },
            ],
            space_reservation: None,
        };
        assert_eq!(
            handles
                .sqlite
                .prepare_file_operation_request(plan)
                .await
                .expect("prepare rename"),
            PrepareJournalOutcome::Prepared
        );
        let ticket = handles
            .file_io
            .reserve_journal_mutation(group_id, 2)
            .expect("reserve rename lease");
        let grant = handles
            .sqlite
            .begin_file_operation_publication_durable(&ticket, 1)
            .await
            .expect("begin rename")
            .expect("prepared rename version");
        let mut lease = ticket.acquire(grant).expect("rename lease");
        assert_eq!(
            handles
                .file_io
                .rename_journal_entry_durable(&mut lease, 0)
                .await
                .expect("rename entry"),
            momento_api::executor::RenameJournalOutcome::Renamed
        );
        assert!(
            handles
                .sqlite
                .record_file_entry_published_durable(group_id.to_string(), 2, 0)
                .await
                .expect("record rename")
                .expect("rename checkpoint")
                .phase_complete
        );
        drop(lease);
        assert_eq!(
            handles
                .sqlite
                .complete_no_product_file_operation_durable(group_id.to_string(), 3)
                .await
                .expect("complete rename"),
            momento_api::io::journal::JournalCheckpointOutcome::Advanced { version: 4 }
        );
    }
    let connection = pool.get().expect("inspection connection");
    let state: String = connection
        .query_row(
            "SELECT state FROM file_operation_groups WHERE id = 'group-one'",
            [],
            |row| row.get(0),
        )
        .expect("group state");
    assert_eq!(state, "cleaned");
    let claim_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM file_operation_path_claims WHERE group_id = 'group-one'",
            [],
            |row| row.get(0),
        )
        .expect("claim count");
    assert_eq!(claim_count, 0);
    connection
        .execute(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, product_target, entry_count, version) VALUES ('product-group', 'test', 'test', 'owner-two', 'files_committed', 'media', 1, 9)",
            [],
        )
        .expect("product group");
    drop(connection);
    assert_eq!(
        handles
            .sqlite
            .complete_no_product_file_operation_durable("product-group".to_string(), 9)
            .await
            .expect("guarded generic finalizer"),
        momento_api::io::journal::JournalCheckpointOutcome::VersionConflict
    );
    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn file_executor_streams_storage_chunks_through_root_relative_operations() {
    let directory = tempfile::tempdir().expect("temporary storage directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let pool = create_pool_at(&directory.path().join("database.sqlite"), 1).expect("database pool");
    init_database(&pool.get().expect("schema connection")).expect("database schema");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config")
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool,
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .expect("executor runtime");
    let path = NormalizedStoragePath::parse("owner/device/upload.bin").expect("storage path");

    let write_session = handles
        .file_io
        .open_storage_write_session_request(StorageRootId::Backups, path.clone(), 0)
        .await
        .expect("open write session");
    let (write_session, first_written) = handles
        .file_io
        .write_storage_session_request(write_session, b"first".to_vec())
        .await
        .expect("first chunk");
    assert_eq!(first_written, 5);
    std::fs::rename(
        directory.path().join("backups/owner/device/upload.bin"),
        directory.path().join("backups/owner/device/moved.bin"),
    )
    .expect("rename open file");
    let (write_session, second_written) = handles
        .file_io
        .write_storage_session_request(write_session, b"-second".to_vec())
        .await
        .expect("second chunk");
    assert_eq!(second_written, 7);
    handles
        .file_io
        .commit_storage_session_request(write_session)
        .await
        .expect("commit write session");

    let moved_path = NormalizedStoragePath::parse("owner/device/moved.bin").expect("moved path");
    let (read_session, snapshot) = handles
        .file_io
        .open_storage_read_session_request(StorageRootId::Backups, moved_path)
        .await
        .expect("open read session");
    assert_eq!(snapshot.byte_size, 12);
    std::fs::rename(
        directory.path().join("backups/owner/device/moved.bin"),
        directory.path().join("backups/owner/device/final.bin"),
    )
    .expect("rename read session file");
    let (read_session, bytes) = handles
        .file_io
        .read_storage_session_request(read_session, 1024)
        .await
        .expect("read file");
    assert_eq!(bytes, b"first-second");
    handles
        .file_io
        .close_storage_session_request(read_session)
        .await
        .expect("close read session");

    let final_path = NormalizedStoragePath::parse("owner/device/final.bin").expect("final path");
    let abort_session = handles
        .file_io
        .open_storage_write_session_request(StorageRootId::Backups, final_path, 5)
        .await
        .expect("open rollback session");
    let (abort_session, _) = handles
        .file_io
        .write_storage_session_request(abort_session, b"discard".to_vec())
        .await
        .expect("write rollback data");
    handles
        .file_io
        .abort_storage_session_request(abort_session)
        .await
        .expect("abort write session");
    let dropped_session = handles
        .file_io
        .open_storage_write_session_request(
            StorageRootId::Backups,
            NormalizedStoragePath::parse("owner/device/final.bin").expect("drop path"),
            5,
        )
        .await
        .expect("open dropped session");
    let (dropped_session, _) = handles
        .file_io
        .write_storage_session_request(dropped_session, b"drop-me".to_vec())
        .await
        .expect("write dropped session");
    drop(dropped_session);
    handles
        .file_io
        .probe_durable(991)
        .await
        .expect("dropped-session cleanup barrier");
    assert_eq!(
        std::fs::read(directory.path().join("backups/owner/device/final.bin"))
            .expect("stored file"),
        b"first"
    );
    handles
        .file_io
        .set_storage_modified_time_durable(
            StorageRootId::Backups,
            NormalizedStoragePath::parse("owner/device/final.bin").expect("mtime path"),
            1_700_000_000,
            123_000_000,
        )
        .await
        .expect("set modified time");
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(directory.path().join("backups/owner/device/final.bin"))
        .expect("modified file metadata");
    assert_eq!(metadata.mtime(), 1_700_000_000);
    assert_eq!(metadata.mtime_nsec(), 123_000_000);

    let pending =
        NormalizedStoragePath::parse(".momento-pending/test.json").expect("atomic temporary path");
    let destination = NormalizedStoragePath::parse("owner/device/metadata.json")
        .expect("atomic destination path");
    handles
        .file_io
        .atomic_replace_storage_file_durable(
            StorageRootId::Backups,
            pending.clone(),
            destination.clone(),
            b"first-version".to_vec(),
        )
        .await
        .expect("first atomic replacement");
    handles
        .file_io
        .atomic_replace_storage_file_durable(
            StorageRootId::Backups,
            pending,
            destination,
            b"second-version".to_vec(),
        )
        .await
        .expect("second atomic replacement");
    assert_eq!(
        std::fs::read(directory.path().join("backups/owner/device/metadata.json"))
            .expect("atomically replaced file"),
        b"second-version"
    );
    assert!(!directory
        .path()
        .join("backups/.momento-pending/test.json")
        .exists());

    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn file_executor_enumerates_large_directories_with_resumable_sessions() {
    let directory = tempfile::tempdir().expect("temporary storage directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let pool = create_pool_at(&directory.path().join("database.sqlite"), 1).expect("database pool");
    init_database(&pool.get().expect("schema connection")).expect("database schema");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config")
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool,
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .expect("executor runtime");
    let source_directory = directory.path().join("imports/source");
    std::fs::create_dir(&source_directory).expect("source directory");
    for index in 0..2_000_u32 {
        std::fs::write(source_directory.join(format!("media-{index:04}.jpg")), b"x")
            .expect("source file");
    }

    let mut session = Some(
        handles
            .file_io
            .open_storage_directory_session_durable(
                StorageRootId::Imports,
                Some(NormalizedStoragePath::parse("source").expect("source path")),
            )
            .await
            .expect("open directory session"),
    );
    let mut chunks = 0;
    let mut names = Vec::new();
    loop {
        let (returned, entries, finished) = handles
            .file_io
            .read_storage_directory_session_durable(session.take().expect("directory session"))
            .await
            .expect("read directory chunk");
        session = Some(returned);
        chunks += 1;
        names.extend(entries.into_iter().map(|entry| entry.name));
        if finished {
            break;
        }
    }
    assert!(chunks > 1);
    names.sort();
    assert_eq!(names.len(), 2_000);
    assert_eq!(names.first().map(String::as_str), Some("media-0000.jpg"));
    assert_eq!(names.last().map(String::as_str), Some("media-1999.jpg"));
    handles
        .file_io
        .close_storage_session_durable(session.expect("directory session"))
        .await
        .expect("close directory session");
    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn journal_move_rejects_a_replaced_source_generation() {
    let directory = tempfile::tempdir().expect("temporary storage directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let pool = create_pool_at(&directory.path().join("database.sqlite"), 1).expect("database pool");
    init_database(&pool.get().expect("schema connection")).expect("database schema");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config")
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool,
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .expect("executor runtime");
    let source = NormalizedStoragePath::parse("generation-source.bin").expect("source path");
    let destination =
        NormalizedStoragePath::parse("generation-destination.bin").expect("destination path");
    std::fs::write(
        directory.path().join("journal/generation-source.bin"),
        b"one",
    )
    .expect("source");
    let (source_session, snapshot) = handles
        .file_io
        .open_storage_read_session_durable(StorageRootId::Journal, source.clone())
        .await
        .expect("source snapshot");
    handles
        .file_io
        .close_storage_session_durable(source_session)
        .await
        .expect("close source snapshot");
    let version = snapshot.identity_version();
    let plan = FileOperationPlan {
        group_id: "generation-fence".to_string(),
        kind: "generation_test".to_string(),
        owner_kind: "test".to_string(),
        owner_id: "generation-owner".to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Move,
            storage_root: StorageRootId::Journal,
            source_path: Some(source.clone()),
            temporary_path: None,
            destination_path: Some(destination.clone()),
            tombstone_path: None,
            expected_size: Some(3),
            expected_sha256: None,
            expected_version: Some(version.clone()),
        }],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: source,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "source".to_string(),
                expected_version: Some(version),
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: destination,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "destination".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: None,
    };
    assert_eq!(
        handles
            .sqlite
            .prepare_file_operation_request(plan)
            .await
            .expect("prepare move"),
        PrepareJournalOutcome::Prepared
    );
    std::fs::remove_file(directory.path().join("journal/generation-source.bin"))
        .expect("remove original generation");
    std::fs::write(
        directory.path().join("journal/generation-source.bin"),
        b"two",
    )
    .expect("replacement generation");
    let ticket = handles
        .file_io
        .reserve_journal_mutation("generation-fence", 2)
        .expect("mutation ticket");
    let grant = handles
        .sqlite
        .begin_file_operation_publication_durable(&ticket, 1)
        .await
        .expect("begin publication")
        .expect("publication grant");
    let mut lease = ticket.acquire(grant).expect("mutation lease");
    let error = handles
        .file_io
        .rename_journal_entry_durable(&mut lease, 0)
        .await
        .expect_err("replacement must be rejected");
    assert_eq!(
        error.kind,
        momento_api::executor::ExecutorErrorKind::FileInvalidData
    );
    assert!(directory
        .path()
        .join("journal/generation-source.bin")
        .is_file());
    assert!(!directory
        .path()
        .join("journal/generation-destination.bin")
        .exists());
    drop(lease);
    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn generic_journal_recovery_finishes_a_rename_not_checkpointed_before_restart() {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let pool = create_pool_at(&directory.path().join("database.sqlite"), 1).expect("database pool");
    init_database(&pool.get().expect("schema connection")).expect("database schema");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config")
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool.clone(),
        identity.clone(),
        directory.path().to_path_buf(),
        None,
    )
    .expect("first executor runtime");
    std::fs::write(directory.path().join("journal/restart.tmp"), b"restart").expect("staged file");
    let plan = FileOperationPlan {
        group_id: "restart-group".to_string(),
        kind: "restart_test".to_string(),
        owner_kind: "test".to_string(),
        owner_id: "restart-owner".to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::Journal,
            source_path: None,
            temporary_path: Some(
                NormalizedStoragePath::parse("restart.tmp").expect("temporary path"),
            ),
            destination_path: Some(
                NormalizedStoragePath::parse("restart.bin").expect("destination path"),
            ),
            tombstone_path: None,
            expected_size: Some(7),
            expected_sha256: Some(Sha256::digest(b"restart").into()),
            expected_version: None,
        }],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: NormalizedStoragePath::parse("restart.tmp").expect("temporary claim"),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "temporary".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: NormalizedStoragePath::parse("restart.bin").expect("destination claim"),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "destination".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: Some(journal_reservation(&handles.file_io, "restart-reservation")),
    };
    assert_eq!(
        handles
            .sqlite
            .prepare_file_operation_request(plan)
            .await
            .expect("prepare restart group"),
        PrepareJournalOutcome::Prepared
    );
    let ticket = handles
        .file_io
        .reserve_journal_mutation("restart-group", 2)
        .expect("reserve publication lease");
    let grant = handles
        .sqlite
        .begin_file_operation_publication_durable(&ticket, 1)
        .await
        .expect("begin publication")
        .expect("prepared version");
    let mut lease = ticket.acquire(grant).expect("publication lease");
    assert_eq!(
        handles
            .file_io
            .publish_journal_entry_durable(&mut lease, 0)
            .await
            .expect("publish before crash"),
        momento_api::executor::PublishJournalOutcome::Published
    );
    drop(lease);
    drop(handles);
    runtime.shutdown().await.expect("first runtime shutdown");

    let (restarted_runtime, restarted_handles) = ExecutorRuntime::start(
        &sizing,
        pool.clone(),
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .expect("restarted executor runtime");
    assert_eq!(
        momento_api::io::recovery::recover_generic_file_operations(&restarted_handles)
            .await
            .expect("journal recovery"),
        2
    );
    let state: String = pool
        .get()
        .expect("inspection connection")
        .query_row(
            "SELECT state FROM file_operation_groups WHERE id = 'restart-group'",
            [],
            |row| row.get(0),
        )
        .expect("recovered state");
    assert_eq!(state, "cleaned");
    assert_eq!(
        std::fs::read(directory.path().join("journal/restart.bin")).expect("published bytes"),
        b"restart"
    );
    let missing_plan = FileOperationPlan {
        group_id: "missing-group".to_string(),
        kind: "restart_test".to_string(),
        owner_kind: "test".to_string(),
        owner_id: "missing-owner".to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::Journal,
            source_path: None,
            temporary_path: Some(
                NormalizedStoragePath::parse("missing.tmp").expect("missing temporary path"),
            ),
            destination_path: Some(
                NormalizedStoragePath::parse("missing.bin").expect("missing destination path"),
            ),
            tombstone_path: None,
            expected_size: Some(7),
            expected_sha256: Some(Sha256::digest(b"repair!").into()),
            expected_version: None,
        }],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: NormalizedStoragePath::parse("missing.tmp").expect("temporary claim"),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "temporary".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: NormalizedStoragePath::parse("missing.bin").expect("destination claim"),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "destination".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: Some(journal_reservation(
            &restarted_handles.file_io,
            "missing-reservation",
        )),
    };
    assert_eq!(
        restarted_handles
            .sqlite
            .prepare_file_operation_request(missing_plan)
            .await
            .expect("prepare missing group"),
        PrepareJournalOutcome::Prepared
    );
    let missing_ticket = restarted_handles
        .file_io
        .reserve_journal_mutation("missing-group", 2)
        .expect("reserve missing publication");
    assert!(restarted_handles
        .sqlite
        .begin_file_operation_publication_durable(&missing_ticket, 1)
        .await
        .expect("begin missing publication")
        .is_some());
    drop(missing_ticket);
    assert_eq!(
        momento_api::io::recovery::recover_generic_file_operations(&restarted_handles)
            .await
            .expect("classify missing source"),
        0
    );
    let inspection = pool.get().expect("failure inspection connection");
    let failed_state: String = inspection
        .query_row(
            "SELECT state FROM file_operation_groups WHERE id = 'missing-group'",
            [],
            |row| row.get(0),
        )
        .expect("failed state");
    assert_eq!(failed_state, "publication_failed");
    let retained_claims: i64 = inspection
        .query_row(
            "SELECT COUNT(*) FROM file_operation_path_claims WHERE group_id = 'missing-group'",
            [],
            |row| row.get(0),
        )
        .expect("retained claims");
    assert_eq!(retained_claims, 2);
    drop(inspection);
    std::fs::write(directory.path().join("journal/missing.tmp"), b"repair!")
        .expect("repair missing source");
    let retry_hash = Sha256::digest(b"retry-request").into();
    assert_eq!(
        restarted_handles
            .sqlite
            .retry_file_operation_request(
                "retry-one".to_string(),
                "missing-group".to_string(),
                3,
                retry_hash,
            )
            .await
            .expect("retry failed publication"),
        momento_api::io::journal::JournalRetryOutcome::Accepted {
            state: "publishing".to_string(),
            version: 4,
            replayed: false,
        }
    );
    assert_eq!(
        restarted_handles
            .sqlite
            .retry_file_operation_request(
                "retry-one".to_string(),
                "missing-group".to_string(),
                3,
                retry_hash,
            )
            .await
            .expect("replay retry receipt"),
        momento_api::io::journal::JournalRetryOutcome::Accepted {
            state: "publishing".to_string(),
            version: 4,
            replayed: true,
        }
    );
    assert_eq!(
        restarted_handles
            .sqlite
            .retry_file_operation_request(
                "retry-one".to_string(),
                "missing-group".to_string(),
                3,
                [2; 32],
            )
            .await
            .expect("reject changed retry receipt"),
        momento_api::io::journal::JournalRetryOutcome::RequestConflict
    );
    assert_eq!(
        momento_api::io::recovery::recover_generic_file_operations(&restarted_handles)
            .await
            .expect("recover repaired group"),
        2
    );
    drop(restarted_handles);
    restarted_runtime
        .shutdown()
        .await
        .expect("restarted runtime shutdown");
}

#[tokio::test]
async fn prepared_journal_cancellation_rolls_back_temporaries_and_keeps_original_sources() {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let pool = create_pool_at(&directory.path().join("database.sqlite"), 1).expect("database pool");
    init_database(&pool.get().expect("schema connection")).expect("database schema");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config")
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool.clone(),
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .expect("executor runtime");
    std::fs::write(directory.path().join("journal/cancel.tmp"), b"cancel").expect("temporary");
    std::fs::write(directory.path().join("journal/original.bin"), b"keep").expect("source");
    let temporary = NormalizedStoragePath::parse("cancel.tmp").expect("temporary path");
    let destination = NormalizedStoragePath::parse("cancel.bin").expect("destination path");
    let source = NormalizedStoragePath::parse("original.bin").expect("source path");
    let moved = NormalizedStoragePath::parse("moved.bin").expect("moved path");
    let plan = FileOperationPlan {
        group_id: "cancel-prepared".to_string(),
        kind: "cancel_test".to_string(),
        owner_kind: "test".to_string(),
        owner_id: "cancel-owner".to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![
            FileEntryPlan {
                action: FileEntryAction::Publish,
                storage_root: StorageRootId::Journal,
                source_path: None,
                temporary_path: Some(temporary.clone()),
                destination_path: Some(destination.clone()),
                tombstone_path: None,
                expected_size: Some(6),
                expected_sha256: Some(Sha256::digest(b"cancel").into()),
                expected_version: None,
            },
            FileEntryPlan {
                action: FileEntryAction::Move,
                storage_root: StorageRootId::Journal,
                source_path: Some(source.clone()),
                temporary_path: None,
                destination_path: Some(moved.clone()),
                tombstone_path: None,
                expected_size: Some(4),
                expected_sha256: Some(Sha256::digest(b"keep").into()),
                expected_version: None,
            },
        ],
        claims: vec![
            (temporary, "temporary"),
            (destination, "destination"),
            (source, "source"),
            (moved, "destination"),
        ]
        .into_iter()
        .map(|(path, role)| FilePathClaimPlan {
            storage_root: StorageRootId::Journal,
            path,
            mode: PathClaimMode::Write,
            scope: PathClaimScope::Exact,
            role: role.to_string(),
            expected_version: None,
        })
        .collect(),
        space_reservation: Some(journal_reservation(&handles.file_io, "cancel-reservation")),
    };
    assert_eq!(
        handles
            .sqlite
            .prepare_file_operation_request(plan)
            .await
            .expect("prepare cancellation group"),
        PrepareJournalOutcome::Prepared
    );
    assert_eq!(
        momento_api::io::recovery::cancel_generic_file_operation(
            &handles,
            "cancel-prepared".to_string(),
            1,
        )
        .await
        .expect("request cancellation"),
        momento_api::io::journal::JournalCancellationOutcome::Requested {
            state: "rollback_pending".to_string(),
            version: 2,
        }
    );
    assert_eq!(
        momento_api::io::recovery::cancel_generic_file_operation(
            &handles,
            "cancel-prepared".to_string(),
            2,
        )
        .await
        .expect("repeat cancellation"),
        momento_api::io::journal::JournalCancellationOutcome::AlreadyRequested {
            state: "rollback_pending".to_string(),
            version: 2,
        }
    );
    std::fs::set_permissions(
        directory.path().join("journal"),
        std::fs::Permissions::from_mode(0o555),
    )
    .expect("make rollback temporarily unwritable");
    assert_eq!(
        momento_api::io::recovery::recover_generic_file_operations(&handles)
            .await
            .expect("defer failed rollback"),
        0
    );
    let (rollback_state, rollback_error): (String, Option<String>) = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT state, rollback_error FROM file_operation_groups WHERE id = 'cancel-prepared'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("rollback failure state");
    assert_eq!(rollback_state, "rollback_pending");
    assert!(rollback_error.is_some());
    std::fs::set_permissions(
        directory.path().join("journal"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("repair rollback directory");
    assert_eq!(
        momento_api::io::recovery::recover_generic_file_operations(&handles)
            .await
            .expect("rollback recovery"),
        1
    );
    assert!(!directory.path().join("journal/cancel.tmp").exists());
    assert!(!directory.path().join("journal/cancel.bin").exists());
    assert_eq!(
        std::fs::read(directory.path().join("journal/original.bin")).expect("original source"),
        b"keep"
    );
    assert!(!directory.path().join("journal/moved.bin").exists());
    let connection = pool.get().expect("inspection connection");
    let (state, claims, reservation): (String, i64, String) = connection
        .query_row(
            "SELECT g.state, (SELECT COUNT(*) FROM file_operation_path_claims WHERE group_id = g.id), (SELECT state FROM data_dir_space_reservations WHERE journal_group_id = g.id) FROM file_operation_groups AS g WHERE g.id = 'cancel-prepared'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("rollback state");
    assert_eq!(state, "rolled_back");
    assert_eq!(claims, 0);
    assert_eq!(reservation, "released");
    drop(connection);

    std::fs::write(directory.path().join("journal/forward.tmp"), b"forward")
        .expect("forward temporary");
    let forward_temporary =
        NormalizedStoragePath::parse("forward.tmp").expect("forward temporary path");
    let forward_destination =
        NormalizedStoragePath::parse("forward.bin").expect("forward destination path");
    let forward_plan = FileOperationPlan {
        group_id: "cancel-forward".to_string(),
        kind: "cancel_test".to_string(),
        owner_kind: "test".to_string(),
        owner_id: "forward-owner".to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::Journal,
            source_path: None,
            temporary_path: Some(forward_temporary.clone()),
            destination_path: Some(forward_destination.clone()),
            tombstone_path: None,
            expected_size: Some(7),
            expected_sha256: Some(Sha256::digest(b"forward").into()),
            expected_version: None,
        }],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: forward_temporary,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "temporary".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: forward_destination,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "destination".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: Some(journal_reservation(&handles.file_io, "forward-reservation")),
    };
    assert_eq!(
        handles
            .sqlite
            .prepare_file_operation_request(forward_plan)
            .await
            .expect("prepare forward group"),
        PrepareJournalOutcome::Prepared
    );
    let ticket = handles
        .file_io
        .reserve_journal_mutation("cancel-forward", 2)
        .expect("reserve forward lease");
    let grant = handles
        .sqlite
        .begin_file_operation_publication_durable(&ticket, 1)
        .await
        .expect("begin forward publication")
        .expect("forward grant");
    let mut lease = ticket.acquire(grant).expect("forward lease");
    handles
        .file_io
        .apply_next_journal_entry_durable(&mut lease)
        .await
        .expect("publish forward entry");
    handles
        .sqlite
        .record_file_entry_published_durable("cancel-forward".to_string(), 2, 0)
        .await
        .expect("checkpoint forward publication")
        .expect("forward checkpoint");
    drop(lease);
    assert_eq!(
        momento_api::io::recovery::cancel_generic_file_operation(
            &handles,
            "cancel-forward".to_string(),
            3,
        )
        .await
        .expect("request forward cancellation"),
        momento_api::io::journal::JournalCancellationOutcome::Requested {
            state: "files_committed".to_string(),
            version: 4,
        }
    );
    assert_eq!(
        handles
            .sqlite
            .complete_no_product_file_operation_durable("cancel-forward".to_string(), 4)
            .await
            .expect("finalize discarded forward group"),
        momento_api::io::journal::JournalCheckpointOutcome::Advanced { version: 5 }
    );
    assert!(directory.path().join("journal/forward.bin").exists());
    assert_eq!(
        momento_api::io::recovery::recover_generic_file_operations(&handles)
            .await
            .expect("discard cleanup"),
        1
    );
    assert!(!directory.path().join("journal/forward.bin").exists());
    let (state, outcome): (String, String) = pool
        .get()
        .expect("inspection connection")
        .query_row(
            "SELECT state, completion_outcome FROM file_operation_groups WHERE id = 'cancel-forward'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("discarded state");
    assert_eq!(state, "cleaned");
    assert_eq!(outcome, "discarded");

    std::fs::write(directory.path().join("journal/fenced.tmp"), b"fenced")
        .expect("fenced temporary");
    let fenced_temporary =
        NormalizedStoragePath::parse("fenced.tmp").expect("fenced temporary path");
    let fenced_destination =
        NormalizedStoragePath::parse("fenced.bin").expect("fenced destination path");
    let fenced_plan = FileOperationPlan {
        group_id: "cancel-fenced".to_string(),
        kind: "cancel_test".to_string(),
        owner_kind: "test".to_string(),
        owner_id: "fenced-owner".to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::Journal,
            source_path: None,
            temporary_path: Some(fenced_temporary.clone()),
            destination_path: Some(fenced_destination.clone()),
            tombstone_path: None,
            expected_size: Some(6),
            expected_sha256: Some(Sha256::digest(b"fenced").into()),
            expected_version: None,
        }],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: fenced_temporary,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "temporary".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::Journal,
                path: fenced_destination,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Exact,
                role: "destination".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: Some(journal_reservation(&handles.file_io, "fenced-reservation")),
    };
    assert_eq!(
        handles
            .sqlite
            .prepare_file_operation_request(fenced_plan)
            .await
            .expect("prepare fenced group"),
        PrepareJournalOutcome::Prepared
    );
    let stale_ticket = handles
        .file_io
        .reserve_journal_mutation("cancel-fenced", 2)
        .expect("reserve stale lease");
    let stale_grant = handles
        .sqlite
        .begin_file_operation_publication_durable(&stale_ticket, 1)
        .await
        .expect("begin fenced publication")
        .expect("stale grant");
    let delayed_ticket = handles
        .file_io
        .reserve_journal_mutation("cancel-fenced", 2)
        .expect("reserve delayed pre-cancellation ticket");
    let delayed_grant = handles
        .sqlite
        .verify_file_operation_publication_durable(&delayed_ticket)
        .await
        .expect("verify delayed pre-cancellation grant")
        .expect("delayed grant");
    let mut stale_lease = stale_ticket.acquire(stale_grant).expect("stale lease");
    assert_eq!(
        momento_api::io::recovery::cancel_generic_file_operation(
            &handles,
            "cancel-fenced".to_string(),
            2,
        )
        .await
        .expect("fence publication"),
        momento_api::io::journal::JournalCancellationOutcome::Requested {
            state: "publishing".to_string(),
            version: 3,
        }
    );
    assert_eq!(
        handles
            .file_io
            .apply_next_journal_entry_durable(&mut stale_lease)
            .await
            .expect_err("stale generation must not mutate")
            .kind,
        momento_api::executor::ExecutorErrorKind::InvalidInput
    );
    drop(stale_lease);
    assert!(!directory.path().join("journal/fenced.bin").exists());
    assert_eq!(
        momento_api::io::recovery::recover_generic_file_operations(&handles)
            .await
            .expect("recover fenced discard"),
        2
    );
    assert!(!directory.path().join("journal/fenced.tmp").exists());
    assert!(!directory.path().join("journal/fenced.bin").exists());
    assert_eq!(
        delayed_ticket
            .acquire(delayed_grant)
            .expect_err("a delayed grant must remain fenced after terminal retirement"),
        momento_api::io::file::MutationLeaseError::Fenced
    );

    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn durable_cancellations_release_mutation_fences_before_rollback_finishes() {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let pool = create_pool_at(&directory.path().join("database.sqlite"), 1).expect("database pool");
    init_database(&pool.get().expect("schema connection")).expect("database schema");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config")
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool,
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .expect("executor runtime");

    for index in 0..=sizing.journal_mutation_registry_capacity {
        let group_id = format!("cancel-capacity-{index}");
        let temporary =
            NormalizedStoragePath::parse(&format!("capacity-{index}.tmp")).expect("temporary path");
        let destination = NormalizedStoragePath::parse(&format!("capacity-{index}.bin"))
            .expect("destination path");
        let plan = FileOperationPlan {
            group_id: group_id.clone(),
            kind: "cancel_capacity_test".to_string(),
            owner_kind: "test".to_string(),
            owner_id: group_id.clone(),
            claim_token: None,
            product_target: None,
            product_version: None,
            entries: vec![FileEntryPlan {
                action: FileEntryAction::Publish,
                storage_root: StorageRootId::Journal,
                source_path: None,
                temporary_path: Some(temporary.clone()),
                destination_path: Some(destination.clone()),
                tombstone_path: None,
                expected_size: Some(1),
                expected_sha256: Some(Sha256::digest([index as u8]).into()),
                expected_version: None,
            }],
            claims: vec![
                FilePathClaimPlan {
                    storage_root: StorageRootId::Journal,
                    path: temporary,
                    mode: PathClaimMode::Write,
                    scope: PathClaimScope::Exact,
                    role: "temporary".to_string(),
                    expected_version: None,
                },
                FilePathClaimPlan {
                    storage_root: StorageRootId::Journal,
                    path: destination,
                    mode: PathClaimMode::Write,
                    scope: PathClaimScope::Exact,
                    role: "destination".to_string(),
                    expected_version: None,
                },
            ],
            space_reservation: Some(journal_reservation(
                &handles.file_io,
                &format!("cancel-capacity-reservation-{index}"),
            )),
        };
        assert_eq!(
            handles
                .sqlite
                .prepare_file_operation_request(plan)
                .await
                .expect("prepare cancellation group"),
            PrepareJournalOutcome::Prepared
        );
        assert_eq!(
            momento_api::io::recovery::cancel_generic_file_operation(&handles, group_id, 1)
                .await
                .expect("request cancellation"),
            momento_api::io::journal::JournalCancellationOutcome::Requested {
                state: "rollback_pending".to_string(),
                version: 2,
            }
        );
    }

    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn backup_cancellation_commits_product_state_and_journal_cleanup_atomically() {
    let directory = tempfile::tempdir().expect("temporary executor directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let pool = create_pool_at(
        &directory.path().join("database.sqlite"),
        sizing.sqlite_workers,
    )
    .expect("database pool");
    init_database(&pool.get().expect("schema connection")).expect("database schema");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config")
        .identity;
    let (runtime, handles) = ExecutorRuntime::start(
        &sizing,
        pool.clone(),
        identity,
        directory.path().to_path_buf(),
        None,
    )
    .expect("executor runtime");

    let connection = pool.get().expect("fixture connection");
    connection
        .execute_batch(
            "INSERT INTO users (id, username, email, hashed_password, role, must_change_password, is_active)
             VALUES (1, 'backup-owner', 'backup-owner@example.com', 'hash', 'user', 0, 1);
             INSERT INTO backup_devices (user_id, device_id, device_name)
             VALUES (1, 'device', 'Device');
             INSERT INTO backup_assets (id, user_id, device_id, client_asset_id, operation_id, original_filename, mime_type, byte_size, source_modified_at, status, staged_path)
             VALUES (1, 1, 'device', 'asset', 'operation', 'photo.jpg', 'image/jpeg', 7, '2024-01-02T03:04:05Z', 'uploading', 'owner/upload.partial');
             INSERT INTO backup_upload_sessions (upload_id, asset_id, user_id, expected_size, uploaded_size, status, expires_at)
             VALUES ('upload', 1, 1, 7, 7, 'uploading', datetime('now', '+1 hour'));",
        )
        .expect("backup cancellation fixture");
    drop(connection);
    let staged_path = directory.path().join("backups/owner/upload.partial");
    std::fs::create_dir_all(staged_path.parent().expect("staging parent"))
        .expect("create staging parent");
    std::fs::write(&staged_path, b"payload").expect("write staged upload");

    let outcome = handles
        .sqlite
        .cancel_backup_upload_request(momento_api::database::operations::CancelBackupUpload {
            user_id: 1,
            upload_id: "upload".to_string(),
        })
        .await
        .expect("cancel backup upload");
    assert!(matches!(
        outcome,
        momento_api::database::operations::CancelBackupUploadOutcome::Cancelled(_)
    ));
    let (asset_state, session_state, journal_state, claims): (String, String, String, i64) = pool
        .get()
        .expect("inspection connection")
        .query_row(
            "SELECT backup_assets.status,
                    backup_upload_sessions.status,
                    file_operation_groups.state,
                    (SELECT COUNT(*) FROM file_operation_path_claims WHERE group_id = file_operation_groups.id)
               FROM backup_assets
               JOIN backup_upload_sessions ON backup_upload_sessions.asset_id = backup_assets.id
               JOIN file_operation_groups ON file_operation_groups.owner_kind = 'backup_asset'
                                         AND file_operation_groups.owner_id = CAST(backup_assets.id AS TEXT)
              WHERE backup_assets.id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("atomic cancellation state");
    assert_eq!(asset_state, "cancelled");
    assert_eq!(session_state, "cancelled");
    assert_eq!(journal_state, "cleanup_pending");
    assert_eq!(claims, 1);
    assert!(staged_path.exists());

    assert_eq!(
        momento_api::io::recovery::recover_generic_file_operations(&handles)
            .await
            .expect("recover backup cleanup"),
        1
    );
    assert!(!staged_path.exists());
    let (journal_state, claims): (String, i64) = pool
        .get()
        .expect("terminal inspection connection")
        .query_row(
            "SELECT state,
                    (SELECT COUNT(*) FROM file_operation_path_claims WHERE group_id = file_operation_groups.id)
               FROM file_operation_groups
              WHERE owner_kind = 'backup_asset' AND owner_id = '1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("terminal cleanup state");
    assert_eq!(journal_state, "cleaned");
    assert_eq!(claims, 0);

    drop(handles);
    runtime.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn permanent_finalization_failure_stops_until_an_administrator_retry() {
    let pool = crate::test_utils::create_test_db();
    let handles = crate::test_utils::test_executor_handles(pool.clone());
    let connection = pool.get().expect("database");
    connection
        .execute_batch(
            "INSERT INTO file_operation_groups (id, kind, owner_kind, owner_id, state, entry_count, version) VALUES
                ('finalize-failure', 'test', 'test', 'owner', 'files_committed', 1, 7);
             INSERT INTO file_operation_entries (group_id, sequence, action, storage_root, source_path, destination_path, state, cleanup_state) VALUES
                ('finalize-failure', 0, 'move', 'journal', 'old', 'new', 'committed', 'cleaned');
             CREATE TRIGGER fail_test_finalization
             BEFORE UPDATE OF state ON file_operation_groups
             WHEN OLD.id = 'finalize-failure' AND NEW.state IN ('completed', 'cleanup_pending')
             BEGIN
                 SELECT RAISE(ABORT, 'test permanent finalization failure');
             END;",
        )
        .expect("finalization failure fixture");
    drop(connection);

    assert_eq!(
        momento_api::io::recovery::recover_generic_file_operations(&handles)
            .await
            .expect("classify finalization failure"),
        0
    );
    let connection = pool.get().expect("database");
    let (state, version, error_kind): (String, i64, String) = connection
        .query_row(
            "SELECT state, version, finalization_error_kind FROM file_operation_groups WHERE id = 'finalize-failure'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("failed finalization state");
    assert_eq!(state, "finalize_failed");
    assert_eq!(version, 8);
    assert_eq!(error_kind, "DatabasePermanent");
    connection
        .execute("DROP TRIGGER fail_test_finalization", [])
        .expect("repair finalization");
    drop(connection);

    assert_eq!(
        handles
            .sqlite
            .retry_file_operation_request(
                "retry-finalize".to_string(),
                "finalize-failure".to_string(),
                8,
                Sha256::digest(b"retry-finalize").into(),
            )
            .await
            .expect("retry finalization"),
        momento_api::io::journal::JournalRetryOutcome::Accepted {
            state: "files_committed".to_string(),
            version: 9,
            replayed: false,
        }
    );
    assert_eq!(
        momento_api::io::recovery::recover_generic_file_operations(&handles)
            .await
            .expect("finish repaired finalization"),
        0
    );
    let state: String = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT state FROM file_operation_groups WHERE id = 'finalize-failure'",
            [],
            |row| row.get(0),
        )
        .expect("completed state");
    assert_eq!(state, "completed");
}
mod process;
