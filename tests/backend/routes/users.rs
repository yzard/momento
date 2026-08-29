use axum::http::{header::AUTHORIZATION, StatusCode};
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

#[tokio::test]
async fn reserved_admin_cannot_be_deactivated_or_deleted_by_another_administrator() {
    let (app, pool) = create_test_app();
    let actor_id = create_test_user(&pool, "manager", "manager@example.com");
    let reserved_admin_id = create_test_user(&pool, "admin", "admin@example.com");
    let connection = pool.get().expect("database");
    connection
        .execute(
            "UPDATE users SET role = 'admin' WHERE id IN (?, ?)",
            [actor_id, reserved_admin_id],
        )
        .expect("administrator roles");
    drop(connection);

    let access_token = create_access_token(actor_id, "manager", "admin", &Config::default(), None)
        .expect("access token");
    let authorization = format!("Bearer {access_token}");
    let server = TestServer::new(app).expect("server");

    server
        .post("/api/v1/user/update")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"userId": reserved_admin_id, "isActive": false}))
        .await
        .assert_status(StatusCode::CONFLICT);
    server
        .post("/api/v1/user/delete")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"userId": reserved_admin_id}))
        .await
        .assert_status(StatusCode::CONFLICT);

    let listed = server
        .post("/api/v1/user/list")
        .add_header(AUTHORIZATION, authorization)
        .await
        .json::<serde_json::Value>();
    let reserved_admin = listed["users"]
        .as_array()
        .expect("users")
        .iter()
        .find(|user| user["id"] == reserved_admin_id)
        .expect("reserved admin");
    assert_eq!(reserved_admin["isReserved"], true);
    assert_eq!(reserved_admin["isActive"], true);
}

#[test]
fn database_rejects_direct_reserved_admin_deactivation_and_deletion() {
    let pool = crate::test_utils::create_test_db();
    let reserved_admin_id = create_test_user(&pool, "admin", "admin@example.com");
    let connection = pool.get().expect("database");

    let deactivation_error = connection
        .execute(
            "UPDATE users SET is_active = 0 WHERE id = ?",
            [reserved_admin_id],
        )
        .expect_err("reserved admin deactivation must fail");
    assert!(deactivation_error
        .to_string()
        .contains("reserved admin account cannot be deactivated"));

    let deletion_error = connection
        .execute("DELETE FROM users WHERE id = ?", [reserved_admin_id])
        .expect_err("reserved admin deletion must fail");
    assert!(deletion_error
        .to_string()
        .contains("reserved admin account cannot be deleted"));
}
