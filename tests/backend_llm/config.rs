use llm_service::config::{Config, ProviderKind};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn loads_baidu_provider_from_config_llm_toml() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "[general]\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmodel_version = \"unlimited_ocr\"\nprovider = \"baidu\"\napi_key = \"test-ak\"\nsecret_key = \"test-sk\"\n"
    )
    .expect("Failed to write config fixture");

    let config = Config::load(file.path()).expect("Config should load");

    assert_eq!(config.service[0].provider, ProviderKind::Baidu);
    assert_eq!(config.service[0].api_key, "test-ak");
}

#[test]
fn rejects_local_provider_without_model_command_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "[general]\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmodel_version = \"unlimited_ocr\"\nprovider = \"local\"\ndocker_command = []\nbase_url = \"http://127.0.0.1:8000/v1\"\nmodel = \"test-model\"\n"
    )
    .expect("Failed to write config fixture");

    assert!(Config::load(file.path()).is_err());
}

#[test]
fn loads_playground_toml_configuration() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../playground/config_llm.toml");
    if !path.exists() {
        return;
    }

    let config = Config::load(&path).expect("Playground TOML configuration should load");

    assert_eq!(config.service[0].provider, ProviderKind::Local);
    assert_eq!(
        config.service_for("image_tagging").unwrap().model_version,
        "ram++"
    );
    let clustering = config.service_for("image_clustering").unwrap();
    assert_eq!(clustering.model, "facebook/dinov2-small");
    assert_eq!(clustering.embedding_dimensions, 384);
    assert_eq!(
        config.logging.file_path,
        std::path::PathBuf::from("playground/logs/llm-service.log")
    );
}

#[test]
fn rejects_non_positive_scheduler_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "[general.scheduler]\npoll_interval_seconds = 0\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmodel_version = \"unlimited_ocr\"\nprovider = \"baidu\"\napi_key = \"test-ak\"\nsecret_key = \"test-sk\"\n"
    )
    .expect("Failed to write config fixture");

    assert!(Config::load(file.path()).is_err());
}

#[test]
fn rejects_image_clustering_with_wrong_embedding_dimensions() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "[general]\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmodel_version = \"ocr\"\nprovider = \"baidu\"\napi_key = \"key\"\nsecret_key = \"secret\"\n\n[[service]]\nenabled = true\nmodel_type = \"image_clustering\"\nmodel_version = \"dinov2-small\"\nprovider = \"local\"\ndocker_command = [\"python3\"]\nbase_url = \"http://127.0.0.1:8300\"\nmodel = \"facebook/dinov2-small\"\nscript_path = \"image_clustering_server.py\"\nembedding_dimensions = 768\n"
    )
    .expect("Failed to write config fixture");

    assert!(Config::load(file.path()).is_err());
}
