use momento_api::processor::ai::input::AiInputStorage;

#[test]
fn parses_only_owned_ai_input_storage_roots() {
    assert_eq!(
        AiInputStorage::parse("originals").expect("originals root"),
        AiInputStorage::Originals
    );
    assert_eq!(
        AiInputStorage::parse("previews").expect("previews root"),
        AiInputStorage::Previews
    );
    assert!(AiInputStorage::parse("/shared/data").is_err());
    assert!(AiInputStorage::parse("thumbnails").is_err());
}
