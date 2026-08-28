use axum::http::{header::COOKIE, HeaderMap};
use momento_api::auth::{
    access_token_cookie, clear_access_token_cookie, clear_refresh_token_cookie,
    create_access_token_cookie, create_refresh_token_cookie, refresh_token_cookie,
};

#[test]
fn authentication_cookies_are_secure_http_only_and_path_scoped() {
    let access = create_access_token_cookie("access-token", 300)
        .expect("access cookie")
        .to_str()
        .expect("access cookie text")
        .to_string();
    let refresh = create_refresh_token_cookie("refresh-token", 600)
        .expect("refresh cookie")
        .to_str()
        .expect("refresh cookie text")
        .to_string();

    for cookie in [&access, &refresh] {
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("SameSite=Strict"));
    }
    assert!(access.contains("Path=/api/v1"));
    assert!(access.contains("Max-Age=300"));
    assert!(refresh.contains("Path=/api/v1/user/session"));
    assert!(refresh.contains("Max-Age=600"));
}

#[test]
fn authentication_cookie_values_are_read_from_multiple_headers() {
    let mut headers = HeaderMap::new();
    headers.append(COOKIE, "unrelated=value".parse().expect("cookie header"));
    headers.append(
        COOKIE,
        "momento_access_token=access; momento_refresh_token=refresh"
            .parse()
            .expect("authentication cookies"),
    );

    assert_eq!(access_token_cookie(&headers).as_deref(), Some("access"));
    assert_eq!(refresh_token_cookie(&headers).as_deref(), Some("refresh"));
}

#[test]
fn cleared_authentication_cookies_retain_security_attributes() {
    for cookie in [clear_access_token_cookie(), clear_refresh_token_cookie()] {
        let cookie = cookie.to_str().expect("clear cookie");
        assert!(cookie.contains("Max-Age=0"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("SameSite=Strict"));
    }
}
