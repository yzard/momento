use std::path::PathBuf;

pub(crate) const SERVER_HOST: &str = "0.0.0.0";
pub(crate) const SERVER_PORT: u16 = 8100;
pub(crate) const SERVER_DATA_DIR: &str = "/data";
pub(crate) const SCHEDULER_POLL_INTERVAL_SECONDS: u64 = 5;
pub(crate) const SCHEDULER_IDLE_SHUTDOWN_SECONDS: u64 = 60;
pub(crate) const SCHEDULER_MAX_IN_FLIGHT_JOBS: usize = 128;
pub(crate) const SCHEDULER_RUNTIME_MAX_ATTEMPTS: usize = 3;
pub(crate) const RESULT_DELIVERY_ACKNOWLEDGEMENT_TIMEOUT_SECONDS: u64 = 30;
pub(crate) const RESULT_DELIVERY_RETRY_DELAY_SECONDS: u64 = 30;
pub(crate) const RESULT_DELIVERY_MAX_ATTEMPTS: usize = 10;
pub(crate) const RESULT_DELIVERY_MAX_CONCURRENT_DELIVERIES: usize = 16;
pub(crate) const SERVICE_ENABLED: bool = false;
pub(crate) const SERVICE_STARTUP_TIMEOUT_SECONDS: u64 = 300;
pub(crate) const SERVICE_REQUEST_TIMEOUT_SECONDS: u64 = 180;
pub(crate) const SERVICE_MAX_TOKENS: u32 = 8192;

pub(crate) mod fallback {
    pub(crate) const SERVER_API_KEY: &str = "";
}

mod template {
    pub(super) const SERVER_API_KEY: &str = "change-me-llm-service-key";
    pub(super) const SERVICE_ENABLED: bool = true;
    pub(super) const OCR_STARTUP_TIMEOUT_SECONDS: u64 = 1800;
    pub(super) const OCR_REQUEST_TIMEOUT_SECONDS: u64 = 1800;
    pub(super) const OCR_MAX_TOKENS: u32 = 8192;
    pub(super) const OCR_MAX_CONCURRENT_JOBS: usize = 100;
    pub(super) const IMAGE_TAGGING_STARTUP_TIMEOUT_SECONDS: u64 = 900;
    pub(super) const IMAGE_TAGGING_REQUEST_TIMEOUT_SECONDS: u64 = 180;
    pub(super) const IMAGE_TAGGING_MAX_CONCURRENT_JOBS: usize = 8;
    pub(super) const IMAGE_CLUSTERING_STARTUP_TIMEOUT_SECONDS: u64 = 900;
    pub(super) const IMAGE_CLUSTERING_REQUEST_TIMEOUT_SECONDS: u64 = 180;
    pub(super) const IMAGE_CLUSTERING_CPU_PROCESSING_CONCURRENCY: usize = 16;
    pub(super) const IMAGE_CLUSTERING_MODEL_CONCURRENCY: usize = 16;
    pub(super) const IMAGE_CLUSTERING_MODEL_BATCH_WAIT_MILLISECONDS: u64 = 5;
    pub(super) const IMAGE_AESTHETICS_STARTUP_TIMEOUT_SECONDS: u64 = 900;
    pub(super) const IMAGE_AESTHETICS_REQUEST_TIMEOUT_SECONDS: u64 = 180;
    pub(super) const IMAGE_AESTHETICS_CPU_PROCESSING_CONCURRENCY: usize = 16;
    pub(super) const IMAGE_AESTHETICS_MODEL_CONCURRENCY: usize = 64;
    pub(super) const IMAGE_AESTHETICS_MODEL_BATCH_WAIT_MILLISECONDS: u64 = 5;
    pub(super) const FACE_DETECTION_STARTUP_TIMEOUT_SECONDS: u64 = 900;
    pub(super) const FACE_DETECTION_REQUEST_TIMEOUT_SECONDS: u64 = 180;
    pub(super) const FACE_DETECTION_CPU_PROCESSING_CONCURRENCY: usize = 8;
    pub(super) const FACE_DETECTION_MODEL_CONCURRENCY: usize = 8;
    pub(super) const FACE_DETECTION_SIZE: u32 = 960;
    pub(super) const FACE_RECOGNITION_BATCH_SIZE: usize = 64;
    pub(super) const FACE_RECOGNITION_BATCH_WAIT_MILLISECONDS: u64 = 5;
    pub(super) const MINIMUM_FACE_LIKELIHOOD: f64 = 0.6;
    pub(super) const MINIMUM_FACE_RESOLUTION_PIXELS: u32 = 100;
    pub(super) const SCREENSHOT_DETECTION_STARTUP_TIMEOUT_SECONDS: u64 = 60;
    pub(super) const SCREENSHOT_DETECTION_REQUEST_TIMEOUT_SECONDS: u64 = 180;
    pub(super) const SCREENSHOT_DETECTION_CPU_PROCESSING_CONCURRENCY: usize = 8;
    pub(super) const SCREENSHOT_DETECTION_MODEL_CONCURRENCY: usize = 8;
    pub(super) const DOCUMENT_DETECTION_STARTUP_TIMEOUT_SECONDS: u64 = 60;
    pub(super) const DOCUMENT_DETECTION_REQUEST_TIMEOUT_SECONDS: u64 = 180;
    pub(super) const DOCUMENT_DETECTION_CPU_PROCESSING_CONCURRENCY: usize = 8;
    pub(super) const DOCUMENT_DETECTION_MODEL_CONCURRENCY: usize = 8;
}

pub(crate) fn server_host() -> String {
    SERVER_HOST.to_string()
}

pub(crate) fn server_port() -> u16 {
    SERVER_PORT
}

pub(crate) fn server_data_dir() -> PathBuf {
    PathBuf::from(SERVER_DATA_DIR)
}

pub(crate) fn server_api_key() -> String {
    fallback::SERVER_API_KEY.to_string()
}

pub(crate) fn scheduler_poll_interval_seconds() -> u64 {
    SCHEDULER_POLL_INTERVAL_SECONDS
}

pub(crate) fn scheduler_idle_shutdown_seconds() -> u64 {
    SCHEDULER_IDLE_SHUTDOWN_SECONDS
}

pub(crate) fn scheduler_max_in_flight_jobs() -> usize {
    SCHEDULER_MAX_IN_FLIGHT_JOBS
}

pub(crate) fn scheduler_runtime_max_attempts() -> usize {
    SCHEDULER_RUNTIME_MAX_ATTEMPTS
}

pub(crate) fn result_delivery_acknowledgement_timeout_seconds() -> u64 {
    RESULT_DELIVERY_ACKNOWLEDGEMENT_TIMEOUT_SECONDS
}

pub(crate) fn result_delivery_retry_delay_seconds() -> u64 {
    RESULT_DELIVERY_RETRY_DELAY_SECONDS
}

pub(crate) fn result_delivery_max_attempts() -> usize {
    RESULT_DELIVERY_MAX_ATTEMPTS
}

pub(crate) fn result_delivery_max_concurrent_deliveries() -> usize {
    RESULT_DELIVERY_MAX_CONCURRENT_DELIVERIES
}

pub(crate) fn service_enabled() -> bool {
    SERVICE_ENABLED
}

pub(crate) fn service_startup_timeout_seconds() -> u64 {
    SERVICE_STARTUP_TIMEOUT_SECONDS
}

pub(crate) fn service_request_timeout_seconds() -> u64 {
    SERVICE_REQUEST_TIMEOUT_SECONDS
}

pub(crate) fn service_max_tokens() -> u32 {
    SERVICE_MAX_TOKENS
}

pub(crate) fn render_template(source: &str) -> String {
    let replacements = [
        ("{{SERVER_HOST}}", SERVER_HOST.to_string()),
        ("{{SERVER_PORT}}", SERVER_PORT.to_string()),
        ("{{SERVER_API_KEY}}", template::SERVER_API_KEY.to_string()),
        ("{{SERVER_DATA_DIR}}", SERVER_DATA_DIR.to_string()),
        (
            "{{SCHEDULER_POLL_INTERVAL_SECONDS}}",
            SCHEDULER_POLL_INTERVAL_SECONDS.to_string(),
        ),
        (
            "{{SCHEDULER_IDLE_SHUTDOWN_SECONDS}}",
            SCHEDULER_IDLE_SHUTDOWN_SECONDS.to_string(),
        ),
        (
            "{{SCHEDULER_MAX_IN_FLIGHT_JOBS}}",
            SCHEDULER_MAX_IN_FLIGHT_JOBS.to_string(),
        ),
        (
            "{{SCHEDULER_RUNTIME_MAX_ATTEMPTS}}",
            SCHEDULER_RUNTIME_MAX_ATTEMPTS.to_string(),
        ),
        (
            "{{RESULT_DELIVERY_ACKNOWLEDGEMENT_TIMEOUT_SECONDS}}",
            RESULT_DELIVERY_ACKNOWLEDGEMENT_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{RESULT_DELIVERY_RETRY_DELAY_SECONDS}}",
            RESULT_DELIVERY_RETRY_DELAY_SECONDS.to_string(),
        ),
        (
            "{{RESULT_DELIVERY_MAX_ATTEMPTS}}",
            RESULT_DELIVERY_MAX_ATTEMPTS.to_string(),
        ),
        (
            "{{RESULT_DELIVERY_MAX_CONCURRENT_DELIVERIES}}",
            RESULT_DELIVERY_MAX_CONCURRENT_DELIVERIES.to_string(),
        ),
        ("{{SERVICE_ENABLED}}", template::SERVICE_ENABLED.to_string()),
        (
            "{{OCR_STARTUP_TIMEOUT_SECONDS}}",
            template::OCR_STARTUP_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{OCR_REQUEST_TIMEOUT_SECONDS}}",
            template::OCR_REQUEST_TIMEOUT_SECONDS.to_string(),
        ),
        ("{{OCR_MAX_TOKENS}}", template::OCR_MAX_TOKENS.to_string()),
        (
            "{{OCR_MAX_CONCURRENT_JOBS}}",
            template::OCR_MAX_CONCURRENT_JOBS.to_string(),
        ),
        (
            "{{IMAGE_TAGGING_STARTUP_TIMEOUT_SECONDS}}",
            template::IMAGE_TAGGING_STARTUP_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{IMAGE_TAGGING_REQUEST_TIMEOUT_SECONDS}}",
            template::IMAGE_TAGGING_REQUEST_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{IMAGE_TAGGING_MAX_CONCURRENT_JOBS}}",
            template::IMAGE_TAGGING_MAX_CONCURRENT_JOBS.to_string(),
        ),
        (
            "{{IMAGE_CLUSTERING_STARTUP_TIMEOUT_SECONDS}}",
            template::IMAGE_CLUSTERING_STARTUP_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{IMAGE_CLUSTERING_REQUEST_TIMEOUT_SECONDS}}",
            template::IMAGE_CLUSTERING_REQUEST_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{IMAGE_CLUSTERING_CPU_PROCESSING_CONCURRENCY}}",
            template::IMAGE_CLUSTERING_CPU_PROCESSING_CONCURRENCY.to_string(),
        ),
        (
            "{{IMAGE_CLUSTERING_MODEL_CONCURRENCY}}",
            template::IMAGE_CLUSTERING_MODEL_CONCURRENCY.to_string(),
        ),
        (
            "{{IMAGE_CLUSTERING_MODEL_BATCH_WAIT_MILLISECONDS}}",
            template::IMAGE_CLUSTERING_MODEL_BATCH_WAIT_MILLISECONDS.to_string(),
        ),
        (
            "{{IMAGE_AESTHETICS_STARTUP_TIMEOUT_SECONDS}}",
            template::IMAGE_AESTHETICS_STARTUP_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{IMAGE_AESTHETICS_REQUEST_TIMEOUT_SECONDS}}",
            template::IMAGE_AESTHETICS_REQUEST_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{IMAGE_AESTHETICS_CPU_PROCESSING_CONCURRENCY}}",
            template::IMAGE_AESTHETICS_CPU_PROCESSING_CONCURRENCY.to_string(),
        ),
        (
            "{{IMAGE_AESTHETICS_MODEL_CONCURRENCY}}",
            template::IMAGE_AESTHETICS_MODEL_CONCURRENCY.to_string(),
        ),
        (
            "{{IMAGE_AESTHETICS_MODEL_BATCH_WAIT_MILLISECONDS}}",
            template::IMAGE_AESTHETICS_MODEL_BATCH_WAIT_MILLISECONDS.to_string(),
        ),
        (
            "{{FACE_DETECTION_STARTUP_TIMEOUT_SECONDS}}",
            template::FACE_DETECTION_STARTUP_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{FACE_DETECTION_REQUEST_TIMEOUT_SECONDS}}",
            template::FACE_DETECTION_REQUEST_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{FACE_DETECTION_CPU_PROCESSING_CONCURRENCY}}",
            template::FACE_DETECTION_CPU_PROCESSING_CONCURRENCY.to_string(),
        ),
        (
            "{{FACE_DETECTION_MODEL_CONCURRENCY}}",
            template::FACE_DETECTION_MODEL_CONCURRENCY.to_string(),
        ),
        (
            "{{FACE_DETECTION_SIZE}}",
            template::FACE_DETECTION_SIZE.to_string(),
        ),
        (
            "{{FACE_RECOGNITION_BATCH_SIZE}}",
            template::FACE_RECOGNITION_BATCH_SIZE.to_string(),
        ),
        (
            "{{FACE_RECOGNITION_BATCH_WAIT_MILLISECONDS}}",
            template::FACE_RECOGNITION_BATCH_WAIT_MILLISECONDS.to_string(),
        ),
        (
            "{{MINIMUM_FACE_LIKELIHOOD}}",
            format!("{:.2}", template::MINIMUM_FACE_LIKELIHOOD),
        ),
        (
            "{{MINIMUM_FACE_RESOLUTION_PIXELS}}",
            template::MINIMUM_FACE_RESOLUTION_PIXELS.to_string(),
        ),
        (
            "{{SCREENSHOT_DETECTION_STARTUP_TIMEOUT_SECONDS}}",
            template::SCREENSHOT_DETECTION_STARTUP_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{SCREENSHOT_DETECTION_REQUEST_TIMEOUT_SECONDS}}",
            template::SCREENSHOT_DETECTION_REQUEST_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{SCREENSHOT_DETECTION_CPU_PROCESSING_CONCURRENCY}}",
            template::SCREENSHOT_DETECTION_CPU_PROCESSING_CONCURRENCY.to_string(),
        ),
        (
            "{{SCREENSHOT_DETECTION_MODEL_CONCURRENCY}}",
            template::SCREENSHOT_DETECTION_MODEL_CONCURRENCY.to_string(),
        ),
        (
            "{{DOCUMENT_DETECTION_STARTUP_TIMEOUT_SECONDS}}",
            template::DOCUMENT_DETECTION_STARTUP_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{DOCUMENT_DETECTION_REQUEST_TIMEOUT_SECONDS}}",
            template::DOCUMENT_DETECTION_REQUEST_TIMEOUT_SECONDS.to_string(),
        ),
        (
            "{{DOCUMENT_DETECTION_CPU_PROCESSING_CONCURRENCY}}",
            template::DOCUMENT_DETECTION_CPU_PROCESSING_CONCURRENCY.to_string(),
        ),
        (
            "{{DOCUMENT_DETECTION_MODEL_CONCURRENCY}}",
            template::DOCUMENT_DETECTION_MODEL_CONCURRENCY.to_string(),
        ),
    ];

    replacements
        .into_iter()
        .fold(source.to_string(), |rendered, (placeholder, value)| {
            rendered.replace(placeholder, &value)
        })
}
