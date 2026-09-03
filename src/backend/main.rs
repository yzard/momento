use std::net::SocketAddr;
use std::sync::Arc;

use momento_common::config_cli::{parse_config_command, ConfigCommand};

use momento_api::app::{create_app, AppDependencies};
use momento_api::auth::{
    cleanup_refresh_tokens, ensure_default_admin, prepare_admin_password_reset,
};
use momento_api::config::{load_config_with_identity, save_default_config, ConfigManager};
use momento_api::cronjob::run_cronjobs;
use momento_api::logging::{init_logging, install_panic_hook};
use momento_api::processor::ai;
use momento_api::processor::import::{recover_interrupted_imports, start_webdav_import_job};
use momento_api::processor::metadata_worker;
use momento_api::routes::cleanup_expired_trash;
use momento_api::runtime::{
    serve_http1, DurableSourceId, ExecutorHandles, HttpIdleTimeouts, RuntimeBuilder,
    SchedulerAdmissionKind, SchedulerHandle,
};

fn start_background_tasks(
    config_manager: ConfigManager,
    scheduler: SchedulerHandle,
    executors: ExecutorHandles,
    llm_transport: momento_api::processor::ai::transport::TransportHandle,
    webdav_request_gate: momento_api::webdav::WebDAVRequestGate,
    system_timezone: momento_api::runtime::SystemTimezoneSnapshot,
) {
    let config = config_manager.current();
    let sqlite = executors.sqlite.clone();
    let backup_executors = executors.clone();

    let journal_executors = executors.clone();
    let journal_scheduler = scheduler.clone();
    scheduler.spawn_control(async move {
        loop {
            let notified = journal_scheduler.journal_recovery_notified();
            let admission = match journal_scheduler
                .acquire_durable(
                    DurableSourceId::JournalRecovery,
                    SchedulerAdmissionKind::RecoveryHandoff,
                )
                .await
            {
                Ok(admission) => admission,
                Err(error) => {
                    tracing::warn!(error, "Journal recovery stopped");
                    return;
                }
            };
            let recovery =
                momento_api::io::recovery::recover_generic_file_operations(&journal_executors)
                    .await;
            drop(admission);
            match recovery {
                Ok(_) => notified.await,
                Err(error) => {
                    tracing::warn!(error = %error, "Journal recovery deferred after a transient failure");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    });

    let refresh_token_cleanup_interval =
        std::time::Duration::from_secs(config.security.refresh_token_cleanup_interval_seconds);
    let refresh_token_scheduler = scheduler.clone();
    let refresh_token_sqlite = sqlite.clone();
    scheduler.spawn_control(async move {
        loop {
            match refresh_token_scheduler
                .acquire_durable(
                    DurableSourceId::Maintenance,
                    SchedulerAdmissionKind::NewClaim,
                )
                .await
            {
                Ok(_worker_permit) => {
                    if let Err(error) = cleanup_refresh_tokens(&refresh_token_sqlite).await {
                        tracing::warn!("Refresh-token cleanup failed: {error}");
                    }
                    if let Err(error) = refresh_token_sqlite
                        .maintain_file_operation_journal_durable()
                        .await
                    {
                        tracing::warn!("File-operation journal maintenance failed: {error}");
                    }
                }
                Err(error) => {
                    tracing::warn!(error, "Refresh-token cleanup stopped");
                    return;
                }
            }
            tokio::time::sleep(refresh_token_cleanup_interval).await;
        }
    });

    let cronjob_transport = llm_transport.clone();
    let cronjob_config = config_manager.clone();
    let cronjob_scheduler = scheduler.clone();
    let cronjob_sqlite = sqlite.clone();
    let cronjob_cpu = executors.cpu.clone();
    let cleanup_sqlite = sqlite.clone();

    scheduler
        .spawn_scheduler_control(async move {
            if let Ok(_worker_permit) = cronjob_scheduler
                .acquire_durable(
                    DurableSourceId::FileCleanup,
                    SchedulerAdmissionKind::RecoveryHandoff,
                )
                .await
            {
                if let Err(error) = cleanup_expired_trash(&cleanup_sqlite, &cronjob_scheduler).await
                {
                    tracing::warn!(error = %error, "Expired trash cleanup deferred");
                }
            }

            run_cronjobs(
                cronjob_config,
                cronjob_sqlite,
                cronjob_cpu,
                cronjob_transport,
                cronjob_scheduler,
                system_timezone,
            )
            .await;
        })
        .expect("failed to register scheduler-owned AI timers");

    let metadata_config = Arc::clone(&config);
    let metadata_executors = executors.clone();
    let metadata_scheduler = scheduler.clone();
    scheduler.spawn_control(async move {
        metadata_worker::run(metadata_config, metadata_executors, metadata_scheduler).await;
    });

    scheduler.spawn_control(async move {
        momento_api::processor::backup::run(backup_executors).await;
    });

    let ai_config = Arc::clone(&config);
    let ai_executors = executors.clone();
    let ai_transport = llm_transport.clone();
    let ai_scheduler = scheduler.clone();
    scheduler.spawn_control(async move {
        ai::run(ai_config, ai_executors, ai_transport, ai_scheduler).await;
    });

    let ai_result_executors = executors.clone();
    let ai_result_process_config = config.media_process.clone();
    scheduler.spawn_control(async move {
        ai::result::run(ai_result_executors, ai_result_process_config).await;
    });

    let deduplicate_executors = executors.clone();
    let deduplicate_scheduler = scheduler.clone();
    scheduler.spawn_control(async move {
        loop {
            match deduplicate_scheduler
                .acquire_durable(
                    DurableSourceId::DeduplicateFinalization,
                    SchedulerAdmissionKind::NewClaim,
                )
                .await
            {
                Ok(_worker_permit) => {
                    if let Err(error) = momento_api::processor::deduplicator::finalize_ready_runs(
                        &deduplicate_executors,
                    )
                    .await
                    {
                        tracing::warn!("deduplicate finalization failed: {error}");
                    }
                }
                Err(error) => {
                    tracing::warn!(error, "deduplicate finalization stopped");
                    return;
                }
            }
            deduplicate_scheduler.ai_finalization_notified().await;
        }
    });

    let face_group_executors = executors.clone();
    let face_group_config = config.face_group.clone();
    let face_group_scheduler = scheduler.clone();
    scheduler.spawn_control(async move {
        loop {
            match face_group_scheduler
                .acquire_durable(
                    DurableSourceId::FaceGroupFinalization,
                    SchedulerAdmissionKind::NewClaim,
                )
                .await
            {
                Ok(_worker_permit) => {
                    if let Err(error) = momento_api::processor::face_detection::finalize_ready_runs(
                        &face_group_executors,
                        &face_group_config,
                    )
                    .await
                    {
                        tracing::warn!("face grouping finalization failed: {error}");
                    }
                }
                Err(error) => {
                    tracing::warn!(error, "face grouping finalization stopped");
                    return;
                }
            }
            face_group_scheduler.ai_finalization_notified().await;
        }
    });

    let webdav_config = Arc::clone(&config);
    let webdav_request_gate = Arc::clone(&webdav_request_gate);
    let webdav_scheduler = scheduler.clone();
    let webdav_executors = executors;
    scheduler.spawn_control(async move {
        start_webdav_import_job(
            webdav_config,
            webdav_executors,
            webdav_request_gate,
            webdav_scheduler,
        )
        .await;
    });
}

fn main() {
    let command = match parse_config_command(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let config_path = match command {
        ConfigCommand::Help => {
            println!("Usage: momento-api -c|--config PATH [--init-config]");
            return;
        }
        ConfigCommand::Initialize(config_path) => match save_default_config(&config_path) {
            Ok(_) => {
                println!("Default configuration saved to {:?}", config_path);
                return;
            }
            Err(e) => {
                eprintln!("Failed to save default configuration: {}", e);
                std::process::exit(1);
            }
        },
        ConfigCommand::Run(config_path) => config_path,
    };

    // Load configuration
    let loaded_config = match load_config_with_identity(&config_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };
    let runtime_sizing = momento_api::runtime::RuntimeSizing::validate_worker_counts(
        &loaded_config.config.thread_pool,
    )
    .expect("Failed to size Momento runtime");
    let database_path = loaded_config.config.server.data_dir.join("database.sqlite");
    eprintln!("Initializing Momento runtime");
    let application_runtime = RuntimeBuilder::new(
        runtime_sizing.clone(),
        database_path,
        loaded_config.identity.clone(),
        &loaded_config.config.server.static_dir,
    )
    .build()
    .expect("Failed to initialize Momento runtime");
    eprintln!("Momento runtime initialized; recovering durable work");
    let executors = application_runtime.executors();
    let system_timezone = application_runtime.system_timezone();
    let authentication_dummy_hash = application_runtime.authentication_dummy_hash().to_string();
    let scheduler: SchedulerHandle = executors.scheduler.clone();
    let config_manager = ConfigManager::new(loaded_config, &executors);
    if let Err(error) = application_runtime.block_on(async move {
        run(
            config_manager,
            scheduler,
            executors,
            authentication_dummy_hash,
            system_timezone,
        )
        .await;
    }) {
        eprintln!("Momento runtime failed: {error}");
        std::process::exit(1);
    }
}

async fn run(
    config_manager: ConfigManager,
    scheduler: SchedulerHandle,
    executors: ExecutorHandles,
    authentication_dummy_hash: String,
    system_timezone: momento_api::runtime::SystemTimezoneSnapshot,
) {
    let config = config_manager.current();
    let _logging_guard =
        init_logging(executors.file_io.log_event_producer()).expect("Failed to initialize logging");
    install_panic_hook();

    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));
    let startup_config_manager = config_manager.clone();
    let startup_config = config_manager.current();
    let startup_executors = executors.clone();
    let admin_password_reset_user_id = scheduler
        .execute_durable(
            DurableSourceId::JournalRecovery,
            SchedulerAdmissionKind::RecoveryHandoff,
            "startup-recovery",
            async move {
                tracing::info!("Starting consistency-critical startup recovery");
                let resumed_directory_copies =
                    momento_api::webdav::handler::resume_prepared_directory_copies_after_restart(
                        &startup_executors,
                    )
                    .await
                    .expect("Failed to resume interrupted WebDAV directory copies");
                let prepared_rollbacks =
                    momento_api::io::recovery::rollback_prepared_file_operations_after_restart(
                        &startup_executors,
                    )
                    .await
                    .expect("Failed to roll back interrupted prepared file operations");
                let discarded_products =
                    momento_api::io::recovery::discard_incomplete_file_products_after_restart(
                        &startup_executors,
                    )
                    .await
                    .expect("Failed to discard interrupted LLM result products");
                let recovered_journal_entries =
                    momento_api::io::recovery::recover_startup_critical_file_operations(
                        &startup_executors,
                    )
                    .await
                    .expect("Failed to recover consistency-critical file operations");
                tracing::info!(
                    resumed_directory_copies,
                    prepared_rollbacks,
                    discarded_products,
                    recovered_journal_entries,
                    "Consistency-critical journal recovery completed"
                );
                startup_executors
                    .sqlite
                    .recover_import_content_hash_claims_durable()
                    .await
                    .expect("Failed to recover durable import content-hash claims");
                startup_executors
                    .sqlite
                    .recover_metadata_claims_durable()
                    .await
                    .expect("Failed to recover interrupted metadata claims");
                loop {
                    let result_recovery = startup_executors
                        .sqlite
                        .recover_llm_result_state_durable()
                        .await
                        .expect("Failed to recover interrupted LLM result state");
                    if !result_recovery.has_more {
                        break;
                    }
                }
                startup_executors
                    .sqlite
                    .recover_deduplicate_runs_durable()
                    .await
                    .expect("Failed to recover interrupted deduplicate scans");
                startup_executors
                    .sqlite
                    .recover_face_grouping_runs_durable()
                    .await
                    .expect("Failed to recover interrupted face grouping scans");
                momento_api::processor::face_detection::recompute_face_representatives(
                    &startup_executors,
                    &startup_config.face_group,
                )
                .await
                .expect("Failed to recompute face group representatives");
                recover_interrupted_imports(&startup_executors)
                    .await
                    .expect("Failed to recover interrupted imports");
                let admin_id = ensure_default_admin(&startup_executors)
                    .await
                    .expect("Failed to initialize administrator");
                let admin_password_reset_user_id = if startup_config.server.reset_admin_password {
                    prepare_admin_password_reset(&startup_executors, admin_id)
                        .await
                        .expect("Failed to prepare administrator password reset");
                    startup_config_manager
                        .consume_admin_password_reset()
                        .await
                        .expect("Failed to consume administrator password reset");
                    tracing::warn!(
                        "Temporary administrator password reset active for this server process; sign in as admin/admin and change the password"
                    );
                    Some(admin_id)
                } else {
                    None
                };
                admin_password_reset_user_id
            },
        )
        .await
        .expect("Startup recovery worker failed");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");
    let llm_transport = momento_api::processor::ai::transport::TransportHandle::default();
    let webdav_request_gate = Arc::new(tokio::sync::RwLock::new(()));

    // Start background tasks
    start_background_tasks(
        config_manager.clone(),
        scheduler.clone(),
        executors.clone(),
        llm_transport.clone(),
        Arc::clone(&webdav_request_gate),
        system_timezone,
    );

    // Create the application
    let app = create_app(
        config_manager,
        AppDependencies {
            executors,
            authentication_dummy_hash,
            llm_transport,
            webdav_request_gate,
            admin_password_reset_user_id,
        },
    );

    scheduler
        .execute_durable(
            DurableSourceId::Maintenance,
            SchedulerAdmissionKind::RecoveryHandoff,
            "startup-log",
            async move {
                tracing::info!("Starting Momento API on {}", addr);
            },
        )
        .await
        .expect("Startup log worker failed");

    let server_result = serve_http1(
        listener,
        app,
        scheduler,
        shutdown_signal(),
        std::time::Duration::from_secs(30),
        HttpIdleTimeouts::SOURCE_OWNED,
    )
    .await;
    server_result.expect("Server failed");
}

async fn shutdown_signal() {
    let interrupt = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }
}
