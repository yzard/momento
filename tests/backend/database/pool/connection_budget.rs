use std::os::unix::fs::MetadataExt;

use super::*;
use r2d2::ManageConnection;

#[test]
fn retiring_the_last_pool_connection_does_not_run_an_unbudgeted_checkpoint() {
    let directory = crate::temporary::tempdir().unwrap();
    let path = directory.path().join("database.sqlite");
    prepare_database_file(&path).unwrap();
    initialize_database_file(&path).unwrap();
    let mut connection = Connection::open(&path).unwrap();
    configure_connection(&mut connection).unwrap();
    connection
        .pragma_update(None, "wal_autocheckpoint", 0)
        .unwrap();
    connection.execute("INSERT INTO users(username,email,hashed_password) VALUES ('close','close@example.com','hash')", []).unwrap();
    let before = snapshot(&path);
    let wal = path.with_extension("sqlite-wal");
    let wal_before = snapshot(&wal);
    drop(connection);
    assert_eq!(snapshot(&path), before);
    assert_eq!(snapshot(&wal), wal_before);
    let mut reopened = Connection::open(&path).unwrap();
    configure_connection(&mut reopened).unwrap();
    assert_eq!(
        reopened
            .query_row(
                "SELECT COUNT(*) FROM users WHERE username='close'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    // Explicit/writer-owned checkpoints remain available.
    reopened
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    assert_eq!(std::fs::metadata(wal).unwrap().len(), 0);
}

#[derive(Debug, Eq, PartialEq)]
struct FileSnapshot {
    device: u64,
    inode: u64,
    length: u64,
    blocks: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
    bytes: Vec<u8>,
}

#[test]
fn replacement_connection_creation_checks_out_and_publishes_sqlite_capacity() {
    let directory = crate::temporary::tempdir().expect("temporary database directory");
    let database_path = directory.path().join("database.sqlite");
    prepare_database_file(&database_path).expect("prepare database");
    initialize_database_file(&database_path).expect("initialize database");
    let budget = crate::io::space_budget::DataDirSpaceBudget::from_directory(
        File::open(directory.path()).expect("data directory descriptor"),
    )
    .expect("space budget");
    let allocated = crate::io::space_budget::measure_sqlite_allocation(&database_path)
        .expect("SQLite allocation");
    let mut reconstruction = budget.begin_reconstruction();
    reconstruction.set_allocated_bytes(allocated, 0);
    reconstruction.publish().expect("publish reconstruction");
    budget.mark_running().expect("running budget");
    let connection_budget = Arc::new(RwLock::new(Some(SqliteConnectionBudget {
        budget: budget.clone(),
        database_path: database_path.clone(),
        peak_additional_bytes: 1024 * 1024,
    })));
    let manager = create_connection_manager(&database_path, connection_budget);

    let connection = manager.connect().expect("budgeted replacement connection");
    connection
        .query_row("SELECT 1", [], |_| Ok(()))
        .expect("validated replacement");
    let snapshot = budget.snapshot().expect("space budget snapshot");
    assert_eq!(snapshot.sqlite_outstanding_bytes, 0);
    assert!(snapshot.sqlite_allocated_bytes >= allocated);
}

fn snapshot(path: &Path) -> FileSnapshot {
    let metadata = std::fs::metadata(path).expect("SQLite file metadata");
    FileSnapshot {
        device: metadata.dev(),
        inode: metadata.ino(),
        length: metadata.len(),
        blocks: metadata.blocks(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
        bytes: std::fs::read(path).expect("SQLite file bytes"),
    }
}

#[test]
fn existing_database_read_only_probe_does_not_mutate_main_wal_or_shm() {
    let directory = crate::temporary::tempdir().expect("database directory");
    let database_path = directory.path().join("database.sqlite");
    prepare_database_file(&database_path).expect("fresh database");
    initialize_database_file(&database_path).expect("WAL activation");
    let mut writer = Connection::open(&database_path).expect("writer connection");
    configure_connection(&mut writer).expect("writer configuration");
    writer
        .execute(
            "INSERT INTO users (username, email, hashed_password) VALUES ('probe', 'probe@example.com', 'hash')",
            [],
        )
        .expect("WAL frame");
    writer
        .query_row("SELECT COUNT(*) FROM users", [], |row| row.get::<_, i64>(0))
        .expect("initialize shared memory");

    let paths = [
        database_path.clone(),
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ];
    let before = paths.iter().map(|path| snapshot(path)).collect::<Vec<_>>();

    let read_only =
        open_existing_database_read_only(&database_path).expect("read-only database probe");
    assert_eq!(
        read_only
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get::<_, i64>(0))
            .expect("read through probe"),
        1
    );
    drop(read_only);

    let after = paths.iter().map(|path| snapshot(path)).collect::<Vec<_>>();
    assert_eq!(after, before);
    drop(writer);
}
