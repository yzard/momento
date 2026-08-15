use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use base64::Engine;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use serde_json::json;

use crate::test_utils::{create_test_app, create_test_media, create_test_user, grant_media_access};

fn token(user_id: i64, role: &str) -> String {
    create_access_token(user_id, "faces", role, &Config::default()).expect("token")
}

#[tokio::test]
async fn face_groups_are_filtered_to_media_access_and_admin_can_merge() {
    let (app, pool) = create_test_app();
    let viewer_id = create_test_user(&pool, "face-viewer", "face-viewer@example.com");
    let administrator_id = create_test_user(&pool, "face-admin", "face-admin@example.com");
    let visible_media_id = create_test_media(&pool, "visible.jpg");
    let hidden_media_id = create_test_media(&pool, "hidden.jpg");
    grant_media_access(&pool, visible_media_id, viewer_id);
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "UPDATE users SET role = 'admin' WHERE id = ?",
            [administrator_id],
        )
        .expect("admin");
    let embedding = base64::engine::general_purpose::STANDARD.encode(vec![0_u8; 512 * 4]);
    for media_id in [visible_media_id, hidden_media_id] {
        connection.execute("INSERT INTO media_faces (media_id, input_sequence, face_index, x, y, width, height, confidence, quality, embedding, crop_path) VALUES (?, 0, 0, 0, 0, 1, 1, 1, 1, ?, 'faces/missing.jpg')", rusqlite::params![media_id, base64::engine::general_purpose::STANDARD.decode(&embedding).expect("embedding")]).expect("face");
        let face_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO face_groups (representative_face_id) VALUES (?)",
                [face_id],
            )
            .expect("group");
        connection
            .execute(
                "INSERT INTO face_group_members (face_group_id, face_id) VALUES (?, ?)",
                [connection.last_insert_rowid(), face_id],
            )
            .expect("member");
    }
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let response = server
        .post("/api/v1/faces/groups/list")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(viewer_id, "user")),
        )
        .json(&json!({"limit": 10}))
        .await;
    response.assert_status_ok();
    let list_body = response.json::<serde_json::Value>();
    assert_eq!(list_body["groups"].as_array().expect("groups").len(), 1);
    assert_eq!(list_body["groups"][0]["mediaCount"], 1);
    server
        .post("/api/v1/faces/groups/merge")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(viewer_id, "user")),
        )
        .json(&json!({"faceGroupIds": [1, 2]}))
        .await
        .assert_status_forbidden();
    let merge = server
        .post("/api/v1/faces/groups/merge")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(administrator_id, "admin")),
        )
        .json(&json!({"faceGroupIds": [1, 2]}))
        .await;
    merge.assert_status_ok();
    assert_eq!(merge.json::<serde_json::Value>()["group"]["mediaCount"], 2);
    let connection = pool.get().expect("connection");
    let (manual_curated, member_count): (i64, i64) = connection
        .query_row(
            "SELECT face_groups.manual_curated, COUNT(face_group_members.face_id) FROM face_groups JOIN face_group_members ON face_group_members.face_group_id = face_groups.id WHERE face_groups.id = 1 GROUP BY face_groups.id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("merged group");
    assert_eq!(manual_curated, 1);
    assert_eq!(member_count, 2);
    let source_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM face_groups WHERE id = 2", [], |row| {
            row.get(0)
        })
        .expect("source group count");
    assert_eq!(source_count, 0);
}
