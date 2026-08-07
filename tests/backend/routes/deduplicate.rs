use crate::test_utils::{create_test_app, create_test_media, create_test_user, grant_media_access};
use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use serde_json::{json, Value};

fn token(user_id: i64, role: &str) -> String {
    create_access_token(user_id, "testuser", role, &Config::default())
        .expect("Failed to create token")
}

#[tokio::test]
async fn groups_only_return_clusters_with_two_visible_items() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "viewer", "viewer@example.com");
    let visible_a = create_test_media(&pool, "a.jpg");
    let visible_b = create_test_media(&pool, "b.jpg");
    let hidden = create_test_media(&pool, "hidden.jpg");
    grant_media_access(&pool, visible_a, user_id);
    grant_media_access(&pool, visible_b, user_id);
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute(
            "INSERT INTO media_similarity_clusters (kind, representative_media_id) VALUES ('near_duplicate', ?)",
            [visible_a],
        )
        .expect("Failed to create cluster");
    let cluster_id = connection.last_insert_rowid();
    for media_id in [visible_a, visible_b, hidden] {
        connection
            .execute(
                "INSERT INTO media_similarity_cluster_members (cluster_id, media_id, cosine_similarity, perceptual_hash_distance) VALUES (?, ?, 1.0, 0)",
                rusqlite::params![cluster_id, media_id],
            )
            .expect("Failed to add cluster member");
    }
    drop(connection);
    let server = TestServer::new(app).expect("Failed to create server");

    let response = server
        .post("/api/v1/deduplicate/groups")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(user_id, "user")))
        .json(&json!({"cursor": null, "limit": 10}))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["groups"].as_array().unwrap().len(), 1);
    assert_eq!(body["groups"][0]["clusterId"], cluster_id);
    assert_eq!(body["groups"][0]["items"].as_array().unwrap().len(), 2);
    assert_eq!(body["nextCursor"], Value::Null);
    assert_eq!(body["hasMore"], false);
}

#[tokio::test]
async fn normal_user_cannot_read_admin_status() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "viewer2", "viewer2@example.com");
    let server = TestServer::new(app).expect("Failed to create server");

    let response = server
        .post("/api/v1/deduplicate/status")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(user_id, "user")))
        .await;

    response.assert_status_forbidden();
}

#[tokio::test]
async fn admin_can_read_idle_status() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "admin2", "admin2@example.com");
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("Failed to promote admin");
    drop(connection);
    let server = TestServer::new(app).expect("Failed to create server");

    let response = server
        .post("/api/v1/deduplicate/status")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(user_id, "admin")))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["status"], "idle");
}

#[tokio::test]
async fn admin_cannot_start_disabled_deduplication() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "admin-disabled", "admin-disabled@example.com");
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("Failed to promote admin");
    drop(connection);
    let server = TestServer::new(app).expect("Failed to create server");

    let response = server
        .post("/api/v1/deduplicate/start")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(user_id, "admin")))
        .await;

    response.assert_status_bad_request();
}

#[tokio::test]
async fn admin_can_cancel_persisted_running_scan() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "admin-cancel", "admin-cancel@example.com");
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("Failed to promote admin");
    drop(connection);
    momento_api::processor::deduplicator::create_run(&pool, "manual", None)
        .expect("Failed to create running scan");
    let server = TestServer::new(app).expect("Failed to create server");

    let response = server
        .post("/api/v1/deduplicate/cancel")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(user_id, "admin")))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["status"], "cancelling");
}

#[tokio::test]
async fn admin_clean_removes_similarity_results_and_marks_media_dirty() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "admin-clean", "admin-clean@example.com");
    let media_id = create_test_media(&pool, "clean.jpg");
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [user_id])
        .expect("Failed to promote admin");
    connection
        .execute(
            "INSERT INTO media_similarity_index (media_id, content_hash, model_version, preprocessing_version, embedding, perceptual_hash) VALUES (?, ?, 'model', 'preprocess', ?, 1)",
            rusqlite::params![media_id, format!("hash_{media_id}"), vec![0_u8; 4]],
        )
        .expect("Failed to insert similarity index");
    connection
        .execute(
            "UPDATE media_similarity_index SET embedding = X'', perceptual_hash = -1, processing_status = -1, processing_error = 'decode failed' WHERE media_id = ?",
            [media_id],
        )
        .expect("Failed to mark similarity failure");
    drop(connection);
    let server = TestServer::new(app).expect("Failed to create server");

    let response = server
        .post("/api/v1/deduplicate/clean")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(user_id, "admin")))
        .await;

    response.assert_status_ok();
    let connection = pool.get().expect("Failed to get connection");
    let index_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM media_similarity_index", [], |row| {
            row.get(0)
        })
        .expect("Failed to count indexes");
    let dirty_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_dirty WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("Failed to count dirty media");
    let run_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM media_similarity_runs", [], |row| {
            row.get(0)
        })
        .expect("Failed to count runs");
    assert_eq!(index_count, 0);
    assert_eq!(dirty_count, 1);
    assert_eq!(run_count, 0);
}
