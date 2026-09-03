use momento_api::database::result_footprint::{
    ResultFootprintError, SqliteFootprintRegistry, MAX_DURABLE_ERROR_BYTES,
    MAX_LLM_RESULT_PERSIST_BATCH_BYTES,
};

#[test]
fn result_footprint_registry_is_exhaustive_and_page_aligned() {
    let registry = SqliteFootprintRegistry::new(4096).expect("footprint registry");
    let tasks = registry.supported_tasks().collect::<Vec<_>>();
    assert_eq!(tasks, momento_common::llm::LLM_TASKS);
    assert!(registry.result_rejection_max_growth_bytes >= MAX_DURABLE_ERROR_BYTES);
    assert!(
        registry.result_cleanup_recovery_max_growth_bytes >= MAX_LLM_RESULT_PERSIST_BATCH_BYTES
    );
    for task in tasks {
        let footprint = registry.result(task, 3, 4096).expect("task footprint");
        assert_eq!(footprint.construction_max_growth_bytes % 4096, 0);
        assert_eq!(footprint.cleanup_recovery_max_growth_bytes % 4096, 0);
        assert!(
            footprint.construction_max_growth_bytes > footprint.cleanup_recovery_max_growth_bytes
        );
    }
}

#[test]
fn result_footprints_are_monotonic_and_face_includes_artifact_plans() {
    let registry = SqliteFootprintRegistry::new(4096).expect("footprint registry");
    let small = registry.result("face_detection", 3, 4096).expect("small");
    let more_records = registry
        .result("face_detection", 4, 4096)
        .expect("more records");
    let more_bytes = registry
        .result("face_detection", 4, 8192)
        .expect("more bytes");
    let classifier = registry
        .result("screenshot_detection", 4, 8192)
        .expect("classifier");
    assert!(more_records.construction_max_growth_bytes > small.construction_max_growth_bytes);
    assert!(more_bytes.construction_max_growth_bytes >= more_records.construction_max_growth_bytes);
    assert!(more_bytes.construction_max_growth_bytes > classifier.construction_max_growth_bytes);
}

#[test]
fn persistence_footprints_are_page_aligned_and_task_derived() {
    let registry = SqliteFootprintRegistry::new(4096).expect("footprint registry");
    let text = registry.persistence("ocr", 32).expect("text footprint");
    let faces = registry
        .persistence("face_detection", 32)
        .expect("face footprint");

    assert_eq!(text % 4096, 0);
    assert_eq!(faces % 4096, 0);
    assert!(faces > text);
    assert!(registry.persistence("unknown", 1).is_err());
    assert!(registry.persistence("ocr", 0).is_err());
}

#[test]
fn result_footprint_rejects_unknown_or_invalid_inputs() {
    assert_eq!(
        SqliteFootprintRegistry::new(1000),
        Err(ResultFootprintError::InvalidPageSize)
    );
    let registry = SqliteFootprintRegistry::new(4096).expect("footprint registry");
    assert_eq!(
        registry.result("unknown", 1, 24),
        Err(ResultFootprintError::UnknownTask)
    );
    assert_eq!(
        registry.result("ocr", 0, 24),
        Err(ResultFootprintError::InvalidManifest)
    );
    assert_eq!(
        registry.result("ocr", 1, 23),
        Err(ResultFootprintError::InvalidManifest)
    );
}
