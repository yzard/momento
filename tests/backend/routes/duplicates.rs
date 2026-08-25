use crate::test_utils::{create_test_app, create_test_media, create_test_user, grant_media_access};
use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use serde_json::{json, Value};

fn token(user_id: i64) -> String {
    create_access_token(user_id, "testuser", "user", &Config::default(), None)
        .expect("access token")
}

#[tokio::test]
async fn list_only_returns_groups_with_two_visible_items() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "viewer", "viewer@example.com");
    let visible_a = create_test_media(&pool, "a.jpg");
    let visible_b = create_test_media(&pool, "b.jpg");
    let hidden = create_test_media(&pool, "hidden.jpg");
    grant_media_access(&pool, visible_a, user_id);
    grant_media_access(&pool, visible_b, user_id);
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO media_similarity_clusters (kind, representative_media_id) VALUES ('near_duplicate', ?)",
            [visible_a],
        )
        .expect("cluster");
    let cluster_id = connection.last_insert_rowid();
    for media_id in [visible_a, visible_b, hidden] {
        connection
            .execute(
                "INSERT INTO media_similarity_cluster_members (cluster_id, media_id, cosine_similarity, perceptual_hash_distance) VALUES (?, ?, 1.0, 0)",
                rusqlite::params![cluster_id, media_id],
            )
            .expect("cluster member");
    }
    drop(connection);
    let server = TestServer::new(app).expect("server");

    let response = server
        .post("/api/v1/duplicates/list")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(user_id)))
        .json(&json!({"cursor": null, "limit": 10}))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["groups"].as_array().expect("groups").len(), 1);
    assert_eq!(body["groups"][0]["clusterId"], cluster_id);
    assert_eq!(
        body["groups"][0]["items"].as_array().expect("items").len(),
        2
    );
    assert_eq!(body["nextCursor"], Value::Null);
    assert_eq!(body["hasMore"], false);
    assert_eq!(body["totalGroups"], 1);
    assert_eq!(body["totalMedia"], 2);
}

#[tokio::test]
async fn list_canonicalizes_identical_sets_before_cursor_pagination() {
    let (app, pool) = create_test_app();
    let user_id = create_test_user(&pool, "paged-viewer", "paged-viewer@example.com");
    let first = create_test_media(&pool, "first.jpg");
    let second = create_test_media(&pool, "second.jpg");
    let third = create_test_media(&pool, "third.jpg");
    let fourth = create_test_media(&pool, "fourth.jpg");
    for media_id in [first, second, third, fourth] {
        grant_media_access(&pool, media_id, user_id);
    }
    let connection = pool.get().expect("database connection");
    let mut cluster_ids = Vec::new();
    for (kind, members) in [
        ("near_duplicate", vec![first, second]),
        ("burst", vec![first, second]),
        ("near_duplicate", vec![second, third]),
        ("near_duplicate", vec![third, fourth]),
    ] {
        connection
            .execute(
                "INSERT INTO media_similarity_clusters (kind, representative_media_id) VALUES (?, ?)",
                rusqlite::params![kind, members[0]],
            )
            .expect("cluster");
        let cluster_id = connection.last_insert_rowid();
        cluster_ids.push(cluster_id);
        for media_id in members {
            connection
                .execute(
                    "INSERT INTO media_similarity_cluster_members (cluster_id, media_id, cosine_similarity, perceptual_hash_distance) VALUES (?, ?, 1.0, 0)",
                    rusqlite::params![cluster_id, media_id],
                )
                .expect("cluster member");
        }
    }
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let authorization = format!("Bearer {}", token(user_id));

    let first_page = server
        .post("/api/v1/duplicates/list")
        .add_header(AUTHORIZATION, authorization.clone())
        .json(&json!({"cursor": null, "limit": 1}))
        .await;
    first_page.assert_status_ok();
    let first_body: Value = first_page.json();
    assert_eq!(first_body["groups"][0]["clusterId"], cluster_ids[0]);
    assert_eq!(first_body["nextCursor"], cluster_ids[0].to_string());
    assert_eq!(first_body["totalGroups"], 3);
    assert_eq!(first_body["totalMedia"], 4);

    let second_page = server
        .post("/api/v1/duplicates/list")
        .add_header(AUTHORIZATION, authorization)
        .json(&json!({"cursor": first_body["nextCursor"], "limit": 1}))
        .await;
    second_page.assert_status_ok();
    let second_body: Value = second_page.json();
    assert_eq!(second_body["groups"][0]["clusterId"], cluster_ids[2]);
    assert_ne!(second_body["groups"][0]["clusterId"], cluster_ids[1]);
}
