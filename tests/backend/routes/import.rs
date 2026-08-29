use axum::http::header::AUTHORIZATION;
use axum_test::TestServer;
use momento_api::{auth::create_access_token, config::Config};

use crate::test_utils::{create_test_app, create_test_media, create_test_user};

#[tokio::test]
async fn local_import_status_reports_distinct_imported_media_separately_from_source_files() {
    let (application, pool) = create_test_app();
    let administrator_id = create_test_user(
        &pool,
        "import-status-admin",
        "import-status-admin@example.com",
    );
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "UPDATE users SET role = 'admin' WHERE id = ?",
            [administrator_id],
        )
        .expect("administrator role");
    drop(connection);

    create_test_media(&pool, "first-imported.jpg");
    create_test_media(&pool, "second-imported.jpg");
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT INTO media (user_id, filename, original_filename, file_path, media_type, import_state, import_source) VALUES (?, 'pending.jpg', 'pending.jpg', 'pending.jpg', 'image', 'importing', 'local')",
            [administrator_id],
        )
        .expect("importing media");
    connection
        .execute(
            "INSERT INTO import_jobs (source, status, total_files, processed_files, successful_imports, failed_imports) VALUES ('local', 'failed', 4, 4, 3, 1)",
            [],
        )
        .expect("local import job");
    let import_job_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO import_job_errors (import_job_id, error) VALUES (?, 'failed to calculate SHA-256 for /data/imports/broken.jpg: permission denied'), (?, 'failed to move /data/imports/full.mov to /data/originals/full.mov: no space left')",
            rusqlite::params![import_job_id, import_job_id],
        )
        .expect("import errors");
    drop(connection);

    let access_token = create_access_token(
        administrator_id,
        "import-status-admin",
        "admin",
        &Config::default(),
        None,
    )
    .expect("access token");
    let server = TestServer::new(application).expect("server");
    let response = server
        .post("/api/v1/import/status")
        .add_header(AUTHORIZATION, format!("Bearer {access_token}"))
        .json(&serde_json::json!({}))
        .await;

    response.assert_status_ok();
    let status = response.json::<serde_json::Value>();
    assert_eq!(status["totalMedia"], 2);
    assert_eq!(status["successfulImports"], 3);
    assert_eq!(status["failedImports"], 1);
    assert_eq!(
        status["errors"],
        serde_json::json!([
            "failed to move /data/imports/full.mov to /data/originals/full.mov: no space left",
            "failed to calculate SHA-256 for /data/imports/broken.jpg: permission denied"
        ])
    );
}
