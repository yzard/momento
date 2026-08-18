use std::net::SocketAddr;
use std::sync::Arc;

use momento_common::config_cli::{parse_config_command, ConfigCommand};
use momento_common::logging::init_logging;

use momento_api::app::create_app;
use momento_api::auth::{ensure_default_admin, prepare_admin_password_reset};
use momento_api::config::{consume_admin_password_reset, load_config, save_default_config};
use momento_api::constants::{init_paths, paths};
use momento_api::cronjob::run_cronjobs;
use momento_api::database::{create_pool, init_database};
use momento_api::logging::install_panic_hook;
use momento_api::processor::ai;
use momento_api::processor::import::{
    recover_interrupted_imports, recover_webdav_claims, start_webdav_import_job,
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
        &paths.previews,
        &paths.imports,
        &paths.albums,
        &paths.trash,
        &paths.webdav,
    ] {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

fn start_background_tasks(
    config: Arc<momento_api::config::Config>,
    pool: momento_api::database::DbPool,
    llm_transport: momento_api::processor::ai::transport::TransportHandle,
    webdav_request_gate: momento_api::webdav::WebDAVRequestGate,
) {
    let config_clone = Arc::clone(&config);
    let pool_clone = pool.clone();

    tokio::spawn(async move {
        if let Ok(conn) = pool_clone.get() {
            let _ = cleanup_expired_trash(&conn);
        }

        run_cronjobs(config_clone, pool_clone).await;
    });

    let metadata_config = Arc::clone(&config);
    let metadata_pool = pool.clone();
    tokio::spawn(async move {
        metadata_worker::run(metadata_config, metadata_pool).await;
    });

    let ai_config = Arc::clone(&config);
    let ai_pool = pool.clone();
    tokio::spawn(async move {
        ai::run(ai_config, ai_pool, llm_transport).await;
    });

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
    let face_group_similarity_threshold = config.llm.face_group_similarity_threshold;
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(5);
        loop {
            if let Err(error) = momento_api::processor::face_detection::finalize_ready_runs(
                &face_pool,
                face_group_similarity_threshold,
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
    recover_webdav_claims(&paths().webdav).expect("Failed to recover interrupted WebDAV imports");

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
    let config = Arc::new(config);

    let llm_transport = momento_api::processor::ai::transport::TransportHandle::default();
    let webdav_request_gate = Arc::new(tokio::sync::Semaphore::new(
        config.webdav.max_concurrent_requests,
    ));

    // Start background tasks
    start_background_tasks(
        Arc::clone(&config),
        pool.clone(),
        llm_transport.clone(),
        Arc::clone(&webdav_request_gate),
    );

    // Create the application
    let app = create_app(
        config.clone(),
        pool,
        llm_transport,
        webdav_request_gate,
        admin_password_reset_user_id,
    );

    tracing::info!("Starting Momento API on {}", addr);

    axum::serve(listener, app).await.expect("Server failed");
}
