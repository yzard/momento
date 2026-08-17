use std::sync::Arc;

use momento_common::config_cli::{parse_config_command, ConfigCommand};
use momento_common::logging::init_logging;
use tracing::info;

use llm_service::config::Config;
use llm_service::provider::ServiceManager;
use llm_service::routes::{serve, AppState};
use llm_service::scheduler::Scheduler;
use llm_service::transport::ConnectionRegistry;

#[tokio::main]
async fn main() {
    let command = match parse_config_command(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let path = match command {
        ConfigCommand::Help => {
            println!("Usage: llm-service -c|--config PATH [--init-config]");
            return;
        }
        ConfigCommand::Initialize(path) => match Config::save_default(&path) {
            Ok(()) => {
                println!("Default configuration saved to {:?}", path);
                return;
            }
            Err(error) => {
                eprintln!("Failed to save default configuration: {error}");
                std::process::exit(1);
            }
        },
        ConfigCommand::Run(path) => path,
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
    let connections = Arc::new(ConnectionRegistry::default());
    let scheduler = Arc::new(
        Scheduler::new(
            config.server.queue_dir(),
            config.scheduler.clone(),
            Arc::clone(&manager),
            connections.clone(),
        )
        .expect("Failed to initialize LLM disk queue"),
    );
    tokio::spawn(Arc::clone(&scheduler).run());
    let state = AppState {
        config,
        manager,
        scheduler,
        connections,
    };
    if let Err(error) = serve(listener, state).await {
        tracing::error!("LLM service failed: {error}");
        std::process::exit(1);
    }
}
