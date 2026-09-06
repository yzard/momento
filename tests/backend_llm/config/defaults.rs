use llm_service::config::{default_config_template, Config};

#[test]
fn rendered_template_uses_central_scheduler_defaults() {
    let template: toml::Value =
        toml::from_str(default_config_template()).expect("rendered template must be valid TOML");
    let runtime_defaults = Config::default();

    assert_eq!(
        template["scheduler"]["max_in_flight_jobs"].as_integer(),
        Some(runtime_defaults.scheduler.max_in_flight_jobs as i64)
    );
    assert!(!default_config_template().contains("{{"));
}

#[test]
fn all_service_defaults_match_the_generated_model_configuration() {
    let template: toml::Value = toml::from_str(default_config_template()).unwrap();
    let services = template["service"].as_array().unwrap();
    assert_eq!(services.len(), 7);
    for service in services {
        let expected: llm_service::config::ServiceConfig = service.clone().try_into().unwrap();
        let mut omitted = service.clone();
        for field in [
            "enabled",
            "startup_timeout_seconds",
            "request_timeout_seconds",
            "max_tokens",
        ] {
            omitted.as_table_mut().unwrap().remove(field);
        }
        let actual: llm_service::config::ServiceConfig = omitted.try_into().unwrap();
        assert_eq!(
            toml::Value::try_from(actual).unwrap(),
            toml::Value::try_from(expected).unwrap()
        );
    }
}

#[test]
fn explicit_service_values_override_defaults_and_unknown_fields_are_rejected() {
    use llm_service::config::ServiceConfig;
    let service: ServiceConfig = toml::from_str(
        "model_type = \"ocr\"\nenabled = false\nstartup_timeout_seconds = 0\nrequest_timeout_seconds = 2\nmax_tokens = 3\n"
    ).unwrap();
    assert!(!service.enabled);
    assert_eq!(service.startup_timeout_seconds, 0);
    assert_eq!(service.request_timeout_seconds, 2);
    assert_eq!(service.max_tokens, 3);
    assert!(toml::from_str::<ServiceConfig>("model_type = \"ocr\"\nunknown = 1").is_err());
    assert!(toml::from_str::<ServiceConfig>("enabled = true").is_err());
    assert!(
        toml::from_str::<ServiceConfig>("model_type = \"ocr\"\nstartup_timeout_seconds = -1")
            .is_err()
    );
    let unknown: ServiceConfig =
        toml::from_str("model_type = \"unknown\"\nenabled = false").unwrap();
    assert_eq!(unknown.startup_timeout_seconds, 300);
    assert_eq!(unknown.request_timeout_seconds, 180);
}

#[test]
fn all_server_and_scheduler_defaults_match_the_generated_configuration() {
    use llm_service::config::{SchedulerConfig, ServerConfig};
    let template: toml::Value = toml::from_str(default_config_template()).unwrap();
    let server: ServerConfig = template["server"].clone().try_into().unwrap();
    let default_server = ServerConfig::default();
    assert_eq!(server.host, default_server.host);
    assert_eq!(server.port, default_server.port);
    assert_eq!(server.data_dir, default_server.data_dir);
    // The generated API key is an explicit deployment binding; omission requires configuration.
    assert_eq!(server.api_key, "${LLM_SERVICE_API_KEY}");
    assert!(default_server.api_key.is_empty());
    let omitted_server: ServerConfig = toml::from_str("").unwrap();
    assert_eq!(omitted_server.host, default_server.host);
    assert_eq!(omitted_server.port, default_server.port);
    assert_eq!(omitted_server.data_dir, default_server.data_dir);
    assert_eq!(omitted_server.api_key, default_server.api_key);
    let expected = toml::Value::try_from(SchedulerConfig::default()).unwrap();
    assert_eq!(template["scheduler"], expected);
    let mut omitted = template["scheduler"].clone();
    omitted
        .as_table_mut()
        .unwrap()
        .retain(|field, _| matches!(field, "max_queue_bytes" | "working_space_reserve_bytes"));
    let scheduler: SchedulerConfig = omitted.try_into().unwrap();
    assert_eq!(toml::Value::try_from(scheduler).unwrap(), expected);
}
