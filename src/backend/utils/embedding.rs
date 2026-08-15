pub fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.is_empty() || left.len() != right.len() {
        return None;
    }
    let mut dot_product = 0.0_f32;
    let mut left_norm = 0.0_f32;
    let mut right_norm = 0.0_f32;
    for (left_component, right_component) in left.iter().zip(right) {
        dot_product += left_component * right_component;
        left_norm += left_component * left_component;
        right_norm += right_component * right_component;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return None;
    }
    Some(dot_product / (left_norm.sqrt() * right_norm.sqrt()))
}

pub fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|component| component.to_le_bytes())
        .collect()
}

pub fn blob_to_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}
