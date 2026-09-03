use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};

pub const MAX_AUTH_PASSWORD_BYTES: usize = 1024;
pub const ARGON2_MEMORY_KIB: u32 = 19_456;
pub const ARGON2_ITERATIONS: u32 = 2;
pub const ARGON2_PARALLELISM: u32 = 1;
const ARGON2_OUTPUT_BYTES: usize = 32;

fn configured_argon2() -> Argon2<'static> {
    let parameters = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        Some(ARGON2_OUTPUT_BYTES),
    )
    .expect("source-owned Argon2 parameters are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, parameters)
}

pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    if password.len() > MAX_AUTH_PASSWORD_BYTES {
        return Err(argon2::password_hash::Error::Password);
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = configured_argon2().hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    if password.len() > MAX_AUTH_PASSWORD_BYTES {
        return false;
    }
    let Ok(parsed_hash) = PasswordHash::new(hash) else {
        return false;
    };
    if !has_exact_parameters(&parsed_hash) {
        return false;
    }
    configured_argon2()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

pub(crate) fn verify_password_or_dummy(
    password: &str,
    hash: Option<&str>,
    dummy_hash: &str,
) -> bool {
    let usable_hash = hash.filter(|candidate| {
        PasswordHash::new(candidate)
            .ok()
            .is_some_and(|parsed| has_exact_parameters(&parsed))
    });
    let verified = verify_password(password, usable_hash.unwrap_or(dummy_hash));
    usable_hash.is_some() && verified
}

fn has_exact_parameters(hash: &PasswordHash<'_>) -> bool {
    if hash.algorithm.as_str() != "argon2id" || hash.version != Some(19) {
        return false;
    }
    let Ok(parameters) = Params::try_from(hash) else {
        return false;
    };
    parameters.m_cost() == ARGON2_MEMORY_KIB
        && parameters.t_cost() == ARGON2_ITERATIONS
        && parameters.p_cost() == ARGON2_PARALLELISM
        && parameters.output_len() == Some(ARGON2_OUTPUT_BYTES)
}
