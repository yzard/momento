use momento_api::config::{default_config_template, Config};

#[test]
fn rendered_template_and_runtime_share_face_group_threshold() {
    let template: toml::Value =
        toml::from_str(default_config_template()).expect("rendered template must be valid TOML");
    let runtime_defaults = Config::default();

    assert_eq!(
        template["llm"]["face_group_similarity_threshold"].as_float(),
        Some(f64::from(
            runtime_defaults.llm.face_group_similarity_threshold
        ))
    );
    assert!(!default_config_template().contains("{{"));
}
