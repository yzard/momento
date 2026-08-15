use momento_api::utils::embedding::{blob_to_embedding, cosine_similarity, embedding_to_blob};

#[test]
fn float32_embedding_blob_round_trips() {
    let embedding = vec![0.25, -1.5, 3.0];

    assert_eq!(blob_to_embedding(&embedding_to_blob(&embedding)), embedding);
}

#[test]
fn cosine_similarity_rejects_invalid_vectors() {
    assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]), Some(1.0));
    assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]), None);
    assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), None);
}
