use momento_api::app::create_app;
use momento_api::auth::hash_password;
use momento_api::config::{load_config, save_default_config};
use momento_api::constants::{
    CONFIG_PATH, DATA_DIR, IMPORTS_DIR, ORIGINALS_DIR, PREVIEWS_DIR, THUMBNAILS_DIR,
};
use momento_api::database::{
    create_pool, ensure_media_columns, fetch_one, init_database, insert_returning_id, queries,
};
use momento_api::logging::{init_logging, install_panic_hook};
use momento_api::processor::regenerator::generate_missing_metadata;
use momento_api::routes::cleanup_expired_trash;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

fn init_directories() {
    for dir in [
        &*DATA_DIR,
        &*ORIGINALS_DIR,
        &*THUMBNAILS_DIR,
        &*PREVIEWS_DIR,
        &*IMPORTS_DIR,
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
    let existing: Option<i64> =
        fetch_one(&conn, queries::users::CHECK_ADMIN, &[], |row| row.get(0))
            .ok()
            .flatten();

    if existing.is_some() {
        return;
    }

    // Create default admin
    let hashed = match hash_password(&config.admin.password) {
        Ok(h) => h,
        Err(_) => return,
    };

    let _ = insert_returning_id(
        &conn,
        queries::users::INSERT_ADMIN,
        &[
            &config.admin.username,
            &format!("{}@localhost", config.admin.username),
            &hashed,
        ],
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

        // Cleanup expired trash items
        if let Ok(conn) = pool_clone.get() {
            let _ = cleanup_expired_trash(&conn);
        }
    });
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
        ensure_media_columns(&conn).expect("Failed to ensure media columns");
        momento_api::database::ensure_access_control_setup(&conn)
            .expect("Failed to ensure access control");
    }

    // Create default admin if needed
    create_default_admin(&pool, &config);

    // Start background tasks
    start_background_tasks(Arc::clone(&config), pool.clone());

    // Create the application
    let app = create_app(Arc::clone(&config), pool);

    // Bind to address
    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));
    info!("Starting Momento API on {}", addr);

    // Start server
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server failed");
}
