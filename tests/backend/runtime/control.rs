use std::time::Duration;

use momento_api::runtime::{
    CronTaskId, DurableSourceId, SchedulerAdmissionKind, SchedulerControlSource, SchedulerState,
};

async fn acquire_new_maintenance(
    scheduler: &momento_api::runtime::SchedulerHandle,
) -> Result<momento_api::runtime::DurableAdmission, String> {
    scheduler
        .acquire_durable(
            DurableSourceId::Maintenance,
            SchedulerAdmissionKind::NewClaim,
        )
        .await
}

#[test]
fn scheduler_registries_have_the_exact_source_owned_members() {
    assert_eq!(DurableSourceId::COUNT, 13);
    assert_eq!(CronTaskId::COUNT, 7);
    assert_eq!(SchedulerControlSource::COUNT, 7);
    assert_eq!(SchedulerAdmissionKind::COUNT, 3);
    assert_eq!(DurableSourceId::ALL[0], DurableSourceId::MediaProcess);
    assert_eq!(CronTaskId::ALL[2], CronTaskId::Deduplicate);
    assert_eq!(
        SchedulerControlSource::ALL[6],
        SchedulerControlSource::ShutdownRequested
    );
}

#[tokio::test]
async fn control_wakeups_are_versioned_and_level_triggered() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let observed = scheduler.control_version(SchedulerControlSource::ConfigChanged);

    let published = scheduler.signal_control(SchedulerControlSource::ConfigChanged);
    assert_eq!(published, observed + 1);
    assert_eq!(
        scheduler
            .wait_for_control_change(SchedulerControlSource::ConfigChanged, observed)
            .await,
        published
    );
    assert_eq!(
        scheduler.control_version(SchedulerControlSource::CancellationChanged),
        0
    );
}

#[tokio::test]
async fn durable_admission_is_bounded_and_released_without_a_queue_command() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let mut admissions = Vec::new();
    for _ in 0..scheduler.durable_capacity() {
        admissions.push(
            acquire_new_maintenance(&scheduler)
                .await
                .expect("durable admission"),
        );
    }
    assert_eq!(
        scheduler.active_durable_for(DurableSourceId::Maintenance),
        scheduler.durable_capacity()
    );
    assert_eq!(
        scheduler.active_durable_kind(SchedulerAdmissionKind::NewClaim),
        scheduler.durable_capacity()
    );

    assert!(tokio::time::timeout(
        Duration::from_millis(20),
        acquire_new_maintenance(&scheduler)
    )
    .await
    .is_err());
    drop(admissions.pop());
    assert_eq!(
        scheduler.active_durable_for(DurableSourceId::Maintenance),
        scheduler.durable_capacity() - 1
    );
    tokio::time::timeout(Duration::from_secs(1), acquire_new_maintenance(&scheduler))
        .await
        .expect("released admission wake")
        .expect("replacement durable admission");
}

#[tokio::test]
async fn outbound_stream_admission_is_bounded_separately_from_durable_work() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let mut outbound = Vec::new();
    for _ in 0..scheduler.outbound_stream_capacity() {
        outbound.push(
            scheduler
                .acquire_outbound_stream()
                .await
                .expect("outbound stream admission"),
        );
    }

    assert_eq!(
        scheduler.active_outbound_stream_total(),
        scheduler.outbound_stream_capacity()
    );
    assert!(tokio::time::timeout(
        Duration::from_millis(20),
        scheduler.acquire_outbound_stream()
    )
    .await
    .is_err());
    let durable = acquire_new_maintenance(&scheduler)
        .await
        .expect("durable work remains independently available");
    drop(durable);

    drop(outbound.pop());
    tokio::time::timeout(Duration::from_secs(1), scheduler.acquire_outbound_stream())
        .await
        .expect("released outbound admission wake")
        .expect("replacement outbound admission");
}

#[tokio::test]
async fn durable_claim_tokens_are_unique_and_unregister_on_drop() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let admission = scheduler
        .acquire_durable(DurableSourceId::Metadata, SchedulerAdmissionKind::NewClaim)
        .await
        .expect("admission");
    let registration = scheduler
        .register_durable_claim(&admission, "metadata-token".to_string())
        .expect("claim registration");

    assert!(scheduler
        .register_durable_claim(&admission, "metadata-token".to_string())
        .is_err());
    drop(registration);
    scheduler
        .register_durable_claim(&admission, "metadata-token".to_string())
        .expect("released token registration");
}

#[test]
fn request_admission_is_non_waiting_and_uses_the_derived_limit() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let mut admissions = Vec::new();
    while let Ok(admission) = scheduler.try_acquire_request() {
        admissions.push(admission);
    }

    assert_eq!(admissions.len(), 32);
    assert!(scheduler.try_acquire_request().is_err());
    drop(admissions.pop());
    assert!(scheduler.try_acquire_request().is_ok());
}

#[test]
fn connection_admission_is_non_waiting_and_uses_the_derived_limit() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let mut admissions = Vec::new();
    while let Ok(admission) = scheduler.try_acquire_connection() {
        admissions.push(admission);
    }
    assert_eq!(admissions.len(), scheduler.connection_capacity());
    assert_eq!(scheduler.active_connection_total(), admissions.len());
    assert!(scheduler.try_acquire_connection().is_err());
    admissions.pop();
    assert!(scheduler.try_acquire_connection().is_ok());
}

#[test]
fn request_admission_converts_once_to_a_separately_bounded_stream_session() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let admission =
        momento_api::runtime::HttpRequestAdmission::acquire(&scheduler).expect("request admission");
    assert_eq!(scheduler.active_request_total(), 1);
    assert_eq!(scheduler.active_stream_total(), 0);

    admission.convert_to_stream().expect("stream conversion");
    assert_eq!(scheduler.active_request_total(), 0);
    assert_eq!(scheduler.active_stream_total(), 1);
    admission
        .convert_to_stream()
        .expect("idempotent stream conversion");
    assert_eq!(scheduler.active_stream_total(), 1);

    drop(admission);
    assert_eq!(scheduler.active_stream_total(), 0);
}

#[tokio::test]
async fn scheduler_quiescing_rejects_new_admission_and_wakes_waiters() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let mut admissions = Vec::new();
    for _ in 0..scheduler.durable_capacity() {
        admissions.push(
            acquire_new_maintenance(&scheduler)
                .await
                .expect("admission"),
        );
    }
    let waiting_scheduler = scheduler.clone();
    let waiter = tokio::spawn(async move { acquire_new_maintenance(&waiting_scheduler).await });
    tokio::task::yield_now().await;

    scheduler.begin_quiescing().expect("begin quiescing");

    assert!(waiter.await.expect("waiter task").is_err());
    assert!(scheduler.try_acquire_request().is_err());
    drop(admissions);
}

#[tokio::test]
async fn graceful_shutdown_waits_for_active_admissions_before_stopping() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let admission = scheduler
        .acquire_durable(
            DurableSourceId::Metadata,
            SchedulerAdmissionKind::ExistingClaimCompletion,
        )
        .await
        .expect("completion admission");
    let shutdown_scheduler = scheduler.clone();
    let shutdown = tokio::spawn(async move {
        shutdown_scheduler
            .finish_shutdown(Duration::from_secs(1))
            .await
    });
    tokio::task::yield_now().await;
    assert_eq!(scheduler.state(), SchedulerState::SecuringMutations);
    assert!(!shutdown.is_finished());
    drop(admission);
    shutdown
        .await
        .expect("shutdown task")
        .expect("graceful shutdown");
    assert_eq!(scheduler.state(), SchedulerState::Stopped);
}

#[tokio::test]
async fn shutdown_deadline_reports_exact_registered_durable_claims() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let admission = scheduler
        .acquire_durable(
            DurableSourceId::Metadata,
            SchedulerAdmissionKind::ExistingClaimCompletion,
        )
        .await
        .expect("completion admission");
    let claim = scheduler
        .register_durable_claim(&admission, "metadata-row-42-token".to_string())
        .expect("claim registration");

    let error = scheduler
        .finish_shutdown(Duration::from_millis(1))
        .await
        .expect_err("active claim must exhaust the shutdown grace");
    assert!(error.contains("Metadata=1"), "{error}");
    assert!(error.contains("Metadata:metadata-row-42-token"), "{error}");

    drop((claim, admission));
}

#[tokio::test]
async fn ai_finalization_wake_notifies_deduplicate_and_face_workers() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    let first_scheduler = scheduler.clone();
    let second_scheduler = scheduler.clone();
    let first = tokio::spawn(async move {
        first_scheduler.ai_finalization_notified().await;
    });
    let second = tokio::spawn(async move {
        second_scheduler.ai_finalization_notified().await;
    });
    tokio::task::yield_now().await;

    scheduler.wake_ai_finalization();

    tokio::time::timeout(Duration::from_secs(1), first)
        .await
        .expect("deduplicate finalization wake")
        .expect("deduplicate waiter");
    tokio::time::timeout(Duration::from_secs(1), second)
        .await
        .expect("face finalization wake")
        .expect("face waiter");
}

#[tokio::test]
async fn shutdown_states_reject_only_new_claims_until_stopped() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);

    assert_eq!(scheduler.state(), SchedulerState::Running);
    scheduler
        .transition_to(SchedulerState::Quiescing)
        .expect("quiesce");
    assert!(acquire_new_maintenance(&scheduler).await.is_err());

    let completion = scheduler
        .acquire_durable(
            DurableSourceId::Metadata,
            SchedulerAdmissionKind::ExistingClaimCompletion,
        )
        .await
        .expect("existing claim completion remains admissible");
    let recovery = scheduler
        .acquire_durable(
            DurableSourceId::JournalRecovery,
            SchedulerAdmissionKind::RecoveryHandoff,
        )
        .await
        .expect("recovery handoff remains admissible");
    drop((completion, recovery));

    scheduler
        .transition_to(SchedulerState::SecuringMutations)
        .expect("secure mutations");
    scheduler
        .transition_to(SchedulerState::Draining)
        .expect("drain");
    scheduler
        .transition_to(SchedulerState::Stopped)
        .expect("stop");
    assert!(scheduler
        .acquire_durable(
            DurableSourceId::Metadata,
            SchedulerAdmissionKind::ExistingClaimCompletion,
        )
        .await
        .is_err());
}

#[test]
fn shutdown_state_transitions_are_strictly_ordered() {
    let pool = crate::test_utils::create_test_db();
    let scheduler = crate::test_utils::test_scheduler(pool);
    assert!(scheduler.transition_to(SchedulerState::Draining).is_err());
    assert_eq!(scheduler.state(), SchedulerState::Running);
}

#[test]
fn business_modules_do_not_create_independent_rust_tasks_or_threads() {
    fn inspect(directory: &std::path::Path, violations: &mut Vec<String>) {
        for entry in std::fs::read_dir(directory).expect("source directory") {
            let entry = entry.expect("source entry");
            let path = entry.path();
            if path.is_dir() {
                if path.ends_with("executor")
                    || path.ends_with("runtime")
                    || path.ends_with("target")
                {
                    continue;
                }
                inspect(&path, violations);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs")
                || path.ends_with("main.rs")
            {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("Rust source");
            for forbidden in [
                "tokio::spawn(",
                "tokio::task::spawn_blocking(",
                "std::thread::spawn(",
                "std::thread::Builder",
                "tokio::task::JoinSet",
                "rayon::",
            ] {
                if source.contains(forbidden) {
                    violations.push(format!("{} contains {forbidden}", path.display()));
                }
            }
        }
    }

    let backend_source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    inspect(backend_source, &mut violations);
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn durable_workers_claim_and_finish_work_in_rolling_lanes() {
    let backend_source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let metadata_worker =
        std::fs::read_to_string(backend_source.join("processor/metadata_worker.rs"))
            .expect("metadata worker source");
    let result_worker = std::fs::read_to_string(backend_source.join("processor/ai/result.rs"))
        .expect("AI result worker source");

    assert!(metadata_worker.contains("process_metadata_lane"));
    assert!(!metadata_worker.contains("let mut claim_guards"));
    assert!(result_worker.contains("process_result_lane"));
    assert!(!result_worker.contains("PreparationOutcome"));
    assert!(!result_worker.contains("mpsc::channel(candidate_count)"));
}
