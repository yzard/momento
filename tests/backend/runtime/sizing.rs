use momento_api::config::ThreadPoolConfig;
use momento_api::runtime::{
    RuntimeSizing, MAX_CPU_WORKERS, MAX_DERIVED_RUNTIME_BYTES, MAX_IO_WORKERS, MAX_SQLITE_WORKERS,
};

#[test]
fn documented_default_configuration_fits_runtime_budget() {
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig::default())
        .expect("documented runtime defaults must fit");

    assert_eq!(sizing.cpu_workers, 8);
    assert_eq!(sizing.io_workers, 8);
    assert_eq!(sizing.file_workers, 6);
    assert_eq!(sizing.sqlite_workers, 4);
    assert!(sizing.derived_runtime_bytes <= MAX_DERIVED_RUNTIME_BYTES);
    assert_eq!(
        sizing.derived_runtime_bytes,
        sizing
            .bootstrap_peak_bytes
            .max(sizing.pre_listener_initialization_peak_bytes)
            .max(sizing.running_peak_bytes)
    );
    assert_eq!(sizing.active_connections, 128);
    assert_eq!(sizing.active_requests, 64);
    assert_eq!(sizing.active_stream_sessions, 64);
    assert_eq!(sizing.active_outbound_stream_sessions, 12);
    assert_eq!(sizing.active_inbound_durable_streams, 12);
    assert_eq!(sizing.active_file_chunks, 12);
    assert_eq!(sizing.durable_orchestrations, 18);
}

#[test]
fn worker_count_boundaries_are_enforced_before_derivation() {
    for (configuration, field) in [
        (
            ThreadPoolConfig {
                cpu_workers: 0,
                io_workers: 8,
                sqlite_workers: 4,
            },
            "cpu_workers",
        ),
        (
            ThreadPoolConfig {
                cpu_workers: MAX_CPU_WORKERS + 1,
                io_workers: 8,
                sqlite_workers: 4,
            },
            "cpu_workers",
        ),
        (
            ThreadPoolConfig {
                cpu_workers: 8,
                io_workers: 3,
                sqlite_workers: 4,
            },
            "io_workers",
        ),
        (
            ThreadPoolConfig {
                cpu_workers: 8,
                io_workers: MAX_IO_WORKERS + 1,
                sqlite_workers: 4,
            },
            "io_workers",
        ),
        (
            ThreadPoolConfig {
                cpu_workers: 8,
                io_workers: 8,
                sqlite_workers: 0,
            },
            "sqlite_workers",
        ),
        (
            ThreadPoolConfig {
                cpu_workers: 8,
                io_workers: 8,
                sqlite_workers: MAX_SQLITE_WORKERS + 1,
            },
            "sqlite_workers",
        ),
    ] {
        let error = RuntimeSizing::validate_worker_counts(&configuration)
            .expect_err("out-of-range worker count must fail");
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn executor_queue_and_registry_capacities_are_derived() {
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: 2,
        io_workers: 4,
        sqlite_workers: 1,
    })
    .expect("minimum runtime");

    assert_eq!(sizing.cpu_queue_capacity, 8);
    assert_eq!(sizing.file_queue_capacity, 8);
    assert_eq!(sizing.sqlite_queue_capacity, 4);
    assert_eq!(sizing.log_event_capacity, 128);
    assert_eq!(sizing.file_registry_capacity, 73);
    assert!(sizing.scheduler_ingress_capacity > sizing.active_requests);
    assert!(sizing.required_open_files > sizing.active_connections as u64);
}

#[test]
fn default_runtime_passes_pre_spawn_allocation_and_descriptor_checks() {
    let sizing = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig::default())
        .expect("default runtime sizing");

    sizing
        .validate_pre_spawn_environment()
        .expect("default runtime preflight");
}

#[test]
fn over_budget_error_reports_one_field_at_a_time_feasible_worker_counts() {
    let error = RuntimeSizing::validate_worker_counts(&ThreadPoolConfig {
        cpu_workers: MAX_CPU_WORKERS,
        io_workers: MAX_IO_WORKERS,
        sqlite_workers: MAX_SQLITE_WORKERS,
    })
    .expect_err("maximum parser values exceed the combined runtime budget");
    let message = error.to_string();

    assert!(message.contains("maximum feasible workers"), "{message}");
    assert!(message.contains("cpu="), "{message}");
    assert!(message.contains("io="), "{message}");
    assert!(message.contains("sqlite="), "{message}");
}
