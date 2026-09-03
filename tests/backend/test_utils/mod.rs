use axum::Router;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use momento_api::app::{create_app, AppDependencies};
use momento_api::config::{Config, ConfigManager};
use momento_api::database::{create_pool_at, DbPool};

static MEDIA_ID_COUNTER: AtomicI64 = AtomicI64::new(1);
static USER_ID_COUNTER: AtomicI64 = AtomicI64::new(1);
static WEBDAV_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
static TEST_EXECUTOR_RUNTIMES: OnceLock<
    std::sync::Mutex<Vec<momento_api::runtime::ExecutorRuntime>>,
> = OnceLock::new();
static TEST_DATABASE_DIRECTORIES: OnceLock<std::sync::Mutex<Vec<tempfile::TempDir>>> =
    OnceLock::new();
static TEST_EXECUTOR_HANDLES: OnceLock<
    std::sync::Mutex<HashMap<std::path::PathBuf, momento_api::runtime::ExecutorHandles>>,
> = OnceLock::new();
static TEST_AUTHENTICATION_DUMMY_HASH: OnceLock<String> = OnceLock::new();

pub const QOI_FIXTURE: &[u8] = &[
    0x71, 0x6f, 0x69, 0x66, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x03, 0x01, 0xfe, 0x0a,
    0x14, 0x1e, 0xc4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
];

pub async fn lock_webdav_test() -> tokio::sync::MutexGuard<'static, ()> {
    WEBDAV_TEST_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

pub fn create_test_db() -> DbPool {
    let directory = tempfile::tempdir().expect("test database directory");
    let pool = create_pool_at(&directory.path().join("database.sqlite"), 5)
        .expect("Failed to create test database pool");
    TEST_DATABASE_DIRECTORIES
        .get_or_init(|| std::sync::Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(directory);
    pool
}

pub fn test_executor_handles(pool: DbPool) -> momento_api::runtime::ExecutorHandles {
    test_executor_handles_with_data_directory(pool).0
}

pub fn test_executor_handles_with_data_directory(
    pool: DbPool,
) -> (momento_api::runtime::ExecutorHandles, std::path::PathBuf) {
    let data_directory_path = test_data_directory(&pool);
    let database_path = data_directory_path.join("database.sqlite");
    let mut registered_handles = TEST_EXECUTOR_HANDLES
        .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(handles) = registered_handles.get(&database_path) {
        return (handles.clone(), data_directory_path);
    }
    let sizing = momento_api::runtime::RuntimeSizing::validate_worker_counts(
        &momento_api::config::ThreadPoolConfig {
            cpu_workers: 1,
            io_workers: 4,
            sqlite_workers: 2,
        },
    )
    .expect("test runtime sizing");
    let config_path = data_directory_path.join("config.toml");
    std::fs::write(&config_path, "# executor config\n").expect("write executor config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load executor config")
        .identity;
    let (runtime, handles) = momento_api::runtime::ExecutorRuntime::start(
        &sizing,
        pool,
        identity,
        data_directory_path.clone(),
        None,
    )
    .expect("test executors");
    futures::executor::block_on(handles.cpu.initialize_reverse_geocoder_durable())
        .expect("initialize test reverse geocoder before publishing executor handles");
    TEST_EXECUTOR_RUNTIMES
        .get_or_init(|| std::sync::Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(runtime);
    registered_handles.insert(database_path, handles.clone());
    (handles, data_directory_path)
}

pub fn test_data_directory(pool: &DbPool) -> std::path::PathBuf {
    pool.get()
        .expect("test database connection")
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name = 'main'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(std::path::PathBuf::from)
        .expect("test database path")
        .parent()
        .expect("test database parent")
        .to_path_buf()
}

pub fn test_authentication_dummy_hash() -> String {
    TEST_AUTHENTICATION_DUMMY_HASH
        .get_or_init(|| {
            momento_api::auth::hash_password("momento-password-verification-placeholder")
                .expect("test authentication dummy hash")
        })
        .clone()
}

pub fn test_app_dependencies(
    pool: DbPool,
    admin_password_reset_user_id: Option<i64>,
) -> AppDependencies {
    let executors = test_executor_handles(pool.clone());
    AppDependencies {
        executors,
        authentication_dummy_hash: test_authentication_dummy_hash(),
        llm_transport: Default::default(),
        webdav_request_gate: Arc::new(tokio::sync::RwLock::new(())),
        admin_password_reset_user_id,
    }
}

pub fn test_scheduler(pool: DbPool) -> momento_api::runtime::SchedulerHandle {
    test_executor_handles(pool).scheduler
}

pub fn create_test_app() -> (Router, DbPool) {
    let pool = create_test_db();
    let config_manager = create_test_config_manager(Config::default());
    let app = create_app(config_manager, test_app_dependencies(pool.clone(), None));
    (app, pool)
}

pub fn create_test_config_manager(mut config: Config) -> ConfigManager {
    if config.llm.enabled {
        if config.llm.client_id.is_empty() {
            config.llm.client_id = "test-client".to_string();
        }
        if config.llm.api_key.is_empty() {
            config.llm.api_key = "test-api-key".to_string();
        }
    }
    let config_path =
        std::env::temp_dir().join(format!("momento-test-config-{}.toml", uuid::Uuid::new_v4()));
    let config_contents = toml::to_string(&config).expect("serialize test config");
    std::fs::write(&config_path, config_contents).expect("write test config");
    let loaded = momento_api::config::load_config_with_identity(&config_path)
        .expect("load test config identity");
    let executors = test_executor_handles(create_test_db());
    ConfigManager::new(loaded, &executors)
}

pub fn create_test_user(pool: &DbPool, username: &str, email: &str) -> i64 {
    let conn = pool.get().expect("Failed to get connection");
    let user_id = USER_ID_COUNTER.fetch_add(1, Ordering::SeqCst);

    conn.execute(
        "INSERT INTO users (id, username, email, hashed_password, role, must_change_password, is_active) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params![user_id, username, email, "hashed_password_placeholder", "user", 0, 1],
    )
    .expect("Failed to insert test user");

    user_id
}

pub fn create_test_media_with_gps(
    pool: &DbPool,
    filename: &str,
    latitude: f64,
    longitude: f64,
) -> i64 {
    create_test_media_with_gps_and_date(pool, filename, latitude, longitude, "2024-01-15T10:30:00")
}

pub fn create_test_media_with_gps_and_date(
    pool: &DbPool,
    filename: &str,
    latitude: f64,
    longitude: f64,
    date_taken: &str,
) -> i64 {
    let conn = pool.get().expect("Failed to get connection");
    let media_id = MEDIA_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    let file_path = format!("test/media/{}", filename);
    let content_hash = format!("hash_{}", media_id);

    let geohash = geohash::encode(
        geohash::Coord {
            x: longitude,
            y: latitude,
        },
        9,
    )
    .ok();

    conn.execute(
        "INSERT INTO media (
            id, filename, original_filename, file_path, media_type, mime_type,
            file_size, content_hash, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))",
        rusqlite::params![
            media_id,
            filename,
            filename,
            file_path,
            "image",
            "image/jpeg",
            1024000,
            content_hash,
        ],
    )
    .expect("Failed to insert test media");

    conn.execute(
        "INSERT INTO media_metadata (
            media_id, width, height, date_taken, gps_latitude, gps_longitude, geohash
        ) VALUES (?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params![media_id, 1920, 1080, date_taken, latitude, longitude, geohash,],
    )
    .expect("Failed to insert test media metadata");

    media_id
}

pub fn grant_media_access(pool: &DbPool, media_id: i64, user_id: i64) {
    let conn = pool.get().expect("Failed to get connection");
    conn.execute(
        "INSERT OR IGNORE INTO media_access (media_id, user_id, access_level) VALUES (?, ?, 1)",
        rusqlite::params![media_id, user_id],
    )
    .expect("Failed to grant media access");
}

pub fn create_test_media(pool: &DbPool, filename: &str) -> i64 {
    let conn = pool.get().expect("Failed to get connection");
    let media_id = MEDIA_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    let file_path = format!("test/media/{}", filename);
    let content_hash = format!("hash_{}", media_id);

    conn.execute(
        "INSERT INTO media (
            id, filename, original_filename, file_path, media_type, mime_type,
            file_size, content_hash, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))",
        rusqlite::params![
            media_id,
            filename,
            filename,
            file_path,
            "image",
            "image/jpeg",
            1024000,
            content_hash,
        ],
    )
    .expect("Failed to insert test media");

    conn.execute(
        "INSERT INTO media_metadata (
            media_id, width, height, date_taken
        ) VALUES (?, ?, ?, ?)",
        rusqlite::params![media_id, 1920, 1080, "2024-01-15T10:30:00",],
    )
    .expect("Failed to insert test media metadata");

    media_id
}

pub fn prepare_failed_ai_job(pool: &DbPool, media_id: i64, task: &str, job_id: &str) {
    let connection = pool.get().expect("database connection");
    connection
        .execute(
            "INSERT OR IGNORE INTO media_metadata_jobs (media_id, status) VALUES (?, 'completed')",
            [media_id],
        )
        .expect("metadata job");
    connection
        .execute(
            "INSERT INTO media_ai_inputs (media_id, task, sequence, input_kind, storage_root, file_path, filename, mime_type, byte_size, content_hash) VALUES (?, ?, 0, 'image', 'previews', ?, ?, 'image/jpeg', 4, 'hash')",
            rusqlite::params![
                media_id,
                task,
                format!("ai/{task}.jpg"),
                format!("{task}.jpg")
            ],
        )
        .expect("prepared AI input");
    match task {
        "face_detection" => {
            connection
                .execute(
                    "INSERT INTO face_grouping_runs (status, completed_at, error) VALUES ('failed', datetime('now'), 'inference failed')",
                    [],
                )
                .expect("failed face grouping run");
            let run_id = connection.last_insert_rowid();
            connection
                .execute(
                    "INSERT INTO llm_jobs (id, media_id, face_grouping_run_id, task, status, last_error) VALUES (?, ?, ?, 'face_detection', 'failed', 'inference failed')",
                    rusqlite::params![job_id, media_id, run_id],
                )
                .expect("failed face detection job");
        }
        "image_clustering" => {
            connection
                .execute(
                    "INSERT INTO media_similarity_runs (trigger, status, completed_at, error) VALUES ('manual', 'failed', datetime('now'), 'inference failed')",
                    [],
                )
                .expect("failed deduplicate run");
            let run_id = connection.last_insert_rowid();
            connection
                .execute(
                    "INSERT INTO llm_jobs (id, media_id, deduplicate_run_id, task, status, last_error) VALUES (?, ?, ?, 'image_clustering', 'failed', 'inference failed')",
                    rusqlite::params![job_id, media_id, run_id],
                )
                .expect("failed image clustering job");
        }
        "ocr"
        | "image_tagging"
        | "image_aesthetics"
        | "screenshot_detection"
        | "document_detection" => {
            connection
                .execute(
                    "INSERT INTO llm_jobs (id, media_id, task, status, last_error) VALUES (?, ?, ?, 'failed', 'inference failed')",
                    rusqlite::params![job_id, media_id, task],
                )
                .expect("failed AI job");
        }
        _ => panic!("unsupported AI task fixture: {task}"),
    }
}

pub fn assert_failed_ai_job_restarted(pool: &DbPool, task: &str) {
    let connection = pool.get().expect("database connection");
    let statuses = connection
        .prepare("SELECT status FROM llm_jobs WHERE task = ? ORDER BY rowid")
        .expect("job status query")
        .query_map([task], |row| row.get::<_, String>(0))
        .expect("job statuses")
        .collect::<Result<Vec<_>, _>>()
        .expect("job status rows");
    assert_eq!(statuses, ["failed", "queued"], "{task} retry history");

    let run_status = match task {
        "face_detection" => connection
            .query_row(
                "SELECT runs.status FROM llm_jobs AS jobs JOIN face_grouping_runs AS runs ON runs.id = jobs.face_grouping_run_id WHERE jobs.task = ? AND jobs.status = 'queued'",
                [task],
                |row| row.get::<_, String>(0),
            )
            .expect("replacement face run status"),
        "image_clustering" => connection
            .query_row(
                "SELECT runs.status FROM llm_jobs AS jobs JOIN media_similarity_runs AS runs ON runs.id = jobs.deduplicate_run_id WHERE jobs.task = ? AND jobs.status = 'queued'",
                [task],
                |row| row.get::<_, String>(0),
            )
            .expect("replacement deduplicate run status"),
        "ocr" | "image_tagging" | "image_aesthetics" | "screenshot_detection"
        | "document_detection" => return,
        _ => panic!("unsupported AI task fixture: {task}"),
    };
    assert_eq!(run_status, "running", "{task} replacement run");
}

#[test]
fn test_create_test_db() {
    let pool = create_test_db();
    let conn = pool.get().expect("Failed to get connection");

    let result: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'",
        [],
        |row| row.get(0),
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 1);
}

#[test]
fn test_create_test_db_on_a_mounted_filesystem_under_two_seconds() {
    let start = Instant::now();
    let _pool = create_test_db();
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(2));
}

#[test]
fn test_create_test_app() {
    let (_app, _pool) = create_test_app();
}

#[test]
fn test_create_test_user() {
    let pool = create_test_db();
    let user_id = create_test_user(&pool, "testuser", "test@example.com");

    let conn = pool.get().expect("Failed to get connection");
    let result: Result<String, _> = conn.query_row(
        "SELECT username FROM users WHERE id = ?",
        [user_id],
        |row| row.get(0),
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "testuser");
}

#[test]
fn test_create_test_media_with_gps() {
    let pool = create_test_db();
    let media_id = create_test_media_with_gps(&pool, "photo.jpg", 40.7128, -74.0060);

    let conn = pool.get().expect("Failed to get connection");
    let result: Result<(f64, f64), _> = conn.query_row(
        "SELECT gps_latitude, gps_longitude FROM media_metadata WHERE media_id = ?",
        [media_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );

    assert!(result.is_ok());
    let (lat, lon) = result.unwrap();
    assert_eq!(lat, 40.7128);
    assert_eq!(lon, -74.0060);
}

#[test]
fn test_create_test_media() {
    let pool = create_test_db();
    let media_id = create_test_media(&pool, "photo.jpg");

    let conn = pool.get().expect("Failed to get connection");
    let result: Result<String, _> = conn.query_row(
        "SELECT filename FROM media WHERE id = ?",
        [media_id],
        |row| row.get(0),
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "photo.jpg");
}

#[test]
fn test_sequential_id_generation() {
    let pool = create_test_db();

    let id1 = create_test_media(&pool, "photo1.jpg");
    let id2 = create_test_media(&pool, "photo2.jpg");
    let id3 = create_test_media(&pool, "photo3.jpg");

    assert!(id1 < id2);
    assert!(id2 < id3);
}

#[test]
fn test_multiple_media_with_gps() {
    let pool = create_test_db();

    let id1 = create_test_media_with_gps(&pool, "photo1.jpg", 40.7128, -74.0060);
    let id2 = create_test_media_with_gps(&pool, "photo2.jpg", 51.5074, -0.1278);

    let conn = pool.get().expect("Failed to get connection");

    let (lat1, lon1): (f64, f64) = conn
        .query_row(
            "SELECT gps_latitude, gps_longitude FROM media_metadata WHERE media_id = ?",
            [id1],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("Failed to query first media");

    assert_eq!(lat1, 40.7128);
    assert_eq!(lon1, -74.0060);

    let (lat2, lon2): (f64, f64) = conn
        .query_row(
            "SELECT gps_latitude, gps_longitude FROM media_metadata WHERE media_id = ?",
            [id2],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("Failed to query second media");

    assert_eq!(lat2, 51.5074);
    assert_eq!(lon2, -0.1278);
}
