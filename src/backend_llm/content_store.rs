use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use momento_common::llm::JobInputDescriptor;

use crate::error::ServiceError;

const SOURCE_FILE: &str = "source";
pub const NORMALIZED_FILE: &str = "normalized.tiff";

pub struct ContentStore {
    root: PathBuf,
}

impl ContentStore {
    pub fn new(queue_dir: &Path) -> Result<Self, ServiceError> {
        let root = queue_dir.join("content");
        fs::create_dir_all(&root).map_err(io_error)?;
        let store = Self { root };
        store.remove_normalization_temporaries()?;
        store.remove_unreferenced_content()?;
        Ok(store)
    }

    pub fn link_cached_input(
        &self,
        descriptor: &JobInputDescriptor,
        destination: &Path,
    ) -> Result<bool, ServiceError> {
        let source = self.source_path(&descriptor.content_hash);
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(io_error(error)),
        };
        if !metadata.file_type().is_file() || metadata.len() != descriptor.byte_size {
            return Err(ServiceError::Internal(format!(
                "cached input {} does not match its descriptor",
                descriptor.content_hash
            )));
        }
        fs::hard_link(source, destination).map_err(io_error)?;
        sync_directory(destination.parent().ok_or_else(|| {
            ServiceError::Internal("cached input destination has no parent".to_string())
        })?)?;
        Ok(true)
    }

    pub fn input_is_cached(&self, descriptor: &JobInputDescriptor) -> Result<bool, ServiceError> {
        let source = self.source_path(&descriptor.content_hash);
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(io_error(error)),
        };
        if !metadata.file_type().is_file() || metadata.len() != descriptor.byte_size {
            return Err(ServiceError::Internal(format!(
                "cached input {} does not match its descriptor",
                descriptor.content_hash
            )));
        }
        Ok(true)
    }

    pub fn publish_input(
        &self,
        descriptor: &JobInputDescriptor,
        staged_input: &Path,
    ) -> Result<bool, ServiceError> {
        let content_directory = self.content_directory(&descriptor.content_hash);
        fs::create_dir_all(&content_directory).map_err(io_error)?;
        let source = content_directory.join(SOURCE_FILE);
        match fs::symlink_metadata(&source) {
            Ok(metadata) => {
                if !metadata.file_type().is_file() || metadata.len() != descriptor.byte_size {
                    return Err(ServiceError::Internal(format!(
                        "cached input {} does not match its descriptor",
                        descriptor.content_hash
                    )));
                }
                fs::remove_file(staged_input).map_err(io_error)?;
                fs::hard_link(&source, staged_input).map_err(io_error)?;
                sync_directory(staged_input.parent().ok_or_else(|| {
                    ServiceError::Internal("staged input destination has no parent".to_string())
                })?)?;
                return Ok(false);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::hard_link(staged_input, &source).map_err(io_error)?;
                sync_directory(&content_directory)?;
            }
            Err(error) => return Err(io_error(error)),
        }
        sync_directory(
            staged_input
                .parent()
                .ok_or_else(|| ServiceError::Internal("staged input has no parent".to_string()))?,
        )?;
        Ok(true)
    }

    pub fn normalized_path(&self, content_hash: &str) -> PathBuf {
        self.content_directory(content_hash).join(NORMALIZED_FILE)
    }

    pub fn link_normalized_input(
        &self,
        content_hash: &str,
        destination: &Path,
    ) -> Result<(), ServiceError> {
        let source = self.normalized_path(content_hash);
        let metadata = fs::symlink_metadata(&source).map_err(io_error)?;
        if !metadata.file_type().is_file() || metadata.len() == 0 {
            return Err(ServiceError::Internal(
                "normalized input cache is invalid".to_string(),
            ));
        }
        match fs::remove_file(destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
        fs::hard_link(source, destination).map_err(io_error)?;
        sync_directory(destination.parent().ok_or_else(|| {
            ServiceError::Internal("normalized input destination has no parent".to_string())
        })?)
    }

    pub fn remove_job_directory(
        &self,
        job_directory: &Path,
        inputs: &[JobInputDescriptor],
    ) -> Result<u64, ServiceError> {
        match fs::remove_dir_all(job_directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(io_error(error)),
        }
        if let Some(parent) = job_directory.parent() {
            sync_directory(parent)?;
        }
        let mut released_bytes = 0_u64;
        for descriptor in inputs {
            released_bytes = released_bytes
                .checked_add(self.remove_content_if_unreferenced(&descriptor.content_hash)?)
                .ok_or_else(|| {
                    ServiceError::Internal("released content byte count overflowed".to_string())
                })?;
        }
        Ok(released_bytes)
    }

    pub fn remove_unreferenced_content(&self) -> Result<(), ServiceError> {
        for entry in fs::read_dir(&self.root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if !entry.file_type().map_err(io_error)?.is_dir() {
                return Err(ServiceError::Internal(
                    "content store contains a non-directory entry".to_string(),
                ));
            }
            let content_hash = entry.file_name().to_string_lossy().into_owned();
            self.remove_content_if_unreferenced(&content_hash)?;
        }
        Ok(())
    }

    fn remove_normalization_temporaries(&self) -> Result<(), ServiceError> {
        for content_entry in fs::read_dir(&self.root).map_err(io_error)? {
            let content_entry = content_entry.map_err(io_error)?;
            if !content_entry.file_type().map_err(io_error)?.is_dir() {
                continue;
            }
            let mut removed = false;
            for entry in fs::read_dir(content_entry.path()).map_err(io_error)? {
                let entry = entry.map_err(io_error)?;
                let filename = entry.file_name();
                let filename = filename.to_string_lossy();
                let is_temporary = (filename.starts_with(".normalized-")
                    && filename.ends_with(".tiff"))
                    || filename == "normalized.json.tmp";
                if !is_temporary {
                    continue;
                }
                if !entry.file_type().map_err(io_error)?.is_file() {
                    return Err(ServiceError::Internal(format!(
                        "normalization temporary path is not a file: {}",
                        entry.path().display()
                    )));
                }
                fs::remove_file(entry.path()).map_err(io_error)?;
                removed = true;
            }
            if removed {
                sync_directory(&content_entry.path())?;
            }
        }
        Ok(())
    }

    fn remove_content_if_unreferenced(&self, content_hash: &str) -> Result<u64, ServiceError> {
        let content_directory = self.content_directory(content_hash);
        let source = content_directory.join(SOURCE_FILE);
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::remove_dir_all(&content_directory) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
                    Err(error) => return Err(io_error(error)),
                }
                sync_directory(&self.root)?;
                return Ok(0);
            }
            Err(error) => return Err(io_error(error)),
        };
        if metadata.nlink() > 1 {
            return Ok(0);
        }
        let released_bytes = metadata.len();
        fs::remove_dir_all(&content_directory).map_err(io_error)?;
        sync_directory(&self.root)?;
        Ok(released_bytes)
    }

    fn content_directory(&self, content_hash: &str) -> PathBuf {
        self.root.join(content_hash)
    }

    fn source_path(&self, content_hash: &str) -> PathBuf {
        self.content_directory(content_hash).join(SOURCE_FILE)
    }
}

fn sync_directory(path: &Path) -> Result<(), ServiceError> {
    fs::File::open(path)
        .map_err(io_error)?
        .sync_all()
        .map_err(io_error)
}

fn io_error(error: std::io::Error) -> ServiceError {
    ServiceError::Internal(error.to_string())
}
