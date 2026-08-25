use chrono::{Duration, Utc};
use jsonwebtoken::{decode_header, Algorithm};
use momento_api::auth::{
    create_access_token, create_media_access_ticket, create_share_session_token,
    decode_access_token, decode_media_access_ticket, decode_share_session_token,
};
use momento_api::config::Config;
use momento_api::models::MediaAccessResource;

#[test]
fn access_tokens_always_use_hs256() {
    let token = create_access_token(1, "user", "user", &Config::default(), None)
        .expect("Failed to create access token");

    let header = decode_header(&token).expect("Failed to decode access token header");

    assert_eq!(header.alg, Algorithm::HS256);
}

#[test]
fn scoped_tokens_are_domain_separated_and_bound_to_their_resource() {
    let config = Config::default();
    let (media_ticket, media_expiration) =
        create_media_access_ticket(7, 42, MediaAccessResource::Original, &config)
            .expect("media ticket");
    let media_claims = decode_media_access_ticket(&media_ticket, &config).expect("media claims");
    assert_eq!(media_claims.sub, "7");
    assert_eq!(media_claims.media_id, 42);
    assert_eq!(media_claims.resource, MediaAccessResource::Original);
    assert!(media_expiration > Utc::now());
    assert!(decode_access_token(&media_ticket, &config).is_none());

    let (share_session, share_expiration) = create_share_session_token(
        11,
        "share-token",
        Some(Utc::now() + Duration::hours(1)),
        &config,
    )
    .expect("share session");
    let share_claims = decode_share_session_token(&share_session, &config).expect("share claims");
    assert_eq!(share_claims.share_id, 11);
    assert!(share_expiration <= Utc::now() + Duration::hours(1));
    assert!(decode_media_access_ticket(&share_session, &config).is_none());
}

#[test]
fn expired_scoped_tokens_are_rejected() {
    let mut config = Config::default();
    config.security.media_access_ticket_expire_hours = -1;
    let (ticket, _) = create_media_access_ticket(7, 42, MediaAccessResource::Original, &config)
        .expect("expired media ticket");

    assert!(decode_media_access_ticket(&ticket, &config).is_none());

    let (share_session, _) = create_share_session_token(
        11,
        "expired-share",
        Some(Utc::now() - Duration::hours(1)),
        &Config::default(),
    )
    .expect("expired share session");
    assert!(decode_share_session_token(&share_session, &Config::default()).is_none());
}
