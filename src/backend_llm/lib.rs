pub mod adapters;
pub mod config;
pub mod content_store;
pub mod error;
pub mod input_normalizer;
pub mod logging;
pub mod provider;
pub mod queue_capacity;
pub mod result_output;
pub mod routes;
pub mod scheduler;
pub mod transport;

pub const VERSION: &str = env!("MOMENTO_VERSION");
