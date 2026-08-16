use llm_service::config::Config;
use std::io::Write;
use tempfile::NamedTempFile;

fn local_ocr_configuration(extra: &str) -> String {
    format!(
        "[general]\n\n[[service]]\nenabled = true\nmodel_type = \"ocr\"\nmodel_version = \"unlimited_ocr\"\ndocker_command = [\"docker\", \"--gpus\", \"all\", \"--max-num-seqs\", \"{{max_concurrent_jobs}}\", \"{{runtime_mount_source}}\", \"{{runtime_mount_target}}\", \"readonly\"]\nbase_url = \"http://127.0.0.1:8000/v1\"\nmodel = \"test-model\"\ndevice = \"cuda\"\nmax_tokens = 1\nmax_concurrent_jobs = 1\n{extra}"
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
fn rejects_configurable_logging_path() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[logging]\nfile_path = \"/var/log/llm-service.log\"\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("Logging path must not be configurable");

    assert!(error.to_string().contains("logging"));
}

#[test]
fn rejects_local_ocr_without_model_command_configuration() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    let configuration = local_ocr_configuration("").replace(
        "docker_command = [\"docker\", \"--gpus\", \"all\", \"--max-num-seqs\", \"{max_concurrent_jobs}\", \"{runtime_mount_source}\", \"{runtime_mount_target}\", \"readonly\"]",
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
    assert_eq!(config.general.scheduler.max_in_flight_jobs, 128);
    assert_eq!(ocr.max_concurrent_jobs, 100);
    assert_eq!(tagging.max_concurrent_jobs, 16);
    assert_eq!(clustering.max_concurrent_jobs, 32);
    assert_eq!(face_detection.max_concurrent_jobs, 16);
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
    for service in [ocr, tagging, clustering, face_detection] {
        assert!(service.docker_command.iter().any(|argument| {
            argument.contains("source={runtime_mount_source}")
                && argument.contains("target={runtime_mount_target}")
                && argument.contains("readonly")
        }));
    }
    assert_eq!(clustering.model, "facebook/dinov2-small");
    assert_eq!(clustering.embedding_dimensions, 384);
    assert_eq!(face_detection.model, "buffalo_l");
    assert_eq!(face_detection.embedding_dimensions, 512);
    assert_eq!(face_detection.minimum_face_likelihood, Some(0.8));
    assert_eq!(face_detection.minimum_face_resolution_pixels, Some(112));
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
        "{}\n[general.scheduler]\nmax_in_flight_jobs = 0\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("zero in-flight window must be rejected");
    assert!(error.to_string().contains("max in-flight jobs"));
}

#[test]
fn rejects_removed_dispatch_batch_size() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[general.scheduler]\ndispatch_batch_size = 64\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("dispatch batch size must be rejected");

    assert!(error.to_string().contains("dispatch_batch_size"));
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

#[test]
fn rejects_relative_runtime_mount_target() {
    let mut file = NamedTempFile::new().expect("Failed to create config fixture");
    write!(
        file,
        "{}\n[storage]\nruntime_mount_target = \"relative-inputs\"\n",
        local_ocr_configuration("")
    )
    .expect("Failed to write config fixture");

    let error = Config::load(file.path()).expect_err("relative runtime target must be rejected");

    assert!(error.to_string().contains("absolute non-root"));
}
