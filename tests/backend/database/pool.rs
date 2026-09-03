use momento_api::database::{create_pool_at, init_database, SqliteBootstrapFootprintSpec};
use tempfile::TempDir;

#[test]
fn file_pool_enables_wal_and_busy_timeout() {
    let directory = TempDir::new().expect("Failed to create temporary directory");
    let database_path = directory.path().join("database.sqlite");
    let pool = create_pool_at(&database_path, 2).expect("Failed to create database pool");
    let connection = pool.get().expect("Failed to get database connection");
    init_database(&connection).expect("Failed to initialize database");

    let journal_mode: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .expect("Failed to query journal mode");
    let busy_timeout: i64 = connection
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .expect("Failed to query busy timeout");
    let synchronous: i64 = connection
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .expect("Failed to query synchronous mode");
    let temp_store: i64 = connection
        .pragma_query_value(None, "temp_store", |row| row.get(0))
        .expect("Failed to query temp store");
    let mmap_size: i64 = connection
        .pragma_query_value(None, "mmap_size", |row| row.get(0))
        .expect("Failed to query mmap size");
    let sqlite_threads: i64 = connection
        .pragma_query_value(None, "threads", |row| row.get(0))
        .expect("Failed to query SQLite threads");

    assert_eq!(journal_mode.to_lowercase(), "wal");
    assert_eq!(busy_timeout, 5_000);
    assert_eq!(synchronous, 1);
    assert_eq!(temp_store, 2);
    assert_eq!(mmap_size, 0);
    assert_eq!(sqlite_threads, 0);
}

#[test]
fn wal_reader_is_not_blocked_by_uncommitted_writer() {
    let directory = TempDir::new().expect("Failed to create temporary directory");
    let database_path = directory.path().join("database.sqlite");
    let pool = create_pool_at(&database_path, 2).expect("Failed to create database pool");
    let writer = pool.get().expect("Failed to get writer connection");
    init_database(&writer).expect("Failed to initialize database");
    writer
        .execute(
            "INSERT INTO users (username, email, hashed_password) VALUES ('first', 'first@example.com', 'hash')",
            [],
        )
        .expect("Failed to insert baseline user");
    let transaction = writer
        .unchecked_transaction()
        .expect("Failed to begin write transaction");
    transaction
        .execute(
            "INSERT INTO users (username, email, hashed_password) VALUES ('second', 'second@example.com', 'hash')",
            [],
        )
        .expect("Failed to insert uncommitted user");
    let reader = pool.get().expect("Failed to get reader connection");

    let visible_users: i64 = reader
        .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
        .expect("WAL reader should not be blocked by writer");

    assert_eq!(visible_users, 1);
    transaction.rollback().expect("Failed to roll back writer");
}

#[test]
fn fresh_schema_bootstrap_footprint_bounds_the_published_database() {
    use std::os::unix::fs::MetadataExt;

    let directory = TempDir::new().expect("bootstrap directory");
    let database_path = directory.path().join("database.sqlite");
    let spec = SqliteBootstrapFootprintSpec::derive(4096).expect("bootstrap footprint");
    let pool = create_pool_at(&database_path, 2).expect("fresh database pool");
    let _connection = pool.get().expect("keep WAL connection open");
    let allocated = [
        database_path.clone(),
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ]
    .into_iter()
    .try_fold(0_u64, |total, path| match std::fs::metadata(path) {
        Ok(metadata) => total.checked_add(metadata.blocks() * 512),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(total),
        Err(error) => panic!("SQLite allocation metadata: {error}"),
    })
    .expect("SQLite allocation sum");

    assert!(spec.schema_page_count > 0);
    assert!(spec.private_database_bytes > 0);
    assert!(allocated <= spec.peak_additional_bytes);
    assert!(spec.peak_additional_bytes < 64 * 1024 * 1024);
}

#[test]
fn fresh_bootstrap_recovers_only_a_valid_empty_source_owned_temporary() {
    let directory = TempDir::new().expect("bootstrap directory");
    let database_path = directory.path().join("database.sqlite");
    let temporary_path = directory
        .path()
        .join(".database.sqlite.momento-bootstrap.tmp");
    let pool = create_pool_at(&database_path, 1).expect("initial fresh database");
    drop(pool);
    std::fs::rename(&database_path, &temporary_path).expect("simulate pre-publish crash");

    let recovered = create_pool_at(&database_path, 1).expect("recover fresh bootstrap");
    assert!(database_path.exists());
    assert!(!temporary_path.exists());
    drop(recovered);
}

#[test]
fn fresh_bootstrap_preserves_untrusted_or_nonempty_temporaries_for_inspection() {
    for fixture in ["invalid", "nonempty"] {
        let directory = TempDir::new().expect("bootstrap directory");
        let database_path = directory.path().join("database.sqlite");
        let temporary_path = directory
            .path()
            .join(".database.sqlite.momento-bootstrap.tmp");
        if fixture == "invalid" {
            std::fs::write(&temporary_path, b"not a sqlite database")
                .expect("invalid bootstrap temporary");
        } else {
            let pool = create_pool_at(&database_path, 1).expect("initial fresh database");
            pool.get()
                .expect("database connection")
                .execute(
                    "INSERT INTO users (username, email, hashed_password) VALUES ('unexpected', 'unexpected@example.com', 'hash')",
                    [],
                )
                .expect("unexpected bootstrap data");
            drop(pool);
            std::fs::rename(&database_path, &temporary_path)
                .expect("simulate nonempty pre-publish crash");
        }

        let error =
            create_pool_at(&database_path, 1).expect_err("untrusted bootstrap temporary must fail");
        assert!(error.to_string().contains("bootstrap temporary"), "{error}");
        assert!(!database_path.exists());
        assert!(temporary_path.exists());
    }
}

#[test]
fn bootstrap_rejects_a_schema_shaped_file_without_momento_identity() {
    let directory = TempDir::new().expect("bootstrap directory");
    let database_path = directory.path().join("database.sqlite");
    let temporary_path = directory
        .path()
        .join(".database.sqlite.momento-bootstrap.tmp");
    let pool = create_pool_at(&database_path, 1).expect("initial fresh database");
    pool.get()
        .expect("database connection")
        .pragma_update(None, "application_id", 0)
        .expect("remove Momento identity");
    drop(pool);
    std::fs::rename(&database_path, &temporary_path).expect("simulate foreign bootstrap file");

    let error = create_pool_at(&database_path, 1)
        .expect_err("schema shape without Momento identity must not be deleted");
    assert!(error.to_string().contains("database identity"), "{error}");
    assert!(!database_path.exists());
    assert!(temporary_path.exists());
}
