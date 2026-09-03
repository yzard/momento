use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::error::ServiceError;

const CONTENT_SOURCE_FILE: &str = "source";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueCapacityStatus {
    pub required_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub reserved_bytes: u64,
    pub max_queue_bytes: u64,
    pub filesystem_available_bytes: u64,
    pub working_space_reserve_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueCapacityDecision {
    Deferred(QueueCapacityStatus),
    JobTooLarge(QueueCapacityStatus),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueCapacityInput {
    pub content_hash: String,
    pub byte_size: u64,
    pub is_cached: bool,
}

#[derive(Debug, Default)]
struct QueueCapacityState {
    used_bytes: u64,
    reservations: HashMap<String, u64>,
    reserved_content: HashMap<String, String>,
}

#[derive(Debug)]
pub struct QueueCapacityManager {
    content_root: PathBuf,
    filesystem_root: PathBuf,
    max_queue_bytes: u64,
    working_space_reserve_bytes: u64,
    state: Mutex<QueueCapacityState>,
}

#[derive(Debug)]
pub struct QueueCapacityReservation {
    manager: Arc<QueueCapacityManager>,
    job_id: String,
    reserved_bytes: u64,
    reserved_content_hashes: Vec<String>,
    finished: bool,
}

impl QueueCapacityManager {
    pub fn new(
        content_root: PathBuf,
        filesystem_root: PathBuf,
        max_queue_bytes: u64,
        working_space_reserve_bytes: u64,
    ) -> Result<Arc<Self>, ServiceError> {
        let filesystem = filesystem_space(&filesystem_root)?;
        if working_space_reserve_bytes >= filesystem.total_bytes {
            tracing::warn!(
                working_space_reserve_bytes,
                filesystem_total_bytes = filesystem.total_bytes,
                "LLM working-space reserve is not currently satisfiable; new source admission is paused while existing jobs can drain"
            );
        }
        let used_bytes = scan_content_bytes(&content_root)?;
        if used_bytes > max_queue_bytes {
            tracing::warn!(
                used_bytes,
                max_queue_bytes,
                "existing LLM content exceeds the configured queue budget; new admissions are paused"
            );
        }
        tracing::info!(
            used_bytes,
            reserved_bytes = 0_u64,
            max_queue_bytes,
            filesystem_total_bytes = filesystem.total_bytes,
            filesystem_available_bytes = filesystem.available_bytes,
            working_space_reserve_bytes,
            "initialized LLM queue capacity"
        );
        Ok(Arc::new(Self {
            content_root,
            filesystem_root,
            max_queue_bytes,
            working_space_reserve_bytes,
            state: Mutex::new(QueueCapacityState {
                used_bytes,
                reservations: HashMap::new(),
                reserved_content: HashMap::new(),
            }),
        }))
    }

    pub fn try_reserve(
        self: &Arc<Self>,
        job_id: &str,
        content: &[QueueCapacityInput],
    ) -> Result<Result<QueueCapacityReservation, QueueCapacityDecision>, ServiceError> {
        let mut unique_content = HashMap::new();
        for input in content {
            match unique_content.insert(
                input.content_hash.clone(),
                (input.byte_size, input.is_cached),
            ) {
                Some((existing_size, existing_cached))
                    if existing_size != input.byte_size || existing_cached != input.is_cached =>
                {
                    return Err(ServiceError::BadRequest(format!(
                        "content hash {} has conflicting capacity descriptors",
                        input.content_hash
                    )));
                }
                _ => {}
            }
        }
        let filesystem_available_bytes = filesystem_space(&self.filesystem_root)?.available_bytes;
        let mut state = self.lock_state()?;
        if state.reservations.contains_key(job_id) {
            return Err(ServiceError::Conflict(format!(
                "queue capacity is already reserved for job {job_id}"
            )));
        }
        let content_upload_in_progress = unique_content
            .keys()
            .any(|content_hash| state.reserved_content.contains_key(content_hash));
        let required_bytes = unique_content.iter().try_fold(
            0_u64,
            |total, (content_hash, (byte_size, cached))| {
                if *cached && !state.reserved_content.contains_key(content_hash) {
                    return Ok(total);
                }
                total.checked_add(*byte_size).ok_or_else(|| {
                    ServiceError::BadRequest("queue capacity request overflowed".to_string())
                })
            },
        )?;
        let status = self.status(&state, required_bytes, filesystem_available_bytes)?;
        if content_upload_in_progress {
            return Ok(Err(QueueCapacityDecision::Deferred(status)));
        }
        if required_bytes > self.max_queue_bytes {
            return Ok(Err(QueueCapacityDecision::JobTooLarge(status)));
        }
        if required_bytes > status.available_bytes {
            return Ok(Err(QueueCapacityDecision::Deferred(status)));
        }
        state
            .reservations
            .insert(job_id.to_string(), required_bytes);
        let reserved_content_hashes = unique_content
            .into_iter()
            .filter_map(|(content_hash, (_, cached))| (!cached).then_some(content_hash))
            .collect::<Vec<_>>();
        for content_hash in &reserved_content_hashes {
            state
                .reserved_content
                .insert(content_hash.clone(), job_id.to_string());
        }
        Ok(Ok(QueueCapacityReservation {
            manager: Arc::clone(self),
            job_id: job_id.to_string(),
            reserved_bytes: required_bytes,
            reserved_content_hashes,
            finished: false,
        }))
    }

    pub fn release_content(&self, released_bytes: u64) -> Result<(), ServiceError> {
        if released_bytes == 0 {
            return Ok(());
        }
        let mut state = self.lock_state()?;
        state.used_bytes = state
            .used_bytes
            .checked_sub(released_bytes)
            .ok_or_else(|| {
                ServiceError::Internal("LLM queue capacity accounting underflowed".to_string())
            })?;
        Ok(())
    }

    pub fn reconcile(&self) -> Result<(), ServiceError> {
        let used_bytes = scan_content_bytes(&self.content_root)?;
        self.lock_state()?.used_bytes = used_bytes;
        Ok(())
    }

    pub fn snapshot(&self, required_bytes: u64) -> Result<QueueCapacityStatus, ServiceError> {
        let filesystem_available_bytes = filesystem_space(&self.filesystem_root)?.available_bytes;
        let state = self.lock_state()?;
        self.status(&state, required_bytes, filesystem_available_bytes)
    }

    fn finish_reservation(
        &self,
        job_id: &str,
        reserved_bytes: u64,
        reserved_content_hashes: &[String],
        committed_bytes: Option<u64>,
    ) -> Result<(), ServiceError> {
        let mut state = self.lock_state()?;
        let existing = state.reservations.get(job_id).copied().ok_or_else(|| {
            ServiceError::Internal(format!(
                "queue capacity reservation disappeared for {job_id}"
            ))
        })?;
        if existing != reserved_bytes {
            return Err(ServiceError::Internal(format!(
                "queue capacity reservation changed for {job_id}"
            )));
        }
        for content_hash in reserved_content_hashes {
            let owner = state.reserved_content.get(content_hash).ok_or_else(|| {
                ServiceError::Internal(format!(
                    "content reservation disappeared for {content_hash}"
                ))
            })?;
            if owner != job_id {
                return Err(ServiceError::Internal(format!(
                    "content reservation owner changed for {content_hash}"
                )));
            }
        }
        let committed_bytes = committed_bytes.unwrap_or(0);
        if committed_bytes > reserved_bytes {
            return Err(ServiceError::Internal(format!(
                "job {job_id} committed more unique content than it reserved"
            )));
        }
        let used_bytes = state
            .used_bytes
            .checked_add(committed_bytes)
            .ok_or_else(|| {
                ServiceError::Internal("LLM queue used-byte count overflowed".to_string())
            })?;

        state.reservations.remove(job_id);
        for content_hash in reserved_content_hashes {
            state.reserved_content.remove(content_hash);
        }
        state.used_bytes = used_bytes;
        Ok(())
    }

    fn status(
        &self,
        state: &QueueCapacityState,
        required_bytes: u64,
        filesystem_available_bytes: u64,
    ) -> Result<QueueCapacityStatus, ServiceError> {
        let reserved_bytes = state
            .reservations
            .values()
            .try_fold(0_u64, |total, value| total.checked_add(*value))
            .ok_or_else(|| {
                ServiceError::Internal("LLM queue reserved-byte count overflowed".to_string())
            })?;
        let committed_and_reserved =
            state
                .used_bytes
                .checked_add(reserved_bytes)
                .ok_or_else(|| {
                    ServiceError::Internal("LLM queue capacity accounting overflowed".to_string())
                })?;
        let queue_available_bytes = self.max_queue_bytes.saturating_sub(committed_and_reserved);
        let filesystem_admission_bytes = filesystem_available_bytes
            .saturating_sub(self.working_space_reserve_bytes)
            .saturating_sub(reserved_bytes);
        Ok(QueueCapacityStatus {
            required_bytes,
            available_bytes: queue_available_bytes.min(filesystem_admission_bytes),
            used_bytes: state.used_bytes,
            reserved_bytes,
            max_queue_bytes: self.max_queue_bytes,
            filesystem_available_bytes,
            working_space_reserve_bytes: self.working_space_reserve_bytes,
        })
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, QueueCapacityState>, ServiceError> {
        self.state
            .lock()
            .map_err(|_| ServiceError::Internal("LLM queue capacity lock is poisoned".to_string()))
    }
}

impl QueueCapacityReservation {
    pub fn commit(mut self, committed_bytes: u64) -> Result<(), ServiceError> {
        self.manager.finish_reservation(
            &self.job_id,
            self.reserved_bytes,
            &self.reserved_content_hashes,
            Some(committed_bytes),
        )?;
        self.finished = true;
        Ok(())
    }

    pub fn reserved_bytes(&self) -> u64 {
        self.reserved_bytes
    }
}

impl Drop for QueueCapacityReservation {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Err(error) = self.manager.finish_reservation(
            &self.job_id,
            self.reserved_bytes,
            &self.reserved_content_hashes,
            None,
        ) {
            tracing::error!(job_id = self.job_id, error = %error, "failed to release LLM queue capacity reservation");
        }
    }
}

struct FilesystemSpace {
    total_bytes: u64,
    available_bytes: u64,
}

fn filesystem_space(path: &Path) -> Result<FilesystemSpace, ServiceError> {
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        ServiceError::Configuration("LLM queue path contains a null byte".to_string())
    })?;
    let mut statistics = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(path.as_ptr(), statistics.as_mut_ptr()) };
    if result != 0 {
        return Err(ServiceError::Internal(format!(
            "could not inspect LLM queue filesystem: {}",
            std::io::Error::last_os_error()
        )));
    }
    let statistics = unsafe { statistics.assume_init() };
    let fragment_size = statistics.f_frsize;
    let total_bytes = statistics
        .f_blocks
        .checked_mul(fragment_size)
        .ok_or_else(|| ServiceError::Internal("filesystem capacity overflowed".to_string()))?;
    let available_bytes = statistics
        .f_bavail
        .checked_mul(fragment_size)
        .ok_or_else(|| {
            ServiceError::Internal("filesystem available-byte count overflowed".to_string())
        })?;
    Ok(FilesystemSpace {
        total_bytes,
        available_bytes,
    })
}

fn scan_content_bytes(content_root: &Path) -> Result<u64, ServiceError> {
    let mut total = 0_u64;
    for entry in std::fs::read_dir(content_root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if !entry.file_type().map_err(io_error)?.is_dir() {
            return Err(ServiceError::Internal(
                "LLM content store contains a non-directory entry".to_string(),
            ));
        }
        let source = entry.path().join(CONTENT_SOURCE_FILE);
        let metadata = match std::fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(io_error(error)),
        };
        if !metadata.file_type().is_file() {
            return Err(ServiceError::Internal(format!(
                "LLM content source is not a regular file: {}",
                source.display()
            )));
        }
        total = total.checked_add(metadata.len()).ok_or_else(|| {
            ServiceError::Internal("LLM content-store byte count overflowed".to_string())
        })?;
    }
    Ok(total)
}

fn io_error(error: std::io::Error) -> ServiceError {
    ServiceError::Internal(error.to_string())
}
