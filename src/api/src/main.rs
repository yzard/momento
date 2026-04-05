use momento_api::app::create_app;
use momento_api::auth::hash_password;
use momento_api::config::{load_config, save_default_config};
use momento_api::constants::{
    CONFIG_PATH, DATA_DIR, IMPORTS_DIR, ORIGINALS_DIR, PREVIEWS_DIR, THUMBNAILS_DIR, WEBDAV_DIR,
};
use momento_api::database::{create_pool, init_database, queries};
use momento_api::logging::{init_logging, install_panic_hook};
use momento_api::processor::importer::start_webdav_import_job;
use momento_api::processor::regenerator::generate_missing_metadata;
use momento_api::routes::cleanup_expired_trash;
use std::net::SocketAddr;
use std::sync::Arc;

fn init_directories() {
    for dir in [
        &*DATA_DIR,
        &*ORIGINALS_DIR,
        &*THUMBNAILS_DIR,
        &*PREVIEWS_DIR,
        &*IMPORTS_DIR,
        &*WEBDAV_DIR,
    ] {
        std::fs::create_dir_all(dir).ok();
    }
}

fn ensure_backtrace_enabled() {
    let has_backtrace = std::env::var_os("RUST_LIB_BACKTRACE").is_some()
        || std::env::var_os("RUST_BACKTRACE").is_some();
    if !has_backtrace {
        std::env::set_var("RUST_LIB_BACKTRACE", "1");
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
        generate_missing_metadata(&config_clone, &pool_clone).await;

        if let Ok(conn) = pool_clone.get() {
            let _ = cleanup_expired_trash(&conn);
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
    if std::env::args().any(|arg| arg == "--init-config") {
        match save_default_config(&CONFIG_PATH) {
            Ok(_) => {
                println!("Default configuration saved to {:?}", *CONFIG_PATH);
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Failed to save default configuration: {}", e);
                std::process::exit(1);
            }
        }
    }

    ensure_backtrace_enabled();

    // Initialize logging
    init_logging();
    install_panic_hook();

    // Load configuration
    let config = Arc::new(load_config(&CONFIG_PATH));

    // Initialize directories
    init_directories();

    // Create database pool
    let pool = create_pool().expect("Failed to create database pool");

    // Initialize database schema
    {
        let conn = pool.get().expect("Failed to get connection");
        init_database(&conn).expect("Failed to initialize database");
    }

    // Create default admin if needed
    create_default_admin(&pool, &config);

    // Start background tasks
    start_background_tasks(Arc::clone(&config), pool.clone());

    // Create the application
    let app = create_app(Arc::clone(&config), pool);

    // Bind to address
    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));
    println!("Starting Momento API on {}", addr);

    // Start server
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server failed");
}
