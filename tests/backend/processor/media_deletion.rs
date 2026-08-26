use crate::test_utils::{
    create_test_db, create_test_media, create_test_user, grant_media_access, init_test_paths,
};
use momento_api::constants::{paths, OCR_MODEL_TYPE};
use momento_api::processor::media_deletion::permanently_delete_for_user;
use momento_api::processor::media_processor::insert_into_rtree;

#[test]
fn permanent_delete_cleans_every_media_owned_row() {
    init_test_paths();
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "owner", "owner@example.com");
    let media_id = create_test_media(&pool, "delete.jpg");
    grant_media_access(&pool, media_id, user_id);
    let file_path = format!("delete-{}.jpg", uuid::Uuid::new_v4());
    let original_path = paths().originals.join(&file_path);
    let sidecar_path = paths()
        .originals
        .join(format!("{file_path}.supplemental-metadata.json"));
    std::fs::write(&original_path, b"original").expect("original file");
    std::fs::write(&sidecar_path, b"{}").expect("supplemental metadata");
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute(
            "UPDATE media_access SET deleted_at = datetime('now') WHERE media_id = ? AND user_id = ?",
            rusqlite::params![media_id, user_id],
        )
        .expect("Failed to move media to trash");
    insert_into_rtree(&connection, media_id, 40.0, -74.0).expect("Failed to insert rtree");
    connection
        .execute(
            "INSERT INTO media_text (media_id, model_type, model_version, string) VALUES (?, ?, ?, ?)",
            rusqlite::params![media_id, OCR_MODEL_TYPE, "test", "text"],
        )
        .expect("Failed to insert canonical media text");
    connection
        .execute(
            "INSERT INTO media_similarity_index (media_id, content_hash, model_version, preprocessing_version, embedding, perceptual_hash) VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![media_id, format!("hash_{media_id}"), "model", "preprocess", vec![0_u8; 4], 1_i64],
        )
        .expect("Failed to insert similarity index");
    connection
        .execute(
            "UPDATE media_similarity_index SET embedding = X'', perceptual_hash = -1, processing_status = -1, processing_error = 'decode failed' WHERE media_id = ?",
            [media_id],
        )
        .expect("Failed to mark similarity failure");
    connection
        .execute(
            "INSERT INTO media_similarity_hash_bands (media_id, band_index, band_value) VALUES (?, 0, 1)",
            [media_id],
        )
        .expect("Failed to insert hash band");
    let cluster_id = {
        connection
            .execute(
                "INSERT INTO media_similarity_clusters (kind, representative_media_id) VALUES ('near_duplicate', ?)",
                [media_id],
            )
            .expect("Failed to insert cluster");
        connection.last_insert_rowid()
    };
    connection
        .execute(
            "INSERT INTO media_similarity_cluster_members (cluster_id, media_id, cosine_similarity, perceptual_hash_distance) VALUES (?, ?, 1.0, 0)",
            rusqlite::params![cluster_id, media_id],
        )
        .expect("Failed to insert cluster member");
    connection
        .execute(
            "INSERT INTO media_face_detection_results (media_id, model_type, model_version) VALUES (?, 'face_detection', 'buffalo_l')",
            [media_id],
        )
        .expect("Failed to insert face result");
    let crop_directory = paths().previews.join("faces").join(media_id.to_string());
    std::fs::create_dir_all(&crop_directory).expect("face crop directory");
    std::fs::write(crop_directory.join("face.jpg"), b"face").expect("face crop");
    connection.execute("INSERT INTO media_faces (media_id, input_sequence, face_index, x, y, width, height, confidence, face_size_score, frontality_score, visibility_score, feature_clarity_score, embedding, crop_path) VALUES (?, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, X'00000000', ?)", rusqlite::params![media_id, format!("faces/{media_id}/face.jpg")]).expect("Failed to insert face");
    let face_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO face_groups (representative_face_id) VALUES (?)",
            [face_id],
        )
        .expect("Failed to insert face group");
    let face_group_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO face_group_members (face_group_id, face_id, manual_anchor) VALUES (?, ?, 0)",
            [face_group_id, face_id],
        )
        .expect("Failed to insert face group member");

    assert!(
        permanently_delete_for_user(&connection, media_id, user_id, &file_path, None,)
            .expect("Permanent deletion should succeed")
    );

    for table in [
        "media",
        "media_metadata",
        "media_text",
        "media_rtree",
        "media_similarity_index",
        "media_similarity_hash_bands",
        "media_similarity_cluster_members",
        "media_similarity_clusters",
        "media_face_detection_results",
        "media_faces",
        "face_group_members",
        "face_groups",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("Cleanup query should succeed");
        assert_eq!(count, 0, "{table} was not cleaned");
    }
    assert!(!crop_directory.exists());
    assert!(!original_path.exists());
    assert!(!sidecar_path.exists());
}

#[test]
fn deleting_cluster_member_marks_remaining_member_dirty() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "owner2", "owner2@example.com");
    let deleted_media_id = create_test_media(&pool, "deleted.jpg");
    let remaining_media_id = create_test_media(&pool, "remaining.jpg");
    grant_media_access(&pool, deleted_media_id, user_id);
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute(
            "UPDATE media_access SET deleted_at = datetime('now') WHERE media_id = ? AND user_id = ?",
            rusqlite::params![deleted_media_id, user_id],
        )
        .expect("Failed to move media to trash");
    connection
        .execute("DELETE FROM media_similarity_dirty", [])
        .expect("Failed to reset dirty rows");
    connection
        .execute(
            "INSERT INTO media_similarity_clusters (kind, representative_media_id) VALUES ('burst', ?)",
            [deleted_media_id],
        )
        .expect("Failed to insert cluster");
    let cluster_id = connection.last_insert_rowid();
    for media_id in [deleted_media_id, remaining_media_id] {
        connection
            .execute(
                "INSERT INTO media_similarity_cluster_members (cluster_id, media_id, cosine_similarity, perceptual_hash_distance) VALUES (?, ?, 1.0, 0)",
                rusqlite::params![cluster_id, media_id],
            )
            .expect("Failed to insert cluster member");
    }

    permanently_delete_for_user(&connection, deleted_media_id, user_id, "missing.jpg", None)
        .expect("Permanent deletion should succeed");

    let cluster_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_clusters",
            [],
            |row| row.get(0),
        )
        .expect("Failed to count clusters");
    let dirty_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_dirty WHERE media_id = ?",
            [remaining_media_id],
            |row| row.get(0),
        )
        .expect("Failed to count dirty rows");
    assert_eq!(cluster_count, 0);
    assert_eq!(dirty_count, 1);
}

#[test]
fn permanent_delete_requires_trashed_access() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "active-owner", "active-owner@example.com");
    let media_id = create_test_media(&pool, "active.jpg");
    grant_media_access(&pool, media_id, user_id);
    let connection = pool.get().expect("Failed to get connection");

    assert!(
        !permanently_delete_for_user(&connection, media_id, user_id, "missing.jpg", None,)
            .expect("Active media deletion should be ignored")
    );

    let media_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media WHERE id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("Failed to count media");
    assert_eq!(media_count, 1);
}

#[test]
fn deleting_one_users_access_preserves_shared_similarity_index() {
    let pool = create_test_db();
    let deleting_user_id = create_test_user(&pool, "shared-a", "shared-a@example.com");
    let remaining_user_id = create_test_user(&pool, "shared-b", "shared-b@example.com");
    let media_id = create_test_media(&pool, "shared.jpg");
    grant_media_access(&pool, media_id, deleting_user_id);
    grant_media_access(&pool, media_id, remaining_user_id);
    let connection = pool.get().expect("Failed to get connection");
    connection
        .execute(
            "UPDATE media_access SET deleted_at = datetime('now') WHERE media_id = ? AND user_id = ?",
            rusqlite::params![media_id, deleting_user_id],
        )
        .expect("Failed to move shared media to trash");
    connection
        .execute(
            "INSERT INTO media_similarity_index (media_id, content_hash, model_version, preprocessing_version, embedding, perceptual_hash) VALUES (?, ?, 'model', 'preprocess', ?, 1)",
            rusqlite::params![media_id, format!("hash_{media_id}"), vec![0_u8; 4]],
        )
        .expect("Failed to insert similarity index");

    assert!(!permanently_delete_for_user(
        &connection,
        media_id,
        deleting_user_id,
        "missing.jpg",
        None,
    )
    .expect("Shared access deletion should succeed"));

    let index_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM media_similarity_index WHERE media_id = ?",
            [media_id],
            |row| row.get(0),
        )
        .expect("Failed to count similarity index");
    assert_eq!(index_count, 1);
}
