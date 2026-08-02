use llm_service::config::{Config, ProviderKind};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn loads_baidu_provider_from_config_llm_yaml() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "provider: baidu\nbaidu:\n  api_key: test-ak\n  secret_key: test-sk\n"
    )
    .expect("Failed to write config fixture");

    let config = Config::load(file.path()).expect("Config should load");

    assert_eq!(config.provider, ProviderKind::Baidu);
    assert_eq!(config.baidu.api_key, "test-ak");
}

#[test]
fn rejects_local_provider_without_model_command_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "provider: local\nlocal:\n  command: ''\n  model: test-model\n"
    )
    .expect("Failed to write config fixture");

    assert!(Config::load(file.path()).is_err());
}
