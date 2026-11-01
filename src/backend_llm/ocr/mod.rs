pub const UNLIMITED_OCR_MODEL: &str = "baidu/Unlimited-OCR-2";

pub fn normalize_unlimited_ocr_text(text: &str) -> String {
    let blocks = text
        .split("<BLOCK>")
        .map(|block| {
            block
                .split_once("<FORK>")
                .map(|(_, body)| body)
                .unwrap_or(block)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cleaned = blocks
        .replace("<PAGE>", "")
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
