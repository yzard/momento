use std::net::SocketAddr;
use std::sync::Arc;

use momento_common::logging::init_logging;

use momento_api::app::create_app;
use momento_api::auth::hash_password;
use momento_api::config::{load_config, save_default_config};
use momento_api::constants::{init_paths, paths};
use momento_api::cronjob::run_cronjobs;
use momento_api::database::{create_pool, init_database, queries};
use momento_api::logging::install_panic_hook;
use momento_api::processor::ai;
use momento_api::processor::import::recover_interrupted_imports;
use momento_api::processor::import::start_webdav_import_job;
use momento_api::processor::metadata_worker;
use momento_api::routes::cleanup_expired_trash;

struct Cli {
    config_path: std::path::PathBuf,
    init_config: bool,
}

fn parse_cli() -> Result<Cli, String> {
    let mut args = std::env::args().skip(1);
    let mut config_path: Option<std::path::PathBuf> = None;
    let mut init_config = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                let path = args
                    .next()
                    .ok_or_else(|| format!("{arg} requires a config path"))?;
                config_path = Some(std::path::PathBuf::from(path));
            }
            "--init-config" => init_config = true,
            "-h" | "--help" => {
                println!("Usage: momento-api -c|--config PATH [--init-config]");
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    let config_path = config_path.ok_or("missing required argument: -c|--config PATH")?;

    Ok(Cli {
        config_path,
        init_config,
    })
}

fn init_directories() {
    let paths = paths();
    for dir in [
        &paths.data,
        &paths.originals,
        &paths.thumbnails,
        &paths.previews,
        &paths.imports,
        &paths.webdav,
    ] {
        std::fs::create_dir_all(dir).ok();
    }
}

fn create_default_admin(
    pool: &momento_api::database::DbPool,
    config: &momento_api::config::Config,
) {
    let conn = match pool.get() {
        Ok(c) => c,
        Err(_) => return,
    };

    // Check if admin exists
    let existing: Option<i64> = conn
        .query_row(queries::users::CHECK_ADMIN, [], |row| row.get(0))
        .ok();

    if existing.is_some() {
        return;
    }

    // Create default admin
    let hashed = match hash_password(&config.admin.password) {
        Ok(h) => h,
        Err(_) => return,
    };

    let email = format!("{}@localhost", config.admin.username);
    let _ = conn.execute(
        queries::users::INSERT_ADMIN,
        (&config.admin.username, &email, &hashed),
    );
}

fn start_background_tasks(
    config: Arc<momento_api::config::Config>,
    pool: momento_api::database::DbPool,
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
        ai::run(ai_config, ai_pool).await;
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

    if config.webdav.enabled {
        let webdav_config = Arc::clone(&config);
        let webdav_pool = pool.clone();
        tokio::spawn(async move {
            start_webdav_import_job(webdav_config, webdav_pool).await;
        });
    }
}

#[tokio::main]
async fn main() {
    let cli = match parse_cli() {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };

    if cli.init_config {
        match save_default_config(&cli.config_path) {
            Ok(_) => {
                println!("Default configuration saved to {:?}", cli.config_path);
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Failed to save default configuration: {}", e);
                std::process::exit(1);
            }
        }
    }

    // Load configuration
    let config = match load_config(&cli.config_path) {
        Ok(config) => Arc::new(config),
        Err(e) => {
            eprintln!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    // Derive every filesystem location from the configured data directory
    init_paths(&config.storage.data_dir);

    let _logging_guard = match init_logging(
        &config.storage.data_dir,
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
    init_directories();

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

    // Create default admin if needed
    create_default_admin(&pool, &config);

    // Start background tasks
    start_background_tasks(Arc::clone(&config), pool.clone());

    // Create the application
    let app = create_app(Arc::clone(&config), pool);

    // Bind to address
    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));
    tracing::info!("Starting Momento API on {}", addr);

    // Start server
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server failed");
}
