use super::*;
use sha2::{Digest, Sha256};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

async fn prepare(server: &TestServer, access: &str, device: &str, op: &str, bytes: &[u8]) -> Value {
    let response = server.post("/api/v1/backup/upload/batch/prepare")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .json(&json!({"uploads": [upload_create_request(device, op, op, "original.jpg", bytes.len() as u64,
            &hash(bytes), "2024-01-02T03:04:05Z")]})).await;
    response.assert_status_ok();
    response.json::<Value>()["results"][0]["upload"].clone()
}

fn body(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let manifest = serde_json::to_vec(&json!({"version":1,"entries":entries.iter().map(|(id, bytes)|
        json!({"uploadId":id,"byteSize":bytes.len(),"contentHash":hash(bytes)})).collect::<Vec<_>>()})).unwrap();
    let mut result = (manifest.len() as u32).to_be_bytes().to_vec();
    result.extend(manifest);
    for (_, bytes) in entries {
        result.extend_from_slice(bytes);
    }
    result
}

#[tokio::test]
async fn batch_upload_preserves_per_file_hashes_and_replay_identity() {
    let (app, pool) = create_test_app();
    let owner = create_test_user(&pool, "batch-owner", "batch@example.com");
    let access = token(owner);
    let server = TestServer::new(app).unwrap();
    register_device(&server, &access, "batch-device").await;
    let first = prepare(&server, &access, "batch-device", "first", b"hello").await;
    let second = prepare(&server, &access, "batch-device", "second", b"world").await;
    let ids = [
        first["uploadId"].as_str().unwrap(),
        second["uploadId"].as_str().unwrap(),
    ];
    let payload = body(&[(ids[0], b"hello"), (ids[1], b"world")]);
    for _ in 0..2 {
        let response = server
            .post("/api/v1/backup/upload/batch")
            .add_header(AUTHORIZATION, format!("Bearer {access}"))
            .add_header(CONTENT_LENGTH, payload.len().to_string())
            .add_header("Content-Type", "application/octet-stream")
            .bytes(Bytes::from(payload.clone()))
            .await;
        response.assert_status_ok();
        let data = response.json::<Value>();
        for (i, bytes) in [b"hello", b"world"].iter().enumerate() {
            assert_eq!(data["results"][i]["key"], ids[i]);
            assert_eq!(data["results"][i]["upload"]["uploadedSize"], 5);
            assert_eq!(data["results"][i]["upload"]["contentHash"], hash(*bytes));
            assert!(data["results"][i]["error"].is_null());
        }
    }
    let status = server
        .post("/api/v1/backup/upload/batch/status")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .json(&json!({"uploadIds":ids}))
        .await;
    status.assert_status_ok();
    assert_eq!(
        status.json::<Value>()["results"].as_array().unwrap().len(),
        2
    );
}

#[tokio::test]
async fn batch_limits_allow_one_oversized_media_but_reject_an_oversized_group() {
    let mut config = crate::test_utils::test_config();
    config.backup.max_batch_bytes = 8;
    let (app, pool) = crate::test_utils::create_test_app_with_config(config);
    let owner = create_test_user(&pool, "batch-limit", "limit@example.com");
    let access = token(owner);
    let server = TestServer::new(app).unwrap();
    register_device(&server, &access, "batch-device").await;
    let a = prepare(&server, &access, "batch-device", "one", b"hello").await;
    let b = prepare(&server, &access, "batch-device", "two", b"world").await;
    let payload = body(&[
        (a["uploadId"].as_str().unwrap(), b"hello"),
        (b["uploadId"].as_str().unwrap(), b"world"),
    ]);
    server
        .post("/api/v1/backup/upload/batch")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .add_header(CONTENT_LENGTH, payload.len().to_string())
        .bytes(Bytes::from(payload))
        .await
        .assert_status_bad_request();
    let big = prepare(&server, &access, "batch-device", "big", b"hello-world").await;
    let payload = body(&[(big["uploadId"].as_str().unwrap(), b"hello-world")]);
    let response = server
        .post("/api/v1/backup/upload/batch")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .add_header(CONTENT_LENGTH, payload.len().to_string())
        .bytes(Bytes::from(payload))
        .await;
    response.assert_status_ok();
    assert_eq!(
        response.json::<Value>()["results"][0]["upload"]["uploadedSize"],
        11
    );
}

#[tokio::test]
async fn batch_is_user_scoped_and_preserves_good_files_when_another_hash_is_wrong() {
    let (app, pool) = create_test_app();
    let owner = create_test_user(&pool, "batch-good", "good@example.com");
    let other = create_test_user(&pool, "batch-other", "other@example.com");
    let access = token(owner);
    let server = TestServer::new(app).unwrap();
    register_device(&server, &access, "batch-device").await;
    let a = prepare(&server, &access, "batch-device", "one", b"hello").await;
    let b = prepare(&server, &access, "batch-device", "two", b"world").await;
    let a_id = a["uploadId"].as_str().unwrap();
    let b_id = b["uploadId"].as_str().unwrap();
    let payload = body(&[(a_id, b"hello")]);
    let forbidden = server
        .post("/api/v1/backup/upload/batch")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(other)))
        .add_header(CONTENT_LENGTH, payload.len().to_string())
        .bytes(Bytes::from(payload))
        .await;
    forbidden.assert_status_ok();
    assert_eq!(
        forbidden.json::<Value>()["results"][0]["error"]["status"],
        404
    );
    let mut payload = body(&[(a_id, b"hello"), (b_id, b"world")]);
    let last = payload.len() - 1;
    payload[last] ^= 1;
    let response = server
        .post("/api/v1/backup/upload/batch")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .add_header(CONTENT_LENGTH, payload.len().to_string())
        .bytes(Bytes::from(payload))
        .await;
    response.assert_status_ok();
    let data = response.json::<Value>();
    assert_eq!(data["results"][0]["upload"]["uploadedSize"], 5);
    assert_eq!(data["results"][1]["error"]["status"], 400);
    let status = server
        .post("/api/v1/backup/upload/status")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .json(&json!({"uploadId":b_id}))
        .await;
    assert_eq!(status.json::<Value>()["uploadedSize"], 0);
}

#[tokio::test]
async fn batch_rejects_duplicate_keys_and_truncated_payload_without_marking_completion() {
    let (app, pool) = create_test_app();
    let owner = create_test_user(&pool, "batch-short", "short@example.com");
    let access = token(owner);
    let server = TestServer::new(app).unwrap();
    register_device(&server, &access, "batch-device").await;
    let a = prepare(&server, &access, "batch-device", "one", b"hello").await;
    let id = a["uploadId"].as_str().unwrap();
    let duplicate = body(&[(id, b"hello"), (id, b"hello")]);
    server
        .post("/api/v1/backup/upload/batch")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .add_header(CONTENT_LENGTH, duplicate.len().to_string())
        .bytes(Bytes::from(duplicate))
        .await
        .assert_status_bad_request();
    let mut short = body(&[(id, b"hello")]);
    short.pop();
    server
        .post("/api/v1/backup/upload/batch")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .add_header(CONTENT_LENGTH, short.len().to_string())
        .bytes(Bytes::from(short))
        .await
        .assert_status_bad_request();
    let status = server
        .post("/api/v1/backup/upload/status")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .json(&json!({"uploadId":id}))
        .await;
    assert_eq!(status.json::<Value>()["uploadedSize"], 0);
}

#[tokio::test]
async fn interrupted_batch_retains_confirmed_files_and_can_retry_the_rest() {
    let (app, pool) = create_test_app();
    let owner = create_test_user(&pool, "batch-interrupt", "interrupt@example.com");
    let access = token(owner);
    let server = TestServer::new(app).unwrap();
    register_device(&server, &access, "batch-device").await;
    let a = prepare(&server, &access, "batch-device", "one", b"hello").await;
    let b = prepare(&server, &access, "batch-device", "two", b"world").await;
    let ids = [
        a["uploadId"].as_str().unwrap(),
        b["uploadId"].as_str().unwrap(),
    ];
    let mut payload = body(&[(ids[0], b"hello"), (ids[1], b"world")]);
    let declared = payload.len();
    payload.pop();
    server
        .post("/api/v1/backup/upload/batch")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .add_header(CONTENT_LENGTH, declared.to_string())
        .bytes(Bytes::from(payload))
        .await
        .assert_status_bad_request();
    let status = server
        .post("/api/v1/backup/upload/batch/status")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .json(&json!({"uploadIds":ids}))
        .await;
    let results = status.json::<Value>();
    assert_eq!(results["results"][0]["upload"]["uploadedSize"], 5);
    assert_eq!(results["results"][1]["upload"]["uploadedSize"], 0);
    let payload = body(&[(ids[1], b"world")]);
    let retry = server
        .post("/api/v1/backup/upload/batch")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .add_header(CONTENT_LENGTH, payload.len().to_string())
        .bytes(Bytes::from(payload))
        .await;
    retry.assert_status_ok();
    assert_eq!(
        retry.json::<Value>()["results"][0]["upload"]["uploadedSize"],
        5
    );
}

#[tokio::test]
async fn fifty_megabyte_batch_streams_beyond_the_json_body_limit() {
    let (app, pool) = create_test_app();
    let owner = create_test_user(&pool, "batch-fifty", "fifty@example.com");
    let access = token(owner);
    let server = TestServer::new(app).unwrap();
    register_device(&server, &access, "batch-device").await;
    let first = vec![1; 25_000_000];
    let second = vec![2; 25_000_000];
    let a = prepare(&server, &access, "batch-device", "one", &first).await;
    let b = prepare(&server, &access, "batch-device", "two", &second).await;
    let payload = body(&[
        (a["uploadId"].as_str().unwrap(), &first),
        (b["uploadId"].as_str().unwrap(), &second),
    ]);
    let response = server
        .post("/api/v1/backup/upload/batch")
        .add_header(AUTHORIZATION, format!("Bearer {access}"))
        .add_header(CONTENT_LENGTH, payload.len().to_string())
        .bytes(Bytes::from(payload))
        .await;
    response.assert_status_ok();
    let results = response.json::<Value>();
    for item in results["results"].as_array().unwrap() {
        assert_eq!(item["upload"]["uploadedSize"], 25_000_000);
        assert!(item["error"].is_null());
    }
}

#[tokio::test]
async fn fragmented_batch_coalesces_disk_work_and_preserves_file_boundaries() {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let (app, pool) = create_test_app();
    let scheduler = crate::test_utils::test_scheduler(pool.clone());
    let owner = create_test_user(&pool, "fragmented-owner", "fragmented@example.com");
    let access = token(owner);
    let server = TestServer::new(app.clone()).unwrap();
    register_device(&server, &access, "fragmented-device").await;
    let first = vec![31; 2 * 1024 * 1024 + 37];
    let second = vec![92; 1024 * 1024 + 13];
    let a = prepare(
        &server,
        &access,
        "fragmented-device",
        "fragment-first",
        &first,
    )
    .await;
    let b = prepare(
        &server,
        &access,
        "fragmented-device",
        "fragment-second",
        &second,
    )
    .await;
    let ids = [
        a["uploadId"].as_str().unwrap(),
        b["uploadId"].as_str().unwrap(),
    ];
    let payload = body(&[(ids[0], &first), (ids[1], &second)]);
    let frames = payload
        .chunks(1021)
        .map(|bytes| Ok::<_, std::io::Error>(Bytes::copy_from_slice(bytes)))
        .collect::<Vec<_>>();
    let before =
        scheduler.control_version(momento_api::runtime::SchedulerControlSource::FileIoCapacity);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/backup/upload/batch")
                .header(AUTHORIZATION, format!("Bearer {access}"))
                .header(CONTENT_LENGTH, payload.len())
                .header("Content-Type", "application/octet-stream")
                .body(Body::from_stream(futures::stream::iter(frames)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    for (i, expected) in [&first, &second].iter().enumerate() {
        assert_eq!(
            result["results"][i]["upload"]["uploadedSize"],
            expected.len()
        );
        assert_eq!(
            result["results"][i]["upload"]["contentHash"],
            hash(expected)
        );
        let path: String = pool.get().unwrap().query_row("SELECT a.staged_path FROM backup_assets a JOIN backup_upload_sessions s ON s.asset_id = a.id WHERE s.upload_id = ?", [ids[i]], |row| row.get(0)).unwrap();
        let root = crate::test_utils::test_data_directory(&pool);
        assert_eq!(
            std::fs::read(root.join("backups").join(path)).unwrap(),
            **expected
        );
    }
    let operations = scheduler
        .control_version(momento_api::runtime::SchedulerControlSource::FileIoCapacity)
        - before;
    // Thousands of HTTP fragments must not dispatch thousands of disk jobs.
    assert!(
        operations < 100,
        "fragmented backup dispatched {operations} file operations"
    );
}
