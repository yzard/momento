use crate::config::Config;
use crate::error::AppResult;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub role: String,
    pub exp: i64,
    #[serde(rename = "type")]
    pub token_type: String,
}

pub fn create_access_token(user_id: i64, username: &str, role: &str, config: &Config) -> AppResult<String> {
    let expiration = Utc::now() + Duration::minutes(config.security.access_token_expire_minutes);

    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        role: role.to_string(),
        exp: expiration.timestamp(),
        token_type: "access".to_string(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.security.secret_key.as_bytes()),
    )?;

    Ok(token)
}

pub fn create_refresh_token(_user_id: i64, config: &Config) -> (String, String, chrono::DateTime<Utc>) {
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
    let validation = Validation::default();

    match decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.security.secret_key.as_bytes()),
        &validation,
    ) {
        Ok(data) => {
            if data.claims.token_type == "access" {
                Some(data.claims)
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

pub fn hash_refresh_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
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
