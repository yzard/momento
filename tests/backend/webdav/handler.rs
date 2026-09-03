use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
};
use momento_api::io::file::{NormalizedStoragePath, PathClaimMode, PathClaimScope, StorageRootId};
use momento_api::io::journal::{
    DirectoryCopyConstructionPlan, FileEntryAction, FileEntryPlan, FileOperationPlan,
    FilePathClaimPlan, JournalSpaceReservationPlan,
};
use momento_api::webdav::handler::{
    completed_upload_path, contains_reserved_destination, contains_reserved_path,
    guard_response_body, handle_webdav_request, invalidated_upload_paths, request_mutates_staging,
    resume_prepared_directory_copies_after_restart, validate_upload_size,
};
use std::sync::Arc;

async fn request(
    executors: &momento_api::runtime::ExecutorHandles,
    username: &str,
    maximum_upload_bytes: u64,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: &'static [u8],
) -> (StatusCode, String) {
    request_with_body(
        executors,
        username,
        maximum_upload_bytes,
        method,
        uri,
        headers,
        Body::from(body),
    )
    .await
}

async fn request_with_body(
    executors: &momento_api::runtime::ExecutorHandles,
    username: &str,
    maximum_upload_bytes: u64,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: Body,
) -> (StatusCode, String) {
    let admission = momento_api::runtime::HttpRequestAdmission::acquire(&executors.scheduler)
        .expect("request admission");
    let parsed_method = Method::from_bytes(method.as_bytes()).expect("method");
    if (request_mutates_staging(&parsed_method) || method == "PROPFIND")
        && admission.convert_to_stream().is_err()
    {
        panic!("stream admission");
    }
    let mut builder = Request::builder().method(parsed_method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let response = handle_webdav_request(
        executors,
        username,
        &admission,
        builder.body(body).expect("request"),
        "/webdav",
        maximum_upload_bytes,
    )
    .await;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn storage_path(value: &str) -> NormalizedStoragePath {
    NormalizedStoragePath::parse(value).expect("normalized storage path")
}

#[tokio::test]
async fn startup_resumes_a_prepared_directory_copy_before_generic_rollback() {
    let pool = crate::test_utils::create_test_db();
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    let source_root = storage_path("restart-user/source");
    let temporary_root = storage_path("restart-user/.copy.tmp");
    let destination_root = storage_path("restart-user/copied");
    let source_file = storage_path("restart-user/source/photo.bin");
    let source_directory = data_directory.join("webdav/restart-user/source");
    std::fs::create_dir_all(&source_directory).expect("source directory");
    std::fs::write(source_directory.join("photo.bin"), b"resumable-copy").expect("source file");
    let (session, snapshot) = executors
        .file_io
        .open_storage_read_session_durable(StorageRootId::WebDav, source_file.clone())
        .await
        .expect("source snapshot");
    executors
        .file_io
        .close_storage_session_durable(session)
        .await
        .expect("close source snapshot");
    let mut fingerprint_record = vec![b'f'];
    fingerprint_record.extend_from_slice(source_file.relative_path().as_bytes());
    fingerprint_record.push(0);
    fingerprint_record.extend_from_slice(snapshot.identity_version().as_bytes());
    let fingerprint = executors
        .cpu
        .sha256_durable(fingerprint_record)
        .await
        .expect("source fingerprint");
    let group_id = "webdav-copy-restart-test".to_string();
    let reservation = executors
        .file_io
        .reserve_journal_space(group_id.clone(), 8192)
        .expect("journal reservation")
        .into_result()
        .expect("journal capacity");
    let plan = FileOperationPlan {
        group_id: group_id.clone(),
        kind: "webdav_directory_copy".to_string(),
        owner_kind: "webdav".to_string(),
        owner_id: "restart-test".to_string(),
        claim_token: None,
        product_target: None,
        product_version: None,
        entries: vec![FileEntryPlan {
            action: FileEntryAction::Publish,
            storage_root: StorageRootId::WebDav,
            source_path: None,
            temporary_path: Some(temporary_root.clone()),
            destination_path: Some(destination_root.clone()),
            tombstone_path: None,
            expected_size: None,
            expected_sha256: None,
            expected_version: None,
        }],
        claims: vec![
            FilePathClaimPlan {
                storage_root: StorageRootId::WebDav,
                path: source_root.clone(),
                mode: PathClaimMode::Read,
                scope: PathClaimScope::Subtree,
                role: "copy_source".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::WebDav,
                path: temporary_root.clone(),
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Subtree,
                role: "copy_temporary".to_string(),
                expected_version: None,
            },
            FilePathClaimPlan {
                storage_root: StorageRootId::WebDav,
                path: destination_root,
                mode: PathClaimMode::Write,
                scope: PathClaimScope::Subtree,
                role: "copy_destination".to_string(),
                expected_version: None,
            },
        ],
        space_reservation: Some(
            JournalSpaceReservationPlan::new(reservation).expect("reservation plan"),
        ),
    };
    executors
        .sqlite
        .prepare_directory_copy_operation_durable(
            plan,
            DirectoryCopyConstructionPlan {
                storage_root: StorageRootId::WebDav,
                source_root,
                temporary_root,
                expected_file_bytes: snapshot.byte_size,
                expected_entry_count: 1,
                expected_fingerprint: fingerprint,
            },
        )
        .await
        .expect("prepared interrupted copy");

    assert_eq!(
        resume_prepared_directory_copies_after_restart(&executors)
            .await
            .expect("resume prepared copy"),
        1
    );
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .expect("finish resumed copy cleanup");
    assert_eq!(
        std::fs::read(data_directory.join("webdav/restart-user/copied/photo.bin"))
            .expect("published copy"),
        b"resumable-copy"
    );
    let state: String = pool
        .get()
        .expect("database")
        .query_row(
            "SELECT state FROM file_operation_groups WHERE id = ?",
            [&group_id],
            |row| row.get(0),
        )
        .expect("operation state");
    assert_eq!(state, "cleaned");
}

#[test]
fn test_upload_size_validation_accepts_undeclared_put_and_bounds_declared_sizes() {
    let mut headers = HeaderMap::new();
    assert_eq!(validate_upload_size(&Method::PUT, &headers, 11), Ok(()));
    assert_eq!(
        validate_upload_size(&Method::PATCH, &headers, 11),
        Err(StatusCode::LENGTH_REQUIRED)
    );

    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("invalid"));
    assert_eq!(
        validate_upload_size(&Method::PUT, &headers, 11),
        Err(StatusCode::BAD_REQUEST)
    );

    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("12"));
    assert_eq!(
        validate_upload_size(&Method::PUT, &headers, 11),
        Err(StatusCode::PAYLOAD_TOO_LARGE)
    );

    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("11"));
    assert_eq!(validate_upload_size(&Method::PUT, &headers, 11), Ok(()));
    let propfind = Method::from_bytes(b"PROPFIND").expect("PROPFIND method");
    assert_eq!(validate_upload_size(&propfind, &headers, 1), Ok(()));
}

#[test]
fn test_response_body_owns_request_permit_until_dropped() {
    let gate = Arc::new(tokio::sync::RwLock::new(()));
    let permit = Arc::clone(&gate).try_read_owned().expect("request permit");
    let response = axum::response::Response::new(Body::empty());

    let guarded_response = guard_response_body(response, permit);

    assert!(gate.try_write().is_err());
    drop(guarded_response);
    assert!(gate.try_write().is_ok());
}

#[test]
fn test_only_staging_mutations_block_import_claims() {
    for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
        assert!(!request_mutates_staging(&method), "{method} must not block");
    }
    let propfind = Method::from_bytes(b"PROPFIND").expect("PROPFIND method");
    assert!(!request_mutates_staging(&propfind));

    for method in [Method::PUT, Method::PATCH, Method::DELETE] {
        assert!(request_mutates_staging(&method), "{method} must block");
    }
    for method in ["MKCOL", "MOVE", "COPY", "PROPPATCH"] {
        let method = Method::from_bytes(method.as_bytes()).expect("WebDAV mutation method");
        assert!(request_mutates_staging(&method), "{method} must block");
    }
}

#[test]
fn test_completed_upload_paths_require_a_complete_mutation() {
    let empty_headers = HeaderMap::new();
    assert_eq!(
        completed_upload_path(&Method::PUT, &empty_headers, "/webdav/video.mp4", "/webdav"),
        Some("video.mp4".to_string())
    );
    assert_eq!(
        completed_upload_path(
            &Method::PATCH,
            &empty_headers,
            "/webdav/video.mp4",
            "/webdav"
        ),
        None
    );

    let mut patch_headers = HeaderMap::new();
    patch_headers.insert(
        header::CONTENT_RANGE,
        HeaderValue::from_static("bytes 10-19/20"),
    );
    assert_eq!(
        completed_upload_path(
            &Method::PATCH,
            &patch_headers,
            "/webdav/video.mp4",
            "/webdav"
        ),
        Some("video.mp4".to_string())
    );

    let move_method = Method::from_bytes(b"MOVE").expect("MOVE method");
    let mut move_headers = HeaderMap::new();
    move_headers.insert(
        "destination",
        HeaderValue::from_static("http://photos.example/webdav/Camera%20Roll/video.mp4"),
    );
    assert_eq!(
        completed_upload_path(
            &move_method,
            &move_headers,
            "/webdav/.uploading.mp4",
            "/webdav"
        ),
        Some("Camera Roll/video.mp4".to_string())
    );
    assert_eq!(
        invalidated_upload_paths(
            &move_method,
            &move_headers,
            "/webdav/.uploading.mp4",
            "/webdav"
        ),
        vec![
            ".uploading.mp4".to_string(),
            "Camera Roll/video.mp4".to_string()
        ]
    );
}

#[test]
fn test_internal_processing_path_is_reserved_after_percent_decoding() {
    assert!(contains_reserved_path(
        "/webdav/.processing/claim/photo.jpg"
    ));
    assert!(contains_reserved_path(
        "/webdav/%2eprocessing/claim/photo.jpg"
    ));
    assert!(!contains_reserved_path(
        "/webdav/Camera%20Roll/.uploading.jpg"
    ));
    let mut headers = HeaderMap::new();
    headers.insert(
        "destination",
        HeaderValue::from_static("http://photos.example/webdav/%2eprocessing/file.jpg"),
    );
    assert!(contains_reserved_destination(&headers));
}

#[tokio::test]
async fn test_photosync_upload_sequence_preserves_webdav_mount_path() {
    let pool = crate::test_utils::create_test_db();
    momento_api::database::init_database(&pool.get().expect("schema connection")).expect("schema");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool);
    let username = "webdav-handler-test";
    let webdav_root = data_directory.join("webdav").join(username);

    let (status, _) = request(&executors, username, 1024, "OPTIONS", "/webdav/", &[], b"").await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = request(
        &executors,
        username,
        1024,
        "MKCOL",
        "/webdav/Camera%20Roll",
        &[],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let jpeg = b"photo bytes";
    let (status, _) = request(
        &executors,
        username,
        1024,
        "PUT",
        "/webdav/Camera%20Roll/.uploading.jpg",
        &[
            (header::CONTENT_TYPE.as_str(), "image/jpeg"),
            (header::CONTENT_LENGTH.as_str(), "11"),
        ],
        jpeg,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = request(
        &executors,
        username,
        1024,
        "PROPFIND",
        "/webdav/Camera%20Roll/",
        &[("depth", "1")],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::MULTI_STATUS);
    assert!(body.contains("/webdav/Camera%20Roll/"), "{body}");
    assert!(
        body.contains("/webdav/Camera%20Roll/.uploading.jpg"),
        "{body}"
    );

    let (status, _) = request(
        &executors,
        username,
        1024,
        "MOVE",
        "/webdav/Camera%20Roll/.uploading.jpg",
        &[(
            "destination",
            "http://photos.example/webdav/Camera%20Roll/photo.jpg",
        )],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        std::fs::read(webdav_root.join("Camera Roll/photo.jpg")).expect("uploaded file"),
        jpeg
    );

    let video = b"chunked video bytes";
    let (status, _) = request(
        &executors,
        username,
        video.len() as u64,
        "PUT",
        "/webdav/Camera%20Roll/video.mp4",
        &[(header::TRANSFER_ENCODING.as_str(), "chunked")],
        video,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        std::fs::read(webdav_root.join("Camera Roll/video.mp4")).expect("uploaded video"),
        video
    );

    let (status, _) = request(
        &executors,
        username,
        5,
        "PUT",
        "/webdav/Camera%20Roll/oversized.mp4",
        &[(header::TRANSFER_ENCODING.as_str(), "chunked")],
        b"too many bytes",
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!webdav_root.join("Camera Roll/oversized.mp4").exists());
}

#[tokio::test]
async fn webdav_collections_empty_files_and_large_frames_use_executor_journals() {
    let pool = crate::test_utils::create_test_db();
    momento_api::database::init_database(&pool.get().expect("schema connection")).expect("schema");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool);
    let username = "webdav-collection-test";
    let root = data_directory.join("webdav").join(username);

    for path in ["/webdav/albums", "/webdav/albums/trip"] {
        let (status, _) = request(
            &executors,
            username,
            4 * 1024 * 1024,
            "MKCOL",
            path,
            &[],
            b"",
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (status, _) = request(
        &executors,
        username,
        4 * 1024 * 1024,
        "PUT",
        "/webdav/albums/trip/empty.txt",
        &[(header::CONTENT_LENGTH.as_str(), "0")],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        std::fs::metadata(root.join("albums/trip/empty.txt"))
            .unwrap()
            .len(),
        0
    );

    let large = vec![0x5a; 2 * 1024 * 1024 + 17];
    let length = large.len().to_string();
    let (status, _) = request_with_body(
        &executors,
        username,
        4 * 1024 * 1024,
        "PUT",
        "/webdav/albums/trip/large.bin",
        &[(header::CONTENT_LENGTH.as_str(), &length)],
        Body::from(large.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        std::fs::read(root.join("albums/trip/large.bin")).unwrap(),
        large
    );

    let (status, _) = request(
        &executors,
        username,
        4 * 1024 * 1024,
        "MOVE",
        "/webdav/albums/trip",
        &[("destination", "http://photos.example/webdav/albums/renamed")],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(root.join("albums/renamed/large.bin").is_file());
    assert!(!root.join("albums/trip").exists());

    let (status, _) = request(
        &executors,
        username,
        4 * 1024 * 1024,
        "COPY",
        "/webdav/albums/renamed",
        &[("destination", "http://photos.example/webdav/albums/copied")],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        std::fs::read(root.join("albums/copied/large.bin")).unwrap(),
        large
    );
    assert_eq!(
        std::fs::metadata(root.join("albums/copied/empty.txt"))
            .unwrap()
            .len(),
        0
    );

    let (status, _) = request(
        &executors,
        username,
        4 * 1024 * 1024,
        "DELETE",
        "/webdav/albums/renamed",
        &[],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!root.join("albums/renamed").exists());

    let (status, _) = request(
        &executors,
        username,
        4 * 1024 * 1024,
        "DELETE",
        "/webdav/albums/copied",
        &[],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!root.join("albums/copied").exists());
}

#[tokio::test]
async fn propfind_streams_depth_zero_one_and_infinity_across_entry_batches() {
    let pool = crate::test_utils::create_test_db();
    momento_api::database::init_database(&pool.get().expect("schema connection")).expect("schema");
    let (executors, data_directory) =
        crate::test_utils::test_executor_handles_with_data_directory(pool);
    let username = "webdav-propfind-test";
    let root = data_directory.join("webdav").join(username);

    let (status, _) = request(
        &executors,
        username,
        1024,
        "MKCOL",
        "/webdav/library",
        &[],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    std::fs::create_dir(root.join("library/nested")).expect("nested directory");
    std::fs::write(root.join("library/nested/deep.txt"), b"deep").expect("nested file");
    for index in 0..257_u16 {
        std::fs::write(root.join(format!("library/item-{index:03}.txt")), b"x")
            .expect("directory entry");
    }

    let (status, depth_zero) = request(
        &executors,
        username,
        1024,
        "PROPFIND",
        "/webdav/library/",
        &[("depth", "0")],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::MULTI_STATUS);
    assert!(depth_zero.contains("/webdav/library/"));
    assert!(!depth_zero.contains("item-000.txt"));

    let (status, depth_one) = request(
        &executors,
        username,
        1024,
        "PROPFIND",
        "/webdav/library/",
        &[("depth", "1")],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::MULTI_STATUS);
    assert_eq!(depth_one.matches("item-").count(), 257);
    assert!(depth_one.contains("/webdav/library/nested/"));
    assert!(!depth_one.contains("deep.txt"));

    let (status, infinity) = request(
        &executors,
        username,
        1024,
        "PROPFIND",
        "/webdav/library/",
        &[("depth", "infinity")],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::MULTI_STATUS);
    assert!(infinity.contains("/webdav/library/nested/deep.txt"));

    let (status, _) = request(
        &executors,
        username,
        1024,
        "PROPFIND",
        "/webdav/library/",
        &[("depth", "2")],
        b"",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
