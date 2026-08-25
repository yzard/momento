use crate::test_utils::{create_test_app, create_test_media, create_test_user};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_RANGE, RANGE};
use axum::http::StatusCode;
use axum_test::TestServer;

#[tokio::test]
async fn public_share_media_uses_the_shared_bounded_range_stream() {
    let (app, pool) = create_test_app();
    let owner_id = create_test_user(&pool, "range-owner", "range-owner@example.com");
    let media_id = create_test_media(&pool, "range-video.mp4");
    let relative_path = format!("route-tests/public-{media_id}.mp4");
    let original_path = momento_api::constants::paths()
        .originals
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
