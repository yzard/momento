use momento_api::auth::{
    ensure_default_admin, prepare_admin_password_reset, verify_password, TEMPORARY_ADMIN_PASSWORD,
    TEMPORARY_ADMIN_USERNAME,
};

use crate::test_utils::{create_test_db, create_test_user};

#[test]
fn ensure_default_admin_creates_one_for_an_empty_database() {
    let pool = create_test_db();

    let admin_id = ensure_default_admin(&pool).expect("create default admin");
    let repeated_id = ensure_default_admin(&pool).expect("reuse default admin");

    assert_eq!(repeated_id, admin_id);
    let connection = pool.get().expect("database");
    let (username, password_hash, role, must_change_password): (String, String, String, i32) =
        connection
            .query_row(
                "SELECT username, hashed_password, role, must_change_password FROM users WHERE id = ?1",
                [admin_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("admin row");
    assert_eq!(username, TEMPORARY_ADMIN_USERNAME);
    assert_eq!(role, "admin");
    assert_eq!(must_change_password, 1);
    assert!(verify_password(TEMPORARY_ADMIN_PASSWORD, &password_hash));
}

#[test]
fn prepare_admin_password_reset_preserves_account_and_revokes_refresh_tokens() {
    let pool = create_test_db();
    let admin_id = create_test_user(&pool, "existing-admin", "admin@example.com");
    let connection = pool.get().expect("database");
    connection
        .execute(
            "UPDATE users SET role = 'admin', must_change_password = 0 WHERE id = ?1",
            [admin_id],
        )
        .expect("promote admin");
    let original_hash: String = connection
        .query_row(
            "SELECT hashed_password FROM users WHERE id = ?1",
            [admin_id],
            |row| row.get(0),
        )
        .expect("password hash");
    connection
        .execute(
            "INSERT INTO refresh_tokens (token_hash, user_id, expires_at) VALUES ('token', ?1, '2999-01-01T00:00:00Z')",
            [admin_id],
        )
        .expect("refresh token");
    drop(connection);

    prepare_admin_password_reset(&pool, admin_id).expect("prepare reset");

    let connection = pool.get().expect("database");
    let (saved_hash, must_change_password, revoked): (String, i32, i32) = connection
        .query_row(
            "SELECT users.hashed_password, users.must_change_password, refresh_tokens.revoked FROM users JOIN refresh_tokens ON refresh_tokens.user_id = users.id WHERE users.id = ?1",
            [admin_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("admin state");
    assert_eq!(saved_hash, original_hash);
    assert_eq!(must_change_password, 0);
    assert_eq!(revoked, 1);
}
