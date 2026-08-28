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
    AuthenticationProtection::new(&config)
}

#[test]
fn password_attempts_are_limited_by_identity_and_source() {
    let protection = create_protection(2, 10);
    protection
        .begin_password_attempt("192.0.2.1", "Alice")
        .expect("first attempt");
    protection
        .begin_password_attempt("192.0.2.2", "alice")
        .expect("second normalized identity attempt");

    assert!(matches!(
        protection.begin_password_attempt("192.0.2.3", "ALICE"),
        Err(AppError::RateLimited { .. })
    ));

    let protection = create_protection(10, 1);
    protection
        .begin_password_attempt("192.0.2.1", "alice")
        .expect("first source attempt");
    assert!(matches!(
        protection.begin_password_attempt("192.0.2.1", "bob"),
        Err(AppError::RateLimited { .. })
    ));
}

#[test]
fn successful_password_clears_the_identity_limit() {
    let protection = create_protection(1, 10);
    protection
        .begin_password_attempt("192.0.2.1", "alice")
        .expect("first attempt");
    protection.record_password_success("192.0.2.1", "alice");
    protection
        .begin_password_attempt("192.0.2.2", "alice")
        .expect("attempt after successful authentication");
}

#[test]
fn forwarded_addresses_are_used_only_for_trusted_peers() {
    let config = SecurityConfig {
        trusted_proxy_ip_addresses: vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))],
        ..SecurityConfig::default()
    };
    let protection = AuthenticationProtection::new(&config);
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
