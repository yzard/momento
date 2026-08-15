pub const UNLIMITED_OCR_MODEL: &str = "baidu/Unlimited-OCR";

pub fn normalize_unlimited_ocr_text(text: &str) -> String {
    let cleaned = text
        .replace("<|ref|>", "")
        .replace("<|/ref|>", "")
        .replace("<|det|>", "")
        .replace("<|/det|>", "")
        .trim()
        .to_string();
    if cleaned == "image [0, 0, 999, 999]" {
        return String::new();
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::normalize_unlimited_ocr_text;

    #[test]
    fn removes_unlimited_ocr_grounding_only_output() {
        assert_eq!(
            normalize_unlimited_ocr_text("<|det|>image [0, 0, 999, 999]<|/det|>"),
            ""
        );
    }

    #[test]
    fn preserves_non_empty_unlimited_ocr_text() {
        assert_eq!(
            normalize_unlimited_ocr_text("<|ref|>Title<|/ref|>"),
            "Title"
        );
    }
}
