use crate::test_utils::{
    create_test_app, create_test_media, create_test_user, test_data_directory,
};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_RANGE, RANGE};
use axum::http::StatusCode;
use axum_test::TestServer;

#[tokio::test]
async fn public_share_media_uses_the_shared_bounded_range_stream() {
    let (server, media_id, _) = public_media_fixture();
    let response = server
        .get(&format!(
            "/api/v1/public/share/range-token/media/{media_id}"
        ))
        .add_header(RANGE, "bytes=2-5")
        .await;
    response.assert_status(StatusCode::PARTIAL_CONTENT);
    response.assert_header(CONTENT_RANGE, "bytes 2-5/10");
    response.assert_header(
        CONTENT_DISPOSITION,
        "attachment; filename=\"range-video.mp4\"",
    );
    assert_eq!(response.as_bytes().as_ref(), b"2345");
}

#[tokio::test]
async fn saturated_media_streams_wait_and_complete_when_a_slot_is_released() {
    let (server, media_id, executors) = public_media_fixture();
    let mut held_streams = Vec::new();
    for _ in 0..executors.scheduler.stream_capacity() {
        let admission = momento_api::runtime::HttpRequestAdmission::acquire(&executors.scheduler)
            .await
            .unwrap();
        admission.convert_to_stream().await.unwrap();
        held_streams.push(admission);
    }
    for range in [false, true] {
        let path = format!("/api/v1/public/share/range-token/media/{media_id}");
        let mut request = server.get(&path);
        if range {
            request = request.add_header(RANGE, "bytes=2-5");
        }
        let response = std::future::IntoFuture::into_future(request);
        tokio::pin!(response);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut response)
                .await
                .is_err()
        );
        held_streams.pop();
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), response)
            .await
            .unwrap();
        response.assert_status(if range {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        });
        assert_eq!(
            response.as_bytes().as_ref(),
            if range {
                &b"2345"[..]
            } else {
                &b"0123456789"[..]
            }
        );
        let admission = momento_api::runtime::HttpRequestAdmission::acquire(&executors.scheduler)
            .await
            .unwrap();
        admission.convert_to_stream().await.unwrap();
        held_streams.push(admission);
    }
    held_streams.pop();
    let response = server
        .get(&format!(
            "/api/v1/public/share/range-token/media/{media_id}"
        ))
        .await;
    response.assert_status_ok();
    assert_eq!(response.as_bytes().as_ref(), b"0123456789");
}

fn public_media_fixture() -> (TestServer, i64, momento_api::runtime::ExecutorHandles) {
    let (app, pool) = create_test_app();
    let owner_id = create_test_user(&pool, "range-owner", "range-owner@example.com");
    let media_id = create_test_media(&pool, "range-video.mp4");
    let relative_path = format!("route-tests/public-{media_id}.mp4");
    let original_path = test_data_directory(&pool)
        .join("originals")
        .join(&relative_path);
    std::fs::create_dir_all(original_path.parent().expect("original parent"))
        .expect("original directory");
    std::fs::write(&original_path, b"0123456789").expect("original bytes");
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "UPDATE media SET file_path = ?, media_type = 'video', mime_type = 'video/mp4' WHERE id = ?",
            rusqlite::params![relative_path, media_id],
        )
        .expect("media path");
    connection
        .execute(
            "INSERT INTO share_links (user_id, media_id, token) VALUES (?, ?, 'range-token')",
            rusqlite::params![owner_id, media_id],
        )
        .expect("share link");
    drop(connection);
    let server = TestServer::new(app).expect("server");
    (
        server,
        media_id,
        crate::test_utils::test_executor_handles(pool),
    )
}
