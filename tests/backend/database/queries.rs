use momento_api::database::queries;

use crate::test_utils::{create_test_db, create_test_media, create_test_user, grant_media_access};

#[test]
fn import_recovery_preserves_active_owners_and_terminal_media() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "recover.jpg");
    let connection = pool.get().unwrap();
    let hash = "a".repeat(64);
    connection.execute("UPDATE media SET import_state='importing', import_source_root='imports', import_source_path='nested/recover.jpg', content_hash=? WHERE id=?", rusqlite::params![hash, media_id]).unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    connection
        .execute(
            queries::import::INSERT_CONTENT_HASH_CLAIM,
            rusqlite::params![hash, token, "local"],
        )
        .unwrap();
    assert_eq!(
        connection
            .execute(queries::import::REQUEUE_INTERRUPTED_MEDIA, [media_id])
            .unwrap(),
        0
    );
    connection
        .execute(queries::import::RECOVER_CONTENT_HASH_CLAIMS, [])
        .unwrap();
    assert_eq!(
        connection
            .execute(queries::import::REQUEUE_INTERRUPTED_MEDIA, [media_id])
            .unwrap(),
        1
    );
    let state: String = connection
        .query_row(queries::import::SELECT_RECOVERY_STATE, [media_id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(state, "recovering");
    assert_eq!(
        connection
            .execute(
                queries::import::MARK_FAILED,
                rusqlite::params!["unsupported source", media_id]
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .execute(queries::import::REQUEUE_INTERRUPTED_MEDIA, [media_id])
            .unwrap(),
        0
    );
    connection
        .execute(
            "UPDATE media SET import_state='imported' WHERE id=?",
            [media_id],
        )
        .unwrap();
    assert_eq!(
        connection
            .execute(queries::import::REQUEUE_INTERRUPTED_MEDIA, [media_id])
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .execute(
                queries::import::MARK_FAILED,
                rusqlite::params!["late failure", media_id]
            )
            .unwrap(),
        0
    );
}

#[test]
fn thumbnail_query_distinguishes_missing_media_from_missing_metadata() {
    use rusqlite::OptionalExtension;

    let pool = create_test_db();
    let media_id = create_test_media(&pool, "thumbnail-query.jpg");
    let connection = pool.get().expect("connection");
    let read_thumbnail = |id| {
        connection
            .query_row(queries::public::SELECT_MEDIA_THUMBNAIL, [id], |row| {
                row.get::<_, Option<String>>(0)
            })
            .optional()
            .expect("thumbnail query")
    };
    assert_eq!(read_thumbnail(-1), None);
    assert_eq!(read_thumbnail(media_id), Some(None));
    connection
        .execute("DELETE FROM media_metadata WHERE media_id = ?", [media_id])
        .expect("remove metadata");
    assert_eq!(read_thumbnail(media_id), Some(None));
    connection
        .execute(
            "INSERT INTO media_metadata (media_id, thumbnail_path) VALUES (?, 'ready.jpg')",
            [media_id],
        )
        .expect("publish thumbnail");
    assert_eq!(
        read_thumbnail(media_id),
        Some(Some("ready.jpg".to_string()))
    );
}

#[test]
fn visible_cluster_page_canonicalizes_user_specific_media_sets() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "query-viewer", "query-viewer@example.com");
    let first = create_test_media(&pool, "visible-first.jpg");
    let second = create_test_media(&pool, "visible-second.jpg");
    let hidden_first = create_test_media(&pool, "hidden-first.jpg");
    let hidden_second = create_test_media(&pool, "hidden-second.jpg");
    grant_media_access(&pool, first, user_id);
    grant_media_access(&pool, second, user_id);
    let connection = pool.get().expect("Failed to get connection");
    let mut cluster_ids = Vec::new();
    for members in [
        [first, second, hidden_first],
        [first, second, hidden_second],
    ] {
        connection
            .execute(
                queries::deduplicate::INSERT_CLUSTER,
                rusqlite::params!["near_duplicate", first],
            )
            .expect("Failed to create cluster");
        let cluster_id = connection.last_insert_rowid();
        cluster_ids.push(cluster_id);
        for media_id in members {
            connection
                .execute(
                    queries::deduplicate::INSERT_CLUSTER_MEMBER,
                    rusqlite::params![cluster_id, media_id, 1.0_f32, 0_u32],
                )
                .expect("Failed to create cluster member");
        }
    }

    let rows = connection
        .prepare(queries::deduplicate::SELECT_VISIBLE_CLUSTER_PAGE)
        .expect("Failed to prepare visible cluster page query")
        .query_map(rusqlite::params![user_id, 0_i64, 11_i64], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .expect("Failed to query visible cluster page")
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to collect visible cluster page");

    assert_eq!(rows, vec![(Some(cluster_ids[0]), 1, 2)]);
}
