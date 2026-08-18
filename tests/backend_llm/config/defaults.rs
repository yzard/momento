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
