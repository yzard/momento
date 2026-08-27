use std::net::SocketAddr;
use std::sync::Arc;

use momento_common::config_cli::{parse_config_command, ConfigCommand};
use momento_common::logging::init_logging;

use momento_api::app::create_app;
use momento_api::auth::{ensure_default_admin, prepare_admin_password_reset};
use momento_api::config::{
    consume_admin_password_reset, load_config, save_default_config, ConfigManager,
};
use momento_api::constants::{init_paths, paths};
use momento_api::cronjob::run_cronjobs;
use momento_api::database::{create_pool, init_database};
use momento_api::logging::install_panic_hook;
use momento_api::processor::ai;
use momento_api::processor::import::{
    recover_import_claims, recover_interrupted_imports, start_webdav_import_job,
};
use momento_api::processor::metadata_worker;
use momento_api::routes::cleanup_expired_trash;

fn init_directories() -> std::io::Result<()> {
    let paths = paths();
    for dir in [
        &paths.data,
        &paths.originals,
        &paths.thumbnails,
        &paths.thumbnails_tiny,
        &paths.thumbnails_places,
        &paths.previews,
        &paths.imports,
        &paths.albums,
        &paths.trash,
        &paths.webdav,
        &paths.backups,
    ] {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

fn start_background_tasks(
    config_manager: ConfigManager,
    pool: momento_api::database::DbPool,
    llm_transport: momento_api::processor::ai::transport::TransportHandle,
    webdav_request_gate: momento_api::webdav::WebDAVRequestGate,
) {
    let config = config_manager.current();
    let pool_clone = pool.clone();
    let cronjob_transport = llm_transport.clone();
    let cronjob_config = config_manager.clone();

    tokio::spawn(async move {
        if let Ok(conn) = pool_clone.get() {
            let _ = cleanup_expired_trash(&conn);
        }

        run_cronjobs(cronjob_config, pool_clone, cronjob_transport).await;
    });

    let metadata_config = Arc::clone(&config);
    let metadata_pool = pool.clone();
    tokio::spawn(async move {
        metadata_worker::run(metadata_config, metadata_pool).await;
    });

    let backup_config = Arc::clone(&config);
    let backup_pool = pool.clone();
    tokio::spawn(async move {
        momento_api::processor::backup::run(backup_config, backup_pool).await;
    });

    let ai_config = Arc::clone(&config);
    let ai_pool = pool.clone();
    let ai_transport = llm_transport.clone();
    tokio::spawn(async move {
        ai::run(ai_config, ai_pool, ai_transport).await;
    });

    let ai_result_pool = pool.clone();
    let ai_result_interval =
        std::time::Duration::from_secs(config.llm_result_worker.poll_interval_seconds);
    let ai_result_cpu_processing_concurrency = config.llm_result_worker.cpu_processing_concurrency;
    std::thread::Builder::new()
        .name("ai-result-writer".to_string())
        .spawn(move || {
            ai::result::run(
                ai_result_pool,
                ai_result_interval,
                ai_result_cpu_processing_concurrency,
            )
        })
        .expect("failed to start the AI result writer thread");

    let deduplicate_pool = pool.clone();
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(5);
        loop {
            if let Err(error) =
                momento_api::processor::deduplicator::finalize_ready_runs(&deduplicate_pool)
            {
                tracing::warn!("deduplicate finalization failed: {error}");
            }
            tokio::time::sleep(interval).await;
        }
    });

    let face_pool = pool.clone();
    let face_group_config = config.face_group.clone();
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(5);
        loop {
            if let Err(error) = momento_api::processor::face_detection::finalize_ready_runs(
                &face_pool,
                &face_group_config,
            ) {
                tracing::warn!("face grouping finalization failed: {error}");
            }
            tokio::time::sleep(interval).await;
        }
    });

    let webdav_config = Arc::clone(&config);
    let webdav_pool = pool.clone();
    let webdav_request_gate = Arc::clone(&webdav_request_gate);
    tokio::spawn(async move {
        start_webdav_import_job(webdav_config, webdav_pool, webdav_request_gate).await;
    });
}

#[tokio::main]
async fn main() {
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
    let mut config = match load_config(&config_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };
    // Derive every filesystem location from the configured data directory
    init_paths(&config.server.data_dir);

    let _logging_guard = match init_logging(
        &config.server.data_dir,
        "momento-api",
        "momento_api=info,tower_http=warn",
    ) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("Failed to initialize logging: {error}");
            std::process::exit(1);
        }
    };
    install_panic_hook();

    // Initialize directories
    init_directories().expect("Failed to initialize data directories");
    momento_api::processor::metadata::reverse_geocoding::initialize()
        .expect("Failed to initialize local reverse geocoder");
    recover_import_claims(&paths().imports).expect("Failed to recover interrupted local imports");
    recover_import_claims(&paths().webdav).expect("Failed to recover interrupted WebDAV imports");

    // Create database pool
    let pool = create_pool().expect("Failed to create database pool");

    // Initialize database schema
    {
        let conn = pool.get().expect("Failed to get connection");
        init_database(&conn).expect("Failed to initialize database");
    }

    momento_api::processor::deduplicator::recover_interrupted_runs(&pool)
        .expect("Failed to recover interrupted deduplicate scans");
    momento_api::processor::face_detection::recover_interrupted_runs(&pool)
        .expect("Failed to recover interrupted face grouping scans");
    momento_api::processor::face_detection::recompute_all_group_representatives(
        &pool,
        &config.face_group,
    )
    .expect("Failed to recompute face group representatives");
    recover_interrupted_imports(&pool).expect("Failed to recover interrupted imports");

    let admin_id = ensure_default_admin(&pool).expect("Failed to initialize administrator");
    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");
    let admin_password_reset_user_id = if config.server.reset_admin_password {
        prepare_admin_password_reset(&pool, admin_id)
            .expect("Failed to prepare administrator password reset");
        consume_admin_password_reset(&config_path, &mut config)
            .expect("Failed to consume administrator password reset");
        tracing::warn!(
            "Temporary administrator password reset active for this server process; sign in as admin/admin and change the password"
        );
        Some(admin_id)
    } else {
        None
    };
    let config_manager = ConfigManager::new(config_path, config);
    let config = config_manager.current();

    let llm_transport = momento_api::processor::ai::transport::TransportHandle::default();
    let webdav_request_gate = Arc::new(tokio::sync::Semaphore::new(
        config.webdav.max_concurrent_requests,
    ));

    // Start background tasks
    start_background_tasks(
        config_manager.clone(),
        pool.clone(),
        llm_transport.clone(),
        Arc::clone(&webdav_request_gate),
    );

    // Create the application
    let app = create_app(
        config_manager,
        pool,
        llm_transport,
        webdav_request_gate,
        admin_password_reset_user_id,
    );

    tracing::info!("Starting Momento API on {}", addr);

    axum::serve(listener, app).await.expect("Server failed");
}
