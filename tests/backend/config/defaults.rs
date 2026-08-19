use momento_api::config::{default_config_template, Config};

#[test]
fn rendered_template_and_runtime_share_face_group_threshold() {
    let template: toml::Value =
        toml::from_str(default_config_template()).expect("rendered template must be valid TOML");
    let runtime_defaults = Config::default();

    let template_threshold = template["llm"]["face_group_similarity_threshold"]
        .as_float()
        .expect("template threshold");
    assert!(
        (template_threshold - f64::from(runtime_defaults.llm.face_group_similarity_threshold))
            .abs()
            < f64::from(f32::EPSILON)
    );
    assert!(template["llm"]["screenshot_detection_enabled"]
        .as_bool()
        .expect("template screenshot detection enablement"));
    assert!(template["llm"]["document_detection_enabled"]
        .as_bool()
        .expect("template document detection enablement"));
    assert!(!runtime_defaults.llm.screenshot_detection_enabled);
    assert!(!runtime_defaults.llm.document_detection_enabled);
    assert_eq!(
        template["cronjob"]["screenshot_detection_cron"].as_str(),
        Some("0 6 * * *")
    );
    assert_eq!(
        template["cronjob"]["document_detection_cron"].as_str(),
        Some("0 7 * * *")
    );
    assert!(!default_config_template().contains("{{"));
}
