use momento_api::config::ThreadPoolConfig;
use momento_api::io::log::{LogSeverity, MAX_LOG_EVENT_BYTES};
use momento_api::runtime::{RuntimeBuilder, RuntimeSizing};

#[test]
fn compose_llm_address_resolves_to_a_declared_service() {
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let compose = std::fs::read_to_string(repository.join("docker-compose.yaml"))
        .expect("playground Docker Compose configuration");
    let service_names = compose
        .lines()
        .filter_map(|line| {
            let name = line.strip_prefix("  ")?.strip_suffix(':')?;
            (!name.starts_with(' ')).then_some(name)
        })
        .collect::<std::collections::HashSet<_>>();
    let llm_host = compose
        .lines()
        .find_map(|line| {
            let address = line
                .trim()
                .strip_prefix("LLM_SERVICE_ADDRESS: ")?
                .trim_matches('"');
            address.split_once(':').map(|(host, _port)| host)
        })
        .expect("LLM_SERVICE_ADDRESS in Docker Compose configuration");

    assert!(
        service_names.contains(llm_host),
        "LLM_SERVICE_ADDRESS host {llm_host:?} is not a declared Compose service"
    );
}

#[test]
fn momento_container_places_runtime_temporary_files_on_the_data_volume() {
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dockerfile =
        std::fs::read_to_string(repository.join("docker/Dockerfile")).expect("Momento Dockerfile");
    let image_magick_policy =
        std::fs::read_to_string(repository.join("docker/imagemagick-policy.xml"))
            .expect("ImageMagick policy");
    let entrypoint = std::fs::read_to_string(repository.join("docker/entrypoint.sh"))
        .expect("Momento entrypoint");
    assert!(dockerfile.contains("TMPDIR=/data/tmp"));
    assert!(dockerfile.contains("TEMP=/data/tmp"));
    assert!(dockerfile.contains("TMP=/data/tmp"));
    assert!(dockerfile.contains("imagemagick-raw"));
    assert!(dockerfile.contains("AVIF DNG GIF HEIC QOI TIFF WEBP"));
    assert!(!image_magick_policy.contains("name=\"time\""));
    assert!(entrypoint.contains("/data/tmp"));
}

#[test]
fn runtime_log_events_are_written_only_by_file_workers_and_count_oversize_drops() {
    let directory = tempfile::tempdir().expect("temporary runtime directory");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("config identity")
        .identity;
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let runtime = RuntimeBuilder::new(
        sizing,
        directory.path().join("database.sqlite"),
        identity,
        directory.path().join("static"),
    )
    .build()
    .expect("application runtime");
    let producer = runtime.executors().file_io.log_event_producer();
    producer.try_emit(
        LogSeverity::Info,
        b"bounded file-worker log event\n".to_vec(),
    );
    producer.try_emit(LogSeverity::Warn, vec![b'x'; MAX_LOG_EVENT_BYTES + 1]);
    assert_eq!(producer.dropped_events().warn, 1);

    runtime
        .block_on(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            drop(producer);
        })
        .expect("clean runtime shutdown");

    let date = chrono::Utc::now().date_naive();
    let log = std::fs::read_to_string(
        directory
            .path()
            .join("logs")
            .join(format!("momento-api.{date}.log")),
    )
    .expect("file-worker log output");
    assert_eq!(log, "bounded file-worker log event\n");
}

#[test]
fn runtime_builder_publishes_named_executor_and_network_workers() {
    let directory = tempfile::tempdir().expect("temporary runtime directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let config_path = directory.path().join("config.toml");
    std::fs::write(
        &config_path,
        "[thread_pool]\ncpu_workers=1\nio_workers=4\nsqlite_workers=1\n",
    )
    .expect("write config");
    let loaded_config =
        momento_api::config::load_config_with_identity(&config_path).expect("load config identity");
    let runtime = RuntimeBuilder::new(
        sizing,
        directory.path().join("database.sqlite"),
        loaded_config.identity,
        directory.path().join("static"),
    )
    .build()
    .expect("application runtime");
    let executors = runtime.executors();

    let names = runtime
        .block_on(async move {
            let network_name = tokio::spawn(async {
                std::thread::current()
                    .name()
                    .expect("named network worker")
                    .to_string()
            })
            .await
            .expect("network task");
            let (_, cpu_name) = executors.cpu.probe_durable(1).await.expect("CPU probe");
            let (_, file_name) = executors
                .file_io
                .probe_durable(2)
                .await
                .expect("file probe");
            let (_, sqlite_name) = executors
                .sqlite
                .probe_durable(3)
                .await
                .expect("SQLite probe");
            (network_name, cpu_name, file_name, sqlite_name)
        })
        .expect("clean runtime shutdown");

    assert!(names.0.starts_with("momento-io-network-"), "{}", names.0);
    assert!(names.1.starts_with("momento-cpu-"), "{}", names.1);
    assert!(names.2.starts_with("momento-io-file-"), "{}", names.2);
    assert!(names.3.starts_with("momento-sqlite-"), "{}", names.3);
}

#[test]
fn journal_mutation_registry_covers_every_active_mutation_owner() {
    let directory = tempfile::tempdir().expect("temporary runtime directory");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 8,
        io_workers: 4,
        sqlite_workers: 4,
    })
    .expect("runtime sizing");
    assert!(sizing.durable_orchestrations > sizing.file_queue_capacity);
    let mutation_capacity = sizing.journal_mutation_registry_capacity;
    let config_path = directory.path().join("config.toml");
    std::fs::write(
        &config_path,
        "[thread_pool]\ncpu_workers=8\nio_workers=4\nsqlite_workers=4\n",
    )
    .expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("load config identity")
        .identity;
    let runtime = RuntimeBuilder::new(
        sizing,
        directory.path().join("database.sqlite"),
        identity,
        directory.path().join("static"),
    )
    .build()
    .expect("application runtime");
    let file_io = runtime.executors().file_io;
    let mut tickets = Vec::new();
    for index in 0..mutation_capacity {
        tickets.push(
            file_io
                .reserve_journal_mutation(&format!("mutation-{index}"), 1)
                .expect("derived mutation slot"),
        );
    }
    assert_eq!(
        file_io
            .reserve_journal_mutation("mutation-over-capacity", 1)
            .expect_err("registry remains bounded"),
        momento_api::io::file::MutationLeaseError::Capacity
    );
    drop(tickets.pop());
    tickets.push(
        file_io
            .reserve_journal_mutation("mutation-replacement", 1)
            .expect("released mutation slot"),
    );
    drop(tickets);
    drop(file_io);

    runtime.block_on(async {}).expect("clean runtime shutdown");
}

#[test]
fn runtime_builder_rejects_a_config_changed_after_initial_read() {
    let directory = tempfile::tempdir().expect("temporary runtime directory");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# initial config\n").expect("write initial config");
    let loaded_config = momento_api::config::load_config_with_identity(&config_path)
        .expect("initial config identity");
    std::fs::write(&config_path, "# changed config\n").expect("replace config contents");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");

    let result = RuntimeBuilder::new(
        sizing,
        directory.path().join("database.sqlite"),
        loaded_config.identity,
        directory.path().join("static"),
    )
    .build();
    let error = match result {
        Ok(_) => panic!("changed config must abort before runtime publication"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("config identity or content changed"));
}

#[test]
fn runtime_builder_holds_an_exclusive_lifetime_lock_on_the_data_directory() {
    let directory = tempfile::tempdir().expect("temporary runtime directory");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("config identity")
        .identity;
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let first = RuntimeBuilder::new(
        sizing.clone(),
        directory.path().join("database.sqlite"),
        identity.clone(),
        directory.path().join("static"),
    )
    .build()
    .expect("first runtime");

    let second = RuntimeBuilder::new(
        sizing,
        directory.path().join("database.sqlite"),
        identity,
        directory.path().join("static"),
    )
    .build();
    let error = match second {
        Ok(_) => panic!("second runtime must not open the same data directory"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("acquire_data_directory_lock"));
    first
        .block_on(async {})
        .expect("first runtime clean shutdown");
}

#[test]
fn runtime_builder_does_not_create_a_missing_data_directory() {
    let directory = tempfile::tempdir().expect("temporary parent directory");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("config identity")
        .identity;
    let missing = directory.path().join("missing");
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");

    let result = RuntimeBuilder::new(
        sizing,
        missing.join("database.sqlite"),
        identity,
        directory.path().join("static"),
    )
    .build();
    assert!(result.is_err());
    assert!(!missing.exists());
}

#[test]
fn file_bootstrap_removes_only_the_reserved_config_update_temporary() {
    let directory = tempfile::tempdir().expect("temporary runtime directory");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# config\n").expect("write config");
    let reserved_temporary = directory.path().join(".config.toml.momento-update.tmp");
    std::fs::write(&reserved_temporary, "# interrupted update\n")
        .expect("write interrupted temporary");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("config identity")
        .identity;
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");

    let runtime = RuntimeBuilder::new(
        sizing,
        directory.path().join("database.sqlite"),
        identity,
        directory.path().join("static"),
    )
    .build()
    .expect("runtime");
    assert!(!reserved_temporary.exists());
    runtime.block_on(async {}).expect("runtime shutdown");
}

#[test]
fn runtime_builder_creates_every_writable_storage_root_before_publication() {
    let directory = tempfile::tempdir().expect("temporary runtime directory");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("config identity")
        .identity;
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");

    let runtime = RuntimeBuilder::new(
        sizing,
        directory.path().join("database.sqlite"),
        identity,
        directory.path().join("missing-static"),
    )
    .build()
    .expect("runtime");
    let capacity = runtime
        .executors()
        .file_io
        .space_budget_snapshot()
        .expect("space budget after root bootstrap");
    assert_eq!(
        capacity.mode,
        momento_api::io::space_budget::SpaceBudgetMode::Running
    );
    assert_eq!(capacity.journal_outstanding_bytes, 0);
    assert!(capacity.epoch > 2);
    assert!(!directory
        .path()
        .join(".database.sqlite.momento-bootstrap.tmp")
        .exists());
    for root in momento_api::io::file::StorageRootId::ALL {
        if root != momento_api::io::file::StorageRootId::Static {
            let root_path = directory.path().join(root.directory_name());
            assert!(root_path.is_dir());
            assert!(std::fs::read_dir(root_path)
                .expect("read bootstrapped root")
                .all(|entry| !entry
                    .expect("root entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".momento-rename-probe-")));
        }
    }
    assert!(directory
        .path()
        .join(momento_api::io::file::StorageRootId::Journal.directory_name())
        .join(momento_api::io::file::LLM_RESULT_INBOX_DIRECTORY)
        .is_dir());
    let sqlite = runtime.executors().sqlite;
    runtime
        .block_on(async move {
            let (sequence, _) = sqlite.probe_durable(41).await.expect("SQLite probe");
            assert_eq!(sequence, 41);
        })
        .expect("runtime shutdown");
}

#[test]
fn runtime_builder_rejects_static_storage_that_overlaps_private_data() {
    let directory = tempfile::tempdir().expect("temporary runtime directory");
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("config identity")
        .identity;
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");

    let result = RuntimeBuilder::new(
        sizing,
        directory.path().join("database.sqlite"),
        identity,
        directory.path(),
    )
    .build();
    let error = match result {
        Ok(_) => panic!("overlapping static root must fail bootstrap"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("must not overlap"));
}

#[test]
fn runtime_builder_rejects_orphan_and_truncated_sqlite_bootstrap_files() {
    for (filename, contents, expected) in [
        (
            "database.sqlite-wal",
            b"orphan".as_slice(),
            "without database.sqlite",
        ),
        ("database.sqlite", b"".as_slice(), "empty or truncated"),
    ] {
        let directory = tempfile::tempdir().expect("temporary runtime directory");
        let config_path = directory.path().join("config.toml");
        std::fs::write(&config_path, "# config\n").expect("write config");
        std::fs::write(directory.path().join(filename), contents).expect("write SQLite fixture");
        let identity = momento_api::config::load_config_with_identity(&config_path)
            .expect("config identity")
            .identity;
        let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
            cpu_workers: 1,
            io_workers: 4,
            sqlite_workers: 1,
        })
        .expect("runtime sizing");
        let result = RuntimeBuilder::new(
            sizing,
            directory.path().join("database.sqlite"),
            identity,
            directory.path().join("missing-static"),
        )
        .build();
        let error = match result {
            Ok(_) => panic!("inconsistent SQLite bootstrap state must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn runtime_builder_rejects_an_existing_schema_that_needs_migration() {
    let directory = tempfile::tempdir().expect("temporary runtime directory");
    let database_path = directory.path().join("database.sqlite");
    let pool = momento_api::database::create_pool_at(&database_path, 1).expect("database pool");
    pool.get()
        .expect("schema connection")
        .execute("CREATE TABLE legacy_migration_debt (id INTEGER)", [])
        .expect("legacy schema fixture");
    drop(pool);
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "# config\n").expect("write config");
    let identity = momento_api::config::load_config_with_identity(&config_path)
        .expect("config identity")
        .identity;
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 1,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("runtime sizing");
    let result = RuntimeBuilder::new(
        sizing,
        database_path,
        identity,
        directory.path().join("missing-static"),
    )
    .build();
    let error = match result {
        Ok(_) => panic!("schema migration must not run implicitly"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("reset the database"), "{error}");
}
