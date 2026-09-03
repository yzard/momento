use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::http::{HeaderMap, HeaderValue};
use momento_api::auth::AuthenticationProtection;
use momento_api::config::SecurityConfig;
use momento_api::error::AppError;

fn create_protection(identity_limit: u32, source_limit: u32) -> AuthenticationProtection {
    let config = SecurityConfig {
        password_attempts_per_identity: identity_limit,
        password_attempts_per_source: source_limit,
        ..SecurityConfig::default()
    };
    let executors = crate::test_utils::test_executor_handles(crate::test_utils::create_test_db());
    AuthenticationProtection::new(
        &config,
        executors.cpu,
        executors.sqlite,
        crate::test_utils::test_authentication_dummy_hash(),
    )
}

#[tokio::test]
async fn password_attempts_are_limited_by_identity_and_source() {
    let protection = create_protection(2, 10);
    protection
        .begin_password_attempt("192.0.2.1", "identity-limit-alice")
        .await
        .expect("first attempt");
    protection
        .begin_password_attempt("192.0.2.2", "IDENTITY-LIMIT-ALICE")
        .await
        .expect("second normalized identity attempt");

    assert!(matches!(
        protection
            .begin_password_attempt("192.0.2.3", "identity-limit-alice")
            .await,
        Err(AppError::RateLimited { .. })
    ));

    let protection = create_protection(10, 1);
    protection
        .begin_password_attempt("192.0.2.101", "source-limit-alice")
        .await
        .expect("first source attempt");
    assert!(matches!(
        protection
            .begin_password_attempt("192.0.2.101", "source-limit-bob")
            .await,
        Err(AppError::RateLimited { .. })
    ));
}

#[tokio::test]
async fn successful_password_clears_the_identity_limit() {
    let protection = create_protection(1, 10);
    protection
        .begin_password_attempt("192.0.2.201", "success-limit-alice")
        .await
        .expect("first attempt");
    protection
        .record_password_success("192.0.2.201", "success-limit-alice")
        .await
        .expect("clear successful attempt buckets");
    protection
        .begin_password_attempt("192.0.2.202", "success-limit-alice")
        .await
        .expect("attempt after successful authentication");
}

#[tokio::test]
async fn password_attempt_limits_survive_protection_reconstruction_without_raw_keys() {
    let pool = crate::test_utils::create_test_db();
    let config = SecurityConfig {
        password_attempts_per_identity: 1,
        password_attempts_per_source: 10,
        ..SecurityConfig::default()
    };
    let first_executors = crate::test_utils::test_executor_handles(pool.clone());
    let first = AuthenticationProtection::new(
        &config,
        first_executors.cpu,
        first_executors.sqlite,
        crate::test_utils::test_authentication_dummy_hash(),
    );
    first
        .begin_password_attempt("192.0.2.210", "persisted-alice@example.com")
        .await
        .expect("first persisted attempt");

    let replacement_executors = crate::test_utils::test_executor_handles(pool.clone());
    let replacement = AuthenticationProtection::new(
        &config,
        replacement_executors.cpu,
        replacement_executors.sqlite,
        crate::test_utils::test_authentication_dummy_hash(),
    );
    assert!(matches!(
        replacement
            .begin_password_attempt("192.0.2.211", "PERSISTED-ALICE@EXAMPLE.COM")
            .await,
        Err(AppError::RateLimited { .. })
    ));

    let connection = pool.get().expect("auth bucket database");
    let (count, invalid_key_count): (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), SUM(CASE WHEN typeof(bucket_key) != 'blob' OR length(bucket_key) != 32 THEN 1 ELSE 0 END) FROM auth_attempt_buckets",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("bounded hashed auth buckets");
    assert_eq!(count, 3);
    assert_eq!(invalid_key_count, 0);
}

#[test]
fn forwarded_addresses_are_used_only_for_trusted_peers() {
    let config = SecurityConfig {
        trusted_proxy_ip_addresses: vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))],
        ..SecurityConfig::default()
    };
    let executors = crate::test_utils::test_executor_handles(crate::test_utils::create_test_db());
    let protection = AuthenticationProtection::new(
        &config,
        executors.cpu,
        executors.sqlite,
        crate::test_utils::test_authentication_dummy_hash(),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_static("198.51.100.8, 10.0.0.2"),
    );

    assert_eq!(
        protection.client_source(&headers, Some(SocketAddr::from(([203, 0, 113, 1], 4000)))),
        "203.0.113.1"
    );
    assert_eq!(
        protection.client_source(&headers, Some(SocketAddr::from(([10, 0, 0, 1], 4000)))),
        "10.0.0.2"
    );
}

#[tokio::test]
async fn missing_account_hash_still_performs_a_password_verification() {
    let protection = create_protection(2, 10);
    assert!(!protection
        .verify_password("unknown-password", None)
        .await
        .expect("dummy verification"));
}
