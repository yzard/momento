use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::database::create_pool_at;

use super::{
    ConfigFileIdentity, ExecutorHandles, ExecutorRuntime, RuntimeSizing, SystemTimezoneSnapshot,
    WORKER_STACK_BYTES,
};

const NETWORK_WORKERS: usize = 2;

pub struct RuntimeBuilder {
    sizing: RuntimeSizing,
    database_path: PathBuf,
    config_identity: ConfigFileIdentity,
    static_dir: PathBuf,
}

impl RuntimeBuilder {
    pub fn new(
        sizing: RuntimeSizing,
        database_path: impl AsRef<Path>,
        config_identity: ConfigFileIdentity,
        static_dir: impl AsRef<Path>,
    ) -> Self {
        Self {
            sizing,
            database_path: database_path.as_ref().to_path_buf(),
            config_identity,
            static_dir: static_dir.as_ref().to_path_buf(),
        }
    }

    pub fn build(self) -> Result<ApplicationRuntime, RuntimeBuildError> {
        self.sizing
            .validate_pre_spawn_environment()
            .map_err(|error| RuntimeBuildError::Preflight(error.to_string()))?;

        let system_timezone =
            SystemTimezoneSnapshot::load().map_err(RuntimeBuildError::Preflight)?;
        let sqlite_workers = self.sizing.sqlite_workers;
        let database_path = self.database_path;
        let data_dir = database_path
            .parent()
            .ok_or_else(|| RuntimeBuildError::Preflight("database path has no parent".to_string()))?
            .to_path_buf();
        let (executor_runtime, executor_handles) = ExecutorRuntime::start_with_pool_factory(
            &self.sizing,
            self.config_identity,
            data_dir,
            Some(self.static_dir),
            || create_pool_at(&database_path, sqlite_workers).map_err(|error| error.to_string()),
        )
        .map_err(RuntimeBuildError::Executor)?;

        let bootstrap_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|error| RuntimeBuildError::Executor(error.to_string()))?;
        let authentication_dummy_hash = match bootstrap_runtime.block_on(async {
            executor_handles
                .cpu
                .initialize_reverse_geocoder_durable()
                .await?;
            executor_handles
                .cpu
                .hash_password_durable("momento-password-verification-placeholder".to_string())
                .await
        }) {
            Ok(hash) => hash,
            Err(error) => {
                bootstrap_runtime
                    .block_on(executor_runtime.shutdown())
                    .map_err(RuntimeBuildError::Executor)?;
                return Err(RuntimeBuildError::Executor(error.to_string()));
            }
        };

        let network_worker_index = AtomicUsize::new(0);
        let network_runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(NETWORK_WORKERS)
            .thread_stack_size(WORKER_STACK_BYTES as usize)
            .thread_name_fn(move || {
                let index = network_worker_index.fetch_add(1, Ordering::Relaxed);
                format!("momento-io-network-{index}")
            })
            .enable_io()
            .enable_time()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let cleanup_runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .map_err(|cleanup_error| {
                        RuntimeBuildError::Network(format!(
                            "{error}; cleanup runtime creation also failed: {cleanup_error}"
                        ))
                    })?;
                cleanup_runtime
                    .block_on(executor_runtime.shutdown())
                    .map_err(RuntimeBuildError::Executor)?;
                return Err(RuntimeBuildError::Network(error.to_string()));
            }
        };

        Ok(ApplicationRuntime {
            sizing: self.sizing,
            executor_runtime: Some(executor_runtime),
            executor_handles,
            authentication_dummy_hash,
            system_timezone,
            network_runtime,
        })
    }
}

pub struct ApplicationRuntime {
    sizing: RuntimeSizing,
    executor_runtime: Option<ExecutorRuntime>,
    executor_handles: ExecutorHandles,
    authentication_dummy_hash: String,
    system_timezone: SystemTimezoneSnapshot,
    network_runtime: tokio::runtime::Runtime,
}

impl ApplicationRuntime {
    pub fn sizing(&self) -> &RuntimeSizing {
        &self.sizing
    }

    pub fn executors(&self) -> ExecutorHandles {
        self.executor_handles.clone()
    }

    pub fn authentication_dummy_hash(&self) -> &str {
        &self.authentication_dummy_hash
    }

    pub fn system_timezone(&self) -> SystemTimezoneSnapshot {
        self.system_timezone.clone()
    }

    pub fn block_on<F>(mut self, future: F) -> Result<F::Output, RuntimeBuildError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let output = self
            .network_runtime
            .block_on(self.network_runtime.spawn(future))
            .map_err(|error| RuntimeBuildError::Network(format!("network task failed: {error}")))?;
        drop(self.executor_handles);
        let executor_runtime = self.executor_runtime.take().ok_or_else(|| {
            RuntimeBuildError::Executor("executor runtime is missing".to_string())
        })?;
        self.network_runtime
            .block_on(executor_runtime.shutdown())
            .map_err(RuntimeBuildError::Executor)?;
        self.network_runtime.shutdown_background();
        Ok(output)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeBuildError {
    Preflight(String),
    Executor(String),
    Network(String),
}

impl fmt::Display for RuntimeBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preflight(detail) => write!(formatter, "runtime preflight failed: {detail}"),
            Self::Executor(detail) => write!(formatter, "executor startup failed: {detail}"),
            Self::Network(detail) => write!(formatter, "network runtime startup failed: {detail}"),
        }
    }
}

impl std::error::Error for RuntimeBuildError {}
