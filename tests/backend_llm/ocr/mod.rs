use llm_service::ocr::normalize_unlimited_ocr_text;

#[test]
fn strips_page_and_block_metadata_but_keeps_recognized_content() {
    assert_eq!(normalize_unlimited_ocr_text("<PAGE><BLOCK>text[1, 2, 3, 4]<FORK>Invoice\n<BLOCK>table[2, 3, 4, 5]<FORK>| Amount |\n| 123.45 |"), "Invoice\n\n| Amount |\n| 123.45 |");
}

#[test]
fn preserves_plain_text_and_empty_success() {
    assert_eq!(normalize_unlimited_ocr_text("<PAGE>"), "");
    assert_eq!(
        normalize_unlimited_ocr_text("<|ref|>Title<|/ref|>"),
        "Title"
    );
    assert_eq!(
        normalize_unlimited_ocr_text("<|det|>image [0, 0, 999, 999]<|/det|>"),
        ""
    );
}
