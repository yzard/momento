use momento_api::database::{create_pool_at, init_database};
use tempfile::TempDir;

#[test]
fn file_pool_enables_wal_and_busy_timeout() {
    let directory = TempDir::new().expect("Failed to create temporary directory");
    let database_path = directory.path().join("database.sqlite");
    let pool = create_pool_at(&database_path).expect("Failed to create database pool");
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

    assert_eq!(journal_mode.to_lowercase(), "wal");
    assert_eq!(busy_timeout, 5_000);
    assert_eq!(synchronous, 1);
}

#[test]
fn wal_reader_is_not_blocked_by_uncommitted_writer() {
    let directory = TempDir::new().expect("Failed to create temporary directory");
    let database_path = directory.path().join("database.sqlite");
    let pool = create_pool_at(&database_path).expect("Failed to create database pool");
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
