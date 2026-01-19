use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

/// Hash a password using Argon2id
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

/// Verify a password against a hash
/// Supports both Argon2 (new) and bcrypt (legacy) hashes
pub fn verify_password(password: &str, hash: &str) -> bool {
    // Try Argon2 first (new format starts with $argon2)
    if hash.starts_with("$argon2") {
        return verify_argon2(password, hash);
    }

    // Fall back to bcrypt (Python bcrypt format starts with $2b$)
    if hash.starts_with("$2") {
        return verify_bcrypt(password, hash);
    }

    false
}

fn verify_argon2(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed_hash) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok(),
        Err(_) => false,
    }
}

fn verify_bcrypt(password: &str, hash: &str) -> bool {
    bcrypt::verify(password, hash).unwrap_or(false)
}

/// Verify password and optionally migrate from bcrypt to Argon2
/// Returns (is_valid, new_hash_if_migrated)
pub fn verify_and_migrate(password: &str, hash: &str) -> (bool, Option<String>) {
    // If already Argon2, just verify
    if hash.starts_with("$argon2") {
        return (verify_argon2(password, hash), None);
    }

    // For bcrypt hashes, verify and migrate if successful
    if hash.starts_with("$2") && verify_bcrypt(password, hash) {
        match hash_password(password) {
            Ok(new_hash) => return (true, Some(new_hash)),
            Err(_) => return (true, None),
        }
    }

    (false, None)
}
