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
    assert!(template["server"]
        .get("request_log_body_max_bytes")
        .is_none());
    assert_eq!(
        template["security"]["media_access_ticket_expire_hours"].as_integer(),
        Some(runtime_defaults.security.media_access_ticket_expire_hours)
    );
    assert_eq!(
        template["security"]["share_session_expire_hours"].as_integer(),
        Some(runtime_defaults.security.share_session_expire_hours)
    );
    assert_eq!(
        template["security"]["password_attempts_per_identity"].as_integer(),
        Some(i64::from(
            runtime_defaults.security.password_attempts_per_identity
        ))
    );
    assert_eq!(
        template["security"]["trusted_proxy_ip_addresses"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        template["media_process"]["maximum_decoded_image_pixels"].as_integer(),
        Some(runtime_defaults.media_process.maximum_decoded_image_pixels as i64)
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
    assert_eq!(
        template["llm"]["enabled"].as_bool(),
        Some(runtime_defaults.llm.enabled)
    );
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
        template["thread_pool"]["cpu_workers"].as_integer(),
        Some(runtime_defaults.thread_pool.cpu_workers as i64)
    );
    assert_eq!(
        template["thread_pool"]["io_workers"].as_integer(),
        Some(runtime_defaults.thread_pool.io_workers as i64)
    );
    assert_eq!(
        template["thread_pool"]["sqlite_workers"].as_integer(),
        Some(runtime_defaults.thread_pool.sqlite_workers as i64)
    );
    assert!(template.get("metadata_worker").is_none());
    assert!(template.get("llm_submission_worker").is_none());
    assert!(template.get("llm_result_worker").is_none());
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

#[test]
fn thumbnail_defaults_match_for_template_runtime_and_omitted_fields() {
    let template: toml::Value = toml::from_str(default_config_template()).expect("valid template");
    let runtime = Config::default();
    for (field, value, expected) in [
        (
            "thumbnails_max_size",
            i64::from(runtime.metadata.thumbnails_max_size),
            400,
        ),
        (
            "thumbnails_tiny_size",
            i64::from(runtime.metadata.thumbnails_tiny_size),
            48,
        ),
        (
            "thumbnails_quality",
            i64::from(runtime.metadata.thumbnails_quality),
            85,
        ),
    ] {
        assert_eq!(value, expected, "runtime default for {field}");
        assert_eq!(template["metadata"][field].as_integer(), Some(expected));
    }
    for (contents, quality) in [
        ("", 85),
        ("[metadata]\n", 85),
        ("[metadata]\nthumbnails_quality = 90\n", 90),
    ] {
        let config: Config = toml::from_str(contents).expect("omitted fields have defaults");
        assert_eq!(config.metadata.thumbnails_max_size, 400);
        assert_eq!(config.metadata.thumbnails_tiny_size, 48);
        assert_eq!(config.metadata.thumbnails_quality, quality);
    }
}

#[test]
fn all_generated_defaults_match_runtime_and_omitted_sections() {
    let mut expected = Config::default();
    // These four fields are explicit container environment bindings, not alternate defaults.
    expected.security.secret_key = "test-secret".to_string();
    expected.llm.api_key = "test-api-key".to_string();
    let resolved = momento_api::config::resolve_config_environment(
        default_config_template(),
        Some(&expected.llm.server_address),
        None,
        Some(&expected.security.secret_key),
        Some(&expected.llm.api_key),
    )
    .expect("resolve explicit deployment bindings");
    let generated: Config = toml::from_str(&resolved).expect("deserialize generated defaults");
    assert_eq!(
        toml::Value::try_from(generated).unwrap(),
        toml::Value::try_from(expected).unwrap()
    );
    let runtime = toml::Value::try_from(Config::default()).unwrap();
    let omitted: Config = toml::from_str("").unwrap();
    assert_eq!(toml::Value::try_from(omitted).unwrap(), runtime);
    for section in runtime
        .as_table()
        .unwrap()
        .keys()
        .filter(|section| section.as_str() != "thread_pool")
    {
        let config: Config = toml::from_str(&format!("[{section}]\n")).unwrap();
        assert_eq!(
            toml::Value::try_from(config).unwrap(),
            runtime,
            "empty {section}"
        );
    }
}
