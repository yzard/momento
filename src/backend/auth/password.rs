use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use std::sync::LazyLock;

static DUMMY_PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| {
    hash_password("momento-password-verification-placeholder")
        .expect("dummy password hash must be constructible")
});

/// Hash a password using Argon2id
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

/// Verify a password against an Argon2 hash.
pub fn verify_password(password: &str, hash: &str) -> bool {
    if !hash.starts_with("$argon2") {
        return false;
    }
    match PasswordHash::new(hash) {
        Ok(parsed_hash) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok(),
        Err(_) => false,
    }
}

/// Performs exactly one Argon2 verification even when no usable account hash exists.
pub fn verify_password_or_dummy(password: &str, hash: Option<&str>) -> bool {
    let usable_hash = hash.filter(|candidate| {
        candidate.starts_with("$argon2") && PasswordHash::new(candidate).is_ok()
    });
    let verified = verify_password(
        password,
        usable_hash.unwrap_or_else(|| DUMMY_PASSWORD_HASH.as_str()),
    );
    usable_hash.is_some() && verified
}
