use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use base64::Engine;
use momento_api::auth::create_access_token;
use momento_api::config::Config;
use momento_api::processor::face_detection;
use serde_json::json;

use crate::test_utils::{create_test_app, create_test_media, create_test_user, grant_media_access};

fn token(user_id: i64, role: &str) -> String {
    create_access_token(user_id, "faces", role, &Config::default(), None).expect("token")
}

#[test]
fn inaccessible_representative_uses_weighted_visible_face_score() {
    let (_, pool) = create_test_app();
    let viewer_id = create_test_user(&pool, "thumbnail-viewer", "thumbnail@example.com");
    let hidden_media_id = create_test_media(&pool, "hidden-representative.jpg");
    let center_media_id = create_test_media(&pool, "center-low-frontality.jpg");
    let frontal_media_id = create_test_media(&pool, "near-center-frontal.jpg");
    grant_media_access(&pool, center_media_id, viewer_id);
    grant_media_access(&pool, frontal_media_id, viewer_id);
    let connection = pool.get().expect("connection");
    let mut face_ids = Vec::new();
    for (media_id, face_x, frontality, crop_path) in [
        (hidden_media_id, 0.4, 1.0, "faces/hidden.jpg"),
        (center_media_id, 0.4, 0.1, "faces/center.jpg"),
        (frontal_media_id, 0.41, 1.0, "faces/frontal.jpg"),
    ] {
        connection.execute("INSERT INTO media_faces (media_id, input_sequence, face_index, x, y, width, height, confidence, face_size_score, frontality_score, visibility_score, feature_clarity_score, embedding, crop_path) VALUES (?, 0, 0, ?, 0.4, 0.2, 0.2, 1, 1, ?, 1, 1, X'00000000', ?)", rusqlite::params![media_id, face_x, frontality, crop_path]).expect("face");
        face_ids.push(connection.last_insert_rowid());
    }
    connection
        .execute(
            "INSERT INTO face_groups (representative_face_id) VALUES (?)",
            [face_ids[0]],
        )
        .expect("group");
    let group_id = connection.last_insert_rowid();
    for face_id in face_ids {
        connection
            .execute(
                "INSERT INTO face_group_members (face_group_id, face_id, manual_anchor) VALUES (?, ?, 0)",
                [group_id, face_id],
            )
            .expect("group member");
    }

    let crop_path = face_detection::visible_representative_crop(
        &connection,
        group_id,
        viewer_id,
        &Config::default().face_group,
    )
    .expect("visible representative query")
    .expect("visible representative crop");

    assert_eq!(crop_path, "faces/frontal.jpg");
}

#[tokio::test]
async fn face_groups_are_paginated_by_descending_media_count() {
    let (app, pool) = create_test_app();
    let viewer_id = create_test_user(&pool, "sorted-faces", "sorted-faces@example.com");
    let connection = pool.get().expect("connection");
    for (group_index, media_count) in [1, 3, 2].into_iter().enumerate() {
        let mut representative_face_id = None;
        let mut face_ids = Vec::new();
        for media_index in 0..media_count {
            let media_id = create_test_media(
                &pool,
                &format!("group-{group_index}-media-{media_index}.jpg"),
            );
            grant_media_access(&pool, media_id, viewer_id);
            connection.execute("INSERT INTO media_faces (media_id, input_sequence, face_index, x, y, width, height, confidence, face_size_score, frontality_score, visibility_score, feature_clarity_score, embedding, crop_path) VALUES (?, 0, 0, 0.4, 0.4, 0.2, 0.2, 1, 1, 1, 1, 1, X'00000000', 'faces/missing.jpg')", [media_id]).expect("face");
            let face_id = connection.last_insert_rowid();
            representative_face_id.get_or_insert(face_id);
            face_ids.push(face_id);
        }
        connection
            .execute(
                "INSERT INTO face_groups (representative_face_id) VALUES (?)",
                [representative_face_id.expect("representative face")],
            )
            .expect("group");
        let group_id = connection.last_insert_rowid();
        for face_id in face_ids {
            connection
                .execute(
                    "INSERT INTO face_group_members (face_group_id, face_id, manual_anchor) VALUES (?, ?, 0)",
                    [group_id, face_id],
                )
                .expect("group member");
        }
    }
    drop(connection);
    let server = TestServer::new(app).expect("server");

    let first_page = server
        .post("/api/v1/faces/groups/list")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(viewer_id, "user")),
        )
        .json(&json!({"limit": 2}))
        .await;
    first_page.assert_status_ok();
    let first_page = first_page.json::<serde_json::Value>();
    assert_eq!(first_page["groups"][0]["faceGroupId"], 2);
    assert_eq!(first_page["groups"][0]["mediaCount"], 3);
    assert_eq!(first_page["groups"][1]["faceGroupId"], 3);
    assert_eq!(first_page["groups"][1]["mediaCount"], 2);
    assert_eq!(first_page["nextCursor"], "2");

    let second_page = server
        .post("/api/v1/faces/groups/list")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(viewer_id, "user")),
        )
        .json(&json!({"limit": 2, "cursor": "2"}))
        .await;
    second_page.assert_status_ok();
    let second_page = second_page.json::<serde_json::Value>();
    assert_eq!(second_page["groups"][0]["faceGroupId"], 1);
    assert_eq!(second_page["groups"][0]["mediaCount"], 1);
    assert!(!second_page["hasMore"].as_bool().expect("hasMore"));
}

#[tokio::test]
async fn face_groups_are_filtered_to_media_access_and_admin_can_merge() {
    let (app, pool) = create_test_app();
    let viewer_id = create_test_user(&pool, "face-viewer", "face-viewer@example.com");
    let administrator_id = create_test_user(&pool, "face-admin", "face-admin@example.com");
    let visible_media_id = create_test_media(&pool, "visible.jpg");
    let hidden_media_id = create_test_media(&pool, "hidden.jpg");
    grant_media_access(&pool, visible_media_id, viewer_id);
    grant_media_access(&pool, visible_media_id, administrator_id);
    grant_media_access(&pool, hidden_media_id, administrator_id);
    let connection = pool.get().expect("connection");
    connection
        .execute(
            "UPDATE users SET role = 'admin' WHERE id = ?",
            [administrator_id],
        )
        .expect("admin");
    let embedding = base64::engine::general_purpose::STANDARD.encode(vec![0_u8; 512 * 4]);
    let mut face_ids = Vec::new();
    for (index, media_id) in [visible_media_id, hidden_media_id].into_iter().enumerate() {
        let face_x = if index == 0 { 0.0 } else { 0.4 };
        let crop_path = format!("faces/route-{index}.jpg");
        connection.execute("INSERT INTO media_faces (media_id, input_sequence, face_index, x, y, width, height, confidence, face_size_score, frontality_score, visibility_score, feature_clarity_score, embedding, crop_path) VALUES (?, 0, 0, ?, 0.4, 0.2, 0.2, 1, 1, 1, 1, 1, ?, ?)", rusqlite::params![media_id, face_x, base64::engine::general_purpose::STANDARD.decode(&embedding).expect("embedding"), crop_path]).expect("face");
        let face_id = connection.last_insert_rowid();
        face_ids.push(face_id);
        connection
            .execute(
                "INSERT INTO face_groups (representative_face_id) VALUES (?)",
                [face_id],
            )
            .expect("group");
        connection
            .execute(
                "INSERT INTO face_group_members (face_group_id, face_id, manual_anchor) VALUES (?, ?, 0)",
                [connection.last_insert_rowid(), face_id],
            )
            .expect("member");
    }
    drop(connection);
    let server = TestServer::new(app).expect("server");
    let response = server
        .post("/api/v1/faces/groups/list")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(viewer_id, "user")),
        )
        .json(&json!({"limit": 10}))
        .await;
    response.assert_status_ok();
    let list_body = response.json::<serde_json::Value>();
    assert_eq!(list_body["groups"].as_array().expect("groups").len(), 1);
    assert_eq!(list_body["groups"][0]["mediaCount"], 1);
    server
        .post("/api/v1/faces/groups/merge")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(viewer_id, "user")),
        )
        .json(&json!({"faceGroupIds": [1, 2]}))
        .await
        .assert_status_forbidden();
    let merge = server
        .post("/api/v1/faces/groups/merge")
        .add_header(
            AUTHORIZATION,
            format!("Bearer {}", token(administrator_id, "admin")),
        )
        .json(&json!({"faceGroupIds": [1, 2]}))
        .await;
    merge.assert_status_ok();
    assert_eq!(merge.json::<serde_json::Value>()["group"]["mediaCount"], 2);
    let connection = pool.get().expect("connection");
    let (manual_curated, manual_anchor_count, member_count, representative_face_id):
        (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT face_groups.manual_curated, SUM(face_group_members.manual_anchor), COUNT(face_group_members.face_id), face_groups.representative_face_id FROM face_groups JOIN face_group_members ON face_group_members.face_group_id = face_groups.id WHERE face_groups.id = 1 GROUP BY face_groups.id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("merged group");
    assert_eq!(manual_curated, 1);
    assert_eq!(manual_anchor_count, 2);
    assert_eq!(member_count, 2);
    assert_eq!(representative_face_id, face_ids[1]);
    let viewer_crop = face_detection::visible_representative_crop(
        &connection,
        1,
        viewer_id,
        &Config::default().face_group,
    )
    .expect("viewer representative query")
    .expect("viewer crop");
    let administrator_crop = face_detection::visible_representative_crop(
        &connection,
        1,
        administrator_id,
        &Config::default().face_group,
    )
    .expect("administrator representative query")
    .expect("administrator crop");
    assert_eq!(viewer_crop, "faces/route-0.jpg");
    assert_eq!(administrator_crop, "faces/route-1.jpg");
    let source_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM face_groups WHERE id = 2", [], |row| {
            row.get(0)
        })
        .expect("source group count");
    assert_eq!(source_count, 0);
}

#[tokio::test]
async fn rejecting_a_detection_is_admin_only_global_precise_and_survives_redetection() {
    let (app, pool) = create_test_app();
    let admin = create_test_user(&pool, "reject-admin", "reject-admin@example.com");
    let viewer = create_test_user(&pool, "reject-viewer", "reject-viewer@example.com");
    let media = create_test_media(&pool, "mixed-faces.jpg");
    grant_media_access(&pool, media, admin);
    grant_media_access(&pool, media, viewer);
    let c = pool.get().unwrap();
    c.execute("UPDATE users SET role='admin' WHERE id=?", [admin])
        .unwrap();
    let insert = "INSERT INTO media_faces (media_id,input_sequence,face_index,x,y,width,height,confidence,face_size_score,frontality_score,visibility_score,feature_clarity_score,embedding,crop_path) VALUES (?,0,?, ?,0.1,0.1,0.1,1,1,1,1,1,?, 'faces/test.jpg')";
    let embedding = vec![0u8; 512 * 4];
    c.execute(insert, rusqlite::params![media, 0, 0.1, embedding])
        .unwrap();
    let false_face = c.last_insert_rowid();
    c.execute(
        "UPDATE media_faces SET crop_path='faces/rejected-only.jpg' WHERE id=?",
        [false_face],
    )
    .unwrap();
    c.execute(insert, rusqlite::params![media, 1, 0.7, embedding])
        .unwrap();
    let true_face = c.last_insert_rowid();
    c.execute(
        "INSERT INTO face_groups (representative_face_id) VALUES (?)",
        [false_face],
    )
    .unwrap();
    let group = c.last_insert_rowid();
    for face in [false_face, true_face] {
        c.execute(
            "INSERT INTO face_group_members(face_group_id,face_id,manual_anchor) VALUES (?,?,0)",
            [group, face],
        )
        .unwrap();
    }
    drop(c);
    let server = TestServer::new(app).unwrap();
    let body = json!({"requestId":uuid::Uuid::new_v4().to_string(),"groupIds":[],"faceGroupId":group,"faceIds":[false_face]});
    server
        .post("/api/v1/faces/reject")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(viewer, "user")))
        .json(&body)
        .await
        .assert_status_forbidden();
    for _ in 0..2 {
        let response = server
            .post("/api/v1/faces/reject")
            .add_header(AUTHORIZATION, format!("Bearer {}", token(admin, "admin")))
            .json(&body)
            .await;
        response.assert_status_ok();
        assert_eq!(response.json::<serde_json::Value>()["rejectedCount"], 1);
    }
    let response = server
        .post("/api/v1/faces/groups/get")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(viewer, "user")))
        .json(&json!({"faceGroupId":group}))
        .await;
    response.assert_status_ok();
    let data = response.json::<serde_json::Value>();
    assert_eq!(data["faces"].as_array().unwrap().len(), 1);
    assert_eq!(data["faces"][0]["faceId"], true_face);
    let c = pool.get().unwrap();
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM file_operation_groups WHERE owner_kind='face_rejection'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1,
        "unreferenced rejected crop has durable cleanup"
    );
    assert_eq!(
        c.execute(insert, rusqlite::params![media, 2, 0.105, embedding])
            .unwrap(),
        0,
        "slightly shifted false detection stays suppressed"
    );
    assert_eq!(
        c.execute(insert, rusqlite::params![media, 3, 0.7, embedding])
            .unwrap(),
        1,
        "another face in the same image remains valid"
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM media WHERE id=?", [media], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let other_frame = insert
        .replace("crop_path)", "crop_path,frame_timestamp_ms)")
        .replace("'faces/test.jpg')", "'faces/test.jpg',1000)");
    assert_eq!(
        c.execute(&other_frame, rusqlite::params![media, 4, 0.105, embedding])
            .unwrap(),
        1,
        "same box in a different frame is not excluded"
    );
    drop(c);
    let response = server.post("/api/v1/faces/reject")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(admin, "admin")))
        .json(&json!({"requestId":uuid::Uuid::new_v4().to_string(),"groupIds":[group],"faceGroupId":null,"faceIds":[]})).await;
    response.assert_status_ok();
    assert_eq!(response.json::<serde_json::Value>()["rejectedCount"], 1);
    // The fixture has no background worker; drain the committed crop cleanup before
    // requesting the exclusive face-directory cleanup.
    let (executors, _) = crate::test_utils::test_executor_handles_with_data_directory(pool.clone());
    momento_api::io::recovery::recover_generic_file_operations(&executors)
        .await
        .unwrap();
    server
        .post("/api/v1/ai/face_detection/clean")
        .add_header(AUTHORIZATION, format!("Bearer {}", token(admin, "admin")))
        .json(&json!({}))
        .await
        .assert_status_ok();
    let c = pool.get().unwrap();
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM face_rejections", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.execute(insert, rusqlite::params![media, 2, 0.105, embedding])
            .unwrap(),
        1,
        "admin clean resets exclusion"
    );
}

#[tokio::test]
async fn rejection_does_not_accept_hidden_faces_or_widen_a_changed_selection() {
    let (app, pool) = create_test_app();
    let admin = create_test_user(&pool, "reject-hidden", "reject-hidden@example.com");
    let visible = create_test_media(&pool, "visible-rejection.jpg");
    let hidden = create_test_media(&pool, "hidden-rejection.jpg");
    grant_media_access(&pool, visible, admin);
    let c = pool.get().unwrap();
    c.execute("UPDATE users SET role='admin' WHERE id=?", [admin])
        .unwrap();
    let mut ids = Vec::new();
    for media in [visible, hidden] {
        c.execute("INSERT INTO media_faces (media_id,input_sequence,face_index,x,y,width,height,confidence,face_size_score,frontality_score,visibility_score,feature_clarity_score,embedding,crop_path) VALUES (?,0,0,0.1,0.1,0.1,0.1,1,1,1,1,1,?,'faces/shared-hidden.jpg')", rusqlite::params![media, vec![0u8;2048]]).unwrap();
        ids.push(c.last_insert_rowid());
    }
    c.execute(
        "INSERT INTO face_groups (representative_face_id) VALUES (?)",
        [ids[0]],
    )
    .unwrap();
    let group = c.last_insert_rowid();
    for face in &ids {
        c.execute(
            "INSERT INTO face_group_members (face_group_id,face_id,manual_anchor) VALUES (?,?,0)",
            [group, *face],
        )
        .unwrap();
    }
    drop(c);
    let server = TestServer::new(app).unwrap();
    server.post("/api/v1/faces/reject")
        .add_header(AUTHORIZATION,format!("Bearer {}",token(admin,"admin")))
        .json(&json!({"requestId":uuid::Uuid::new_v4().to_string(),"groupIds":[],"faceGroupId":group,"faceIds":ids})).await.assert_status(axum::http::StatusCode::CONFLICT);
    assert_eq!(
        pool.get()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM media_faces WHERE media_id IN (?,?)",
                [visible, hidden],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2,
        "invalid mixed selection rolls back entirely"
    );
    let response = server.post("/api/v1/faces/reject")
        .add_header(AUTHORIZATION,format!("Bearer {}",token(admin,"admin")))
        .json(&json!({"requestId":uuid::Uuid::new_v4().to_string(),"groupIds":[group],"faceGroupId":null,"faceIds":[]})).await;
    response.assert_status_ok();
    assert_eq!(response.json::<serde_json::Value>()["rejectedCount"], 1);
    assert_eq!(
        pool.get()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM media_faces WHERE id=?",
                [ids[1]],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1,
        "group operation preserves inaccessible media detections"
    );
}
