use momento_api::auth::cleanup_refresh_tokens;

use crate::test_utils::{create_test_db, create_test_user, test_executor_handles};

#[tokio::test]
async fn cleanup_deletes_expired_and_revoked_refresh_tokens_only() {
    let pool = create_test_db();
    let executors = test_executor_handles(pool.clone());
    let user_id = create_test_user(&pool, "cleanup-user", "cleanup@example.com");
    let connection = pool.get().expect("database");
    for (token_hash, expires_at, revoked) in [
        ("active", "2999-01-01T00:00:00Z", 0),
        ("expired", "2000-01-01T00:00:00Z", 0),
        ("revoked", "2999-01-01T00:00:00Z", 1),
    ] {
        connection
            .execute(
                "INSERT INTO refresh_tokens (token_hash, user_id, expires_at, revoked) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![token_hash, user_id, expires_at, revoked],
            )
            .expect("refresh token");
    }
    drop(connection);

    assert_eq!(
        cleanup_refresh_tokens(&executors.sqlite)
            .await
            .expect("cleanup"),
        2
    );
    let remaining: Vec<String> = pool
        .get()
        .expect("database")
        .prepare("SELECT token_hash FROM refresh_tokens ORDER BY token_hash")
        .expect("query")
        .query_map([], |row| row.get(0))
        .expect("tokens")
        .collect::<Result<_, _>>()
        .expect("token hashes");
    assert_eq!(remaining, vec!["active"]);
}
