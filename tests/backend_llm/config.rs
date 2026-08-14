use llm_service::config::{Config, ProviderKind};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn loads_baidu_provider_from_config_llm_toml() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "[general]\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmodel_version = \"unlimited_ocr\"\nprovider = \"baidu\"\napi_key = \"test-ak\"\nsecret_key = \"test-sk\"\nmax_concurrent_jobs = 1\n"
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
    let tagging = config.service_for("image_tagging").unwrap();
    let ocr = config.service_for("ocr").unwrap();
    assert_eq!(tagging.device, "cuda");
    assert_eq!(clustering.device, "cuda");
    assert_eq!(ocr.max_concurrent_jobs, 100);
    assert_eq!(tagging.max_concurrent_jobs, 16);
    assert_eq!(clustering.max_concurrent_jobs, 32);
    assert!(ocr
        .docker_command
        .iter()
        .any(|argument| argument == "--gpus"));
    assert!(!ocr
        .docker_command
        .iter()
        .any(|argument| argument == "--device"));
    assert!(ocr.docker_command.iter().any(|argument| argument == "8400"));
    assert_eq!(ocr.base_url, "http://127.0.0.1:8400/v1");
    assert_eq!(clustering.model, "facebook/dinov2-small");
    assert_eq!(clustering.embedding_dimensions, 384);
    assert!(clustering
        .docker_command
        .iter()
        .any(|argument| argument.contains("transformers==4.46.3")));
    assert!(clustering
        .docker_command
        .iter()
        .any(|argument| argument.contains("--reinstall-package transformers")));
    assert_eq!(
        config.logging.file_path,
        std::path::PathBuf::from("playground/logs/llm-service.log")
    );
}

#[test]
fn rejects_local_gpu_service_without_cuda_device() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "[general]\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmodel_version = \"unlimited_ocr\"\nprovider = \"local\"\ndocker_command = [\"docker\", \"--gpus\", \"all\"]\nbase_url = \"http://127.0.0.1:8000/v1\"\nmodel = \"test-model\"\ndevice = \"cpu\"\nmax_tokens = 1\nmax_concurrent_jobs = 1\n"
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("CPU device must be rejected");
    assert!(error.to_string().contains("CUDA GPU"));
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
fn rejects_non_positive_idle_shutdown_timeout() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "[general.scheduler]\nidle_shutdown_seconds = 0\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmodel_version = \"unlimited_ocr\"\nprovider = \"baidu\"\napi_key = \"test-ak\"\nsecret_key = \"test-sk\"\nmax_concurrent_jobs = 1\n"
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("zero timeout must be rejected");
    assert!(error.to_string().contains("idle shutdown"));
}

#[test]
fn rejects_enabled_service_without_concurrency_limit() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "[general]\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmodel_version = \"unlimited_ocr\"\nprovider = \"baidu\"\napi_key = \"test-ak\"\nsecret_key = \"test-sk\"\n"
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("missing limit must be rejected");
    assert!(error.to_string().contains("max_concurrent_jobs"));
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
