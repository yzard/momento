use std::path::PathBuf;
use std::sync::Arc;

use momento_common::logging::init_logging;
use tracing::info;

use llm_service::config::Config;
use llm_service::provider::ServiceManager;
use llm_service::routes::{serve, AppState};
use llm_service::scheduler::Scheduler;

fn config_path() -> Result<PathBuf, String> {
    let mut args = std::env::args().skip(1);
    let Some(arg) = args.next() else {
        return Err("missing required argument: -c|--config PATH".to_string());
    };
    if arg == "-h" || arg == "--help" {
        println!("Usage: llm-service -c|--config PATH");
        std::process::exit(0);
    }
    if arg != "-c" && arg != "--config" {
        return Err(format!("unknown argument: {arg}"));
    }

    let path = args
        .next()
        .ok_or_else(|| format!("{arg} requires a config path"))?;
    if let Some(extra) = args.next() {
        return Err(format!("unknown argument: {extra}"));
    }
    Ok(PathBuf::from(path))
}

#[tokio::main]
async fn main() {
    let path = match config_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let config = match Config::load(&path) {
        Ok(config) => Arc::new(config),
        Err(error) => {
            eprintln!("Failed to load {}: {error}", path.display());
            std::process::exit(1);
        }
    };
    let _logging_guard = match init_logging(&config.server.data_dir, "llm-service", "info") {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("Failed to initialize logging: {error}");
            std::process::exit(1);
        }
    };
    let address = format!("{}:{}", config.server.host, config.server.port);
    let listener = match tokio::net::TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!("Failed to bind {address}: {error}");
            std::process::exit(1);
        }
    };

    info!("Starting LLM service on {}", listener.local_addr().unwrap());
    let manager = Arc::new(tokio::sync::Mutex::new(ServiceManager::new(Arc::clone(
        &config,
    ))));
    let scheduler = Arc::new(
        Scheduler::new(
            config.server.queue_dir(),
            config.server.scheduler.clone(),
            config.callback.clone(),
            Arc::clone(&manager),
        )
        .expect("Failed to initialize LLM disk queue"),
    );
    tokio::spawn(Arc::clone(&scheduler).run());
    let state = AppState {
        config,
        manager,
        scheduler,
    };
    if let Err(error) = serve(listener, state).await {
        tracing::error!("LLM service failed: {error}");
        std::process::exit(1);
    }
}
