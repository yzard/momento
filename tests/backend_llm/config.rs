use llm_service::config::Config;
use std::io::Write;
use tempfile::NamedTempFile;

fn local_ocr_configuration(extra: &str) -> String {
    format!(
        "[general]\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmodel_version = \"unlimited_ocr\"\ndocker_command = [\"docker\", \"--gpus\", \"all\", \"--max-num-seqs\", \"{{max_concurrent_jobs}}\"]\nbase_url = \"http://127.0.0.1:8000/v1\"\nmodel = \"test-model\"\ndevice = \"cuda\"\nmax_tokens = 1\nmax_concurrent_jobs = 1\n{extra}"
    )
}

#[test]
fn loads_local_ocr_runtime_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(file, "{}", local_ocr_configuration("")).expect("Failed to write config fixture");

    let config = Config::load(file.path()).expect("Config should load");

    assert_eq!(config.service[0].model, "test-model");
}

#[test]
fn rejects_local_ocr_without_model_command_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    let configuration = local_ocr_configuration("").replace(
        "docker_command = [\"docker\", \"--gpus\", \"all\", \"--max-num-seqs\", \"{max_concurrent_jobs}\"]",
        "docker_command = []",
    );
    write!(file, "{configuration}").expect("Failed to write config fixture");

    assert!(Config::load(file.path()).is_err());
}

#[test]
fn rejects_legacy_remote_provider_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    let configuration = local_ocr_configuration("").replace(
        "enabled = true",
        "enabled = true\nprovider = \"local\"\napi_key = \"legacy\"",
    );
    write!(file, "{configuration}").expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("Legacy provider fields must be rejected");

    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn loads_playground_toml_configuration() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../playground/config_llm.toml");
    if !path.exists() {
        return;
    }

    let config = Config::load(&path).expect("Playground TOML configuration should load");

    assert_eq!(
        config.service_for("image_tagging").unwrap().model_version,
        "ram++"
    );
    let clustering = config.service_for("image_clustering").unwrap();
    let face_detection = config.service_for("face_detection").unwrap();
    let tagging = config.service_for("image_tagging").unwrap();
    let ocr = config.service_for("ocr").unwrap();
    assert_eq!(config.general.scheduler.dispatch_batch_size, 64);
    assert_eq!(ocr.max_concurrent_jobs, 100);
    assert_eq!(tagging.max_concurrent_jobs, 16);
    assert_eq!(clustering.max_concurrent_jobs, 32);
    assert_eq!(face_detection.max_concurrent_jobs, 32);
    assert_eq!(config.callback.max_concurrent_deliveries, 16);
    assert!(ocr
        .docker_command
        .iter()
        .any(|argument| argument == "--max-num-seqs"));
    assert!(ocr
        .docker_command
        .iter()
        .any(|argument| argument == "{max_concurrent_jobs}"));
    for service in [tagging, clustering, face_detection] {
        assert!(service
            .docker_command
            .iter()
            .any(|argument| argument.contains("--max-concurrent-jobs {max_concurrent_jobs}")));
    }
    assert_eq!(clustering.model, "facebook/dinov2-small");
    assert_eq!(clustering.embedding_dimensions, 384);
    assert_eq!(face_detection.model, "buffalo_l");
    assert_eq!(face_detection.embedding_dimensions, 512);
}

#[test]
fn rejects_local_gpu_service_without_cuda_device() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    let configuration =
        local_ocr_configuration("").replace("device = \"cuda\"", "device = \"cpu\"");
    write!(file, "{configuration}").expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("CPU device must be rejected");
    assert!(error.to_string().contains("CUDA GPU"));
}

#[test]
fn rejects_non_positive_scheduler_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[general.scheduler]\ndispatch_batch_size = 0\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("zero batch size must be rejected");
    assert!(error.to_string().contains("dispatch batch size"));
}

#[test]
fn rejects_enabled_service_without_concurrency_limit() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    let configuration =
        local_ocr_configuration("").replace("max_concurrent_jobs = 1", "max_concurrent_jobs = 0");
    write!(file, "{configuration}").expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("zero limit must be rejected");
    assert!(error.to_string().contains("max_concurrent_jobs"));
}
