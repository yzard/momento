use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{fmt::writer::MakeWriterExt, EnvFilter};

use llm_service::config::Config;
use llm_service::provider::ServiceManager;
use llm_service::routes::{serve, AppState};

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
    if let Err(error) = init_logging(&config.logging.file_path) {
        eprintln!("Failed to initialize logging: {error}");
        std::process::exit(1);
    }
    let address = format!("{}:{}", config.general.host, config.general.port);
    let listener = match tokio::net::TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!("Failed to bind {address}: {error}");
            std::process::exit(1);
        }
    };

    info!("Starting LLM service on {}", listener.local_addr().unwrap());
    let state = AppState {
        manager: Arc::new(tokio::sync::Mutex::new(ServiceManager::new(Arc::clone(
            &config,
        )))),
        config,
    };
    if let Err(error) = serve(listener, state).await {
        tracing::error!("LLM service failed: {error}");
        std::process::exit(1);
    }
}

fn init_logging(file_path: &Path) -> io::Result<()> {
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(file.and(std::io::stdout))
        .init();
    Ok(())
}
