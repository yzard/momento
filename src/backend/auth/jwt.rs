use crate::config::Config;
use crate::error::AppResult;
use crate::models::MediaAccessResource;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MEDIA_ACCESS_TICKET_CONTEXT: &str = "momento-media-access-ticket-v1";
const SHARE_SESSION_CONTEXT: &str = "momento-share-session-v1";

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub role: String,
    pub exp: i64,
    #[serde(rename = "type")]
    pub token_type: String,
    #[serde(default)]
    pub admin_reset_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MediaAccessTicketClaims {
    pub sub: String,
    pub media_id: i64,
    pub resource: MediaAccessResource,
    pub exp: i64,
    #[serde(rename = "type")]
    pub token_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShareSessionClaims {
    pub share_id: i64,
    pub share_token_hash: String,
    pub exp: i64,
    #[serde(rename = "type")]
    pub token_type: String,
}

pub fn create_access_token(
    user_id: i64,
    username: &str,
    role: &str,
    config: &Config,
    admin_reset_id: Option<&str>,
) -> AppResult<String> {
    let expiration = Utc::now() + Duration::minutes(config.security.access_token_expire_minutes);

    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        role: role.to_string(),
        exp: expiration.timestamp(),
        token_type: "access".to_string(),
        admin_reset_id: admin_reset_id.map(str::to_string),
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(config.security.secret_key.as_bytes()),
    )?;

    Ok(token)
}

pub fn create_refresh_token(
    _user_id: i64,
    config: &Config,
) -> (String, String, chrono::DateTime<Utc>) {
    let raw_token: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(43)
        .map(char::from)
        .collect();

    let token_hash = hash_refresh_token(&raw_token);
    let expires_at = Utc::now() + Duration::days(config.security.refresh_token_expire_days);

    (raw_token, token_hash, expires_at)
}

pub fn decode_access_token(token: &str, config: &Config) -> Option<Claims> {
    let validation = Validation::new(Algorithm::HS256);
    let claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.security.secret_key.as_bytes()),
        &validation,
    )
    .ok()?
    .claims;
    if claims.token_type != "access" {
        return None;
    }
    Some(claims)
}

pub fn create_media_access_ticket(
    user_id: i64,
    media_id: i64,
    resource: MediaAccessResource,
    config: &Config,
) -> AppResult<(String, chrono::DateTime<Utc>)> {
    let expires_at = Utc::now() + Duration::hours(config.security.media_access_ticket_expire_hours);
    let claims = MediaAccessTicketClaims {
        sub: user_id.to_string(),
        media_id,
        resource,
        exp: expires_at.timestamp(),
        token_type: "media_access".to_string(),
    };
    let token = encode_signed_token(&claims, config, MEDIA_ACCESS_TICKET_CONTEXT)?;
    Ok((token, expires_at))
}

pub fn decode_media_access_ticket(token: &str, config: &Config) -> Option<MediaAccessTicketClaims> {
    let claims: MediaAccessTicketClaims =
        decode_signed_token(token, config, MEDIA_ACCESS_TICKET_CONTEXT)?;
    if claims.token_type != "media_access" {
        return None;
    }
    Some(claims)
}

pub fn create_share_session_token(
    share_id: i64,
    share_token: &str,
    share_expires_at: Option<chrono::DateTime<Utc>>,
    config: &Config,
) -> AppResult<(String, chrono::DateTime<Utc>)> {
    let configured_expiration =
        Utc::now() + Duration::hours(config.security.share_session_expire_hours);
    let expires_at = share_expires_at
        .map(|share_expiration| share_expiration.min(configured_expiration))
        .unwrap_or(configured_expiration);
    let claims = ShareSessionClaims {
        share_id,
        share_token_hash: hash_token(share_token),
        exp: expires_at.timestamp(),
        token_type: "share_session".to_string(),
    };
    let token = encode_signed_token(&claims, config, SHARE_SESSION_CONTEXT)?;
    Ok((token, expires_at))
}

pub fn decode_share_session_token(token: &str, config: &Config) -> Option<ShareSessionClaims> {
    let claims: ShareSessionClaims = decode_signed_token(token, config, SHARE_SESSION_CONTEXT)?;
    if claims.token_type != "share_session" {
        return None;
    }
    Some(claims)
}

pub fn share_token_hash(token: &str) -> String {
    hash_token(token)
}

pub fn hash_refresh_token(token: &str) -> String {
    hash_token(token)
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

fn encode_signed_token<T: Serialize>(
    claims: &T,
    config: &Config,
    context: &str,
) -> AppResult<String> {
    let signing_key = derive_signing_key(config, context);
    Ok(encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(&signing_key),
    )?)
}

fn decode_signed_token<T: DeserializeOwned>(
    token: &str,
    config: &Config,
    context: &str,
) -> Option<T> {
    let signing_key = derive_signing_key(config, context);
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 0;
    decode::<T>(token, &DecodingKey::from_secret(&signing_key), &validation)
        .ok()
        .map(|token_data| token_data.claims)
}

fn derive_signing_key(config: &Config, context: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(context.as_bytes());
    hasher.update([0]);
    hasher.update(config.security.secret_key.as_bytes());
    hasher.finalize().into()
}

// Add hex encoding dependency
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}
