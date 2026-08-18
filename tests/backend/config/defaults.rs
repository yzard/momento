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
    assert!(!default_config_template().contains("{{"));
}
