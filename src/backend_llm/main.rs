use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use llm_service::config::Config;
use llm_service::provider::Provider;
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
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

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
    let provider = match Provider::build(&config).await {
        Ok(provider) => Arc::new(provider),
        Err(error) => {
            eprintln!(
                "Failed to initialize {} provider: {error}",
                provider_name(&config)
            );
            std::process::exit(1);
        }
    };
    let address = format!("{}:{}", config.server.host, config.server.port);
    let listener = match tokio::net::TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("Failed to bind {address}: {error}");
            std::process::exit(1);
        }
    };

    info!("Starting LLM service on {}", listener.local_addr().unwrap());
    let state = AppState { config, provider };
    if let Err(error) = serve(listener, state).await {
        eprintln!("LLM service failed: {error}");
        std::process::exit(1);
    }
}

fn provider_name(config: &Config) -> &'static str {
    match &config.provider {
        llm_service::config::ProviderKind::Baidu => "baidu",
        llm_service::config::ProviderKind::Local => "local",
    }
}
