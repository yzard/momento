use argon2::{password_hash::PasswordHash, Params};
use momento_api::auth::{
    hash_password, verify_password, ARGON2_ITERATIONS, ARGON2_MEMORY_KIB, ARGON2_PARALLELISM,
    MAX_AUTH_PASSWORD_BYTES,
};

#[test]
fn password_hashing_uses_only_the_source_owned_argon2id_parameters() {
    let hash = hash_password("bounded-password").expect("password hash");
    let parsed = PasswordHash::new(&hash).expect("encoded password hash");
    let parameters = Params::try_from(&parsed).expect("Argon2 parameters");

    assert_eq!(parsed.algorithm.as_str(), "argon2id");
    assert_eq!(parsed.version, Some(19));
    assert_eq!(parameters.m_cost(), ARGON2_MEMORY_KIB);
    assert_eq!(parameters.t_cost(), ARGON2_ITERATIONS);
    assert_eq!(parameters.p_cost(), ARGON2_PARALLELISM);
    assert_eq!(parameters.output_len(), Some(32));
    assert!(verify_password("bounded-password", &hash));

    for incompatible in [
        hash.replace("m=19456", "m=19457"),
        hash.replace("t=2", "t=3"),
        hash.replace("p=1", "p=2"),
        hash.replace("argon2id", "argon2i"),
    ] {
        assert!(!verify_password("bounded-password", &incompatible));
    }
}

#[test]
fn password_input_bound_is_enforced_at_1024_bytes() {
    let maximum = "a".repeat(MAX_AUTH_PASSWORD_BYTES);
    let oversized = "a".repeat(MAX_AUTH_PASSWORD_BYTES + 1);
    let hash = hash_password(&maximum).expect("maximum-size password");

    assert!(verify_password(&maximum, &hash));
    assert!(hash_password(&oversized).is_err());
    assert!(!verify_password(&oversized, &hash));
}
