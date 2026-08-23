use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::{auth::create_access_token, config::Config};
use serde_json::json;

use crate::test_utils::{create_test_app, create_test_user};

#[tokio::test]
async fn user_update_requires_the_target_user_id_in_the_json_body() {
    let (app, pool) = create_test_app();
    let admin_id = create_test_user(&pool, "update-admin", "update-admin@example.com");
    let target_user_id = create_test_user(&pool, "update-target", "update-target@example.com");
    pool.get()
        .expect("database")
        .execute("UPDATE users SET role = 'admin' WHERE id = ?", [admin_id])
        .expect("administrator role");
    let access_token =
        create_access_token(admin_id, "update-admin", "admin", &Config::default(), None)
            .expect("access token");
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {access_token}");

    server
        .post("/api/v1/user/update?userId=999")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"isActive": false}))
        .await
        .assert_status_unprocessable_entity();
    let updated = server
        .post("/api/v1/user/update")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({"userId": target_user_id, "isActive": false}))
        .await;
    updated.assert_status_ok();
    assert_eq!(updated.json::<serde_json::Value>()["id"], target_user_id);
    assert_eq!(updated.json::<serde_json::Value>()["isActive"], false);
}
