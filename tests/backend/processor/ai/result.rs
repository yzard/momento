use super::*;

#[test]
fn staging_detaches_only_bounded_non_face_results() {
    for task in [
        "ocr",
        "image_tagging",
        "image_aesthetics",
        "image_clustering",
        "screenshot_detection",
        "document_detection",
    ] {
        assert!(can_detach_result_staging(task, 8192, 32));
        assert!(!can_detach_result_staging(task, 8193, 32));
        assert!(!can_detach_result_staging(task, 8192, 33));
    }
    assert!(!can_detach_result_staging("face_detection", 24, 1));
}
