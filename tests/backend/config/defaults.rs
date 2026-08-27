use momento_api::config::{default_config_template, Config};

#[test]
fn rendered_template_and_runtime_share_face_group_threshold() {
    let template: toml::Value =
        toml::from_str(default_config_template()).expect("rendered template must be valid TOML");
    let runtime_defaults = Config::default();

    assert_eq!(
        template["server"]["api_request_body_max_bytes"].as_integer(),
        Some(runtime_defaults.server.api_request_body_max_bytes as i64)
    );
    assert_eq!(
        template["server"]["request_log_body_max_bytes"].as_integer(),
        Some(runtime_defaults.server.request_log_body_max_bytes as i64)
    );
    assert_eq!(
        template["security"]["media_access_ticket_expire_hours"].as_integer(),
        Some(runtime_defaults.security.media_access_ticket_expire_hours)
    );
    assert_eq!(
        template["security"]["share_session_expire_hours"].as_integer(),
        Some(runtime_defaults.security.share_session_expire_hours)
    );
    assert_eq!(
        template["webdav"]["max_upload_bytes"].as_integer(),
        Some(50 * 1024 * 1024 * 1024)
    );
    assert_eq!(
        template["backup"]["max_upload_bytes"].as_integer(),
        Some(50 * 1024 * 1024 * 1024)
    );

    let template_threshold = template["face_group"]["similarity_threshold"]
        .as_float()
        .expect("template threshold");
    assert!(
        (template_threshold - f64::from(runtime_defaults.face_group.similarity_threshold)).abs()
            < f64::from(f32::EPSILON)
    );
    assert_eq!(template["llm"]["enabled"].as_bool(), Some(true));
    assert!(!runtime_defaults.llm.enabled);
    for removed_field in [
        "ocr_enabled",
        "image_tagging_enabled",
        "deduplicate_enabled",
        "face_detection_enabled",
        "image_aesthetics_enabled",
        "screenshot_detection_enabled",
        "document_detection_enabled",
    ] {
        assert!(template["llm"].get(removed_field).is_none());
    }
    for (field, runtime_weight) in [
        (
            "confidence_weight",
            runtime_defaults.face_group.confidence_weight,
        ),
        (
            "face_size_weight",
            runtime_defaults.face_group.face_size_weight,
        ),
        (
            "center_proximity_weight",
            runtime_defaults.face_group.center_proximity_weight,
        ),
        (
            "frontality_weight",
            runtime_defaults.face_group.frontality_weight,
        ),
        (
            "visibility_weight",
            runtime_defaults.face_group.visibility_weight,
        ),
        (
            "feature_clarity_weight",
            runtime_defaults.face_group.feature_clarity_weight,
        ),
    ] {
        assert_eq!(
            template["face_group"][field].as_float(),
            Some(runtime_weight)
        );
    }
    assert_eq!(
        template["llm_submission_worker"]["max_async_submission_tasks"].as_integer(),
        Some(
            runtime_defaults
                .llm_submission_worker
                .max_async_submission_tasks as i64
        )
    );
    assert!(template["llm_submission_worker"]
        .get("max_in_flight")
        .is_none());
    assert_eq!(
        template["llm_result_worker"]["concurrency"].as_integer(),
        Some(runtime_defaults.llm_result_worker.concurrency as i64)
    );
    assert!(template["llm_result_worker"]
        .get("cpu_processing_concurrency")
        .is_none());
    assert_eq!(
        template["llm"]["screenshot_detection_cron"].as_str(),
        Some("0 6 * * *")
    );
    assert_eq!(
        template["llm"]["document_detection_cron"].as_str(),
        Some("0 7 * * *")
    );
    assert!(!default_config_template().contains("{{"));
}
