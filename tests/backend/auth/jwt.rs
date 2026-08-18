use jsonwebtoken::{decode_header, Algorithm};
use momento_api::auth::create_access_token;
use momento_api::config::Config;

#[test]
fn access_tokens_always_use_hs256() {
    let token = create_access_token(1, "user", "user", &Config::default(), None)
        .expect("Failed to create access token");

    let header = decode_header(&token).expect("Failed to decode access token header");

    assert_eq!(header.alg, Algorithm::HS256);
}
