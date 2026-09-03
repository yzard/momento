use std::fs::File;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
pub const MAX_CONFIG_PATH_BYTES: usize = 4096;
pub const MAX_CONFIG_PATH_COMPONENTS: usize = 256;
pub const MAX_CONFIG_COMPONENT_BYTES: usize = 255;
pub const MAX_CONFIG_FILENAME_BYTES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFileIdentity {
    pub canonical_path: PathBuf,
    pub device: u64,
    pub inode: u64,
    pub byte_size: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
    pub sha256: [u8; 32],
}

pub(crate) struct BoundedConfigFile {
    pub contents: String,
    pub identity: ConfigFileIdentity,
}

pub(crate) fn replace_config_if_unchanged(
    expected: &ConfigFileIdentity,
    contents: &str,
) -> std::io::Result<ConfigFileIdentity> {
    if contents.len() as u64 > MAX_CONFIG_BYTES {
        return Err(std::io::Error::other(format!(
            "config file exceeds {MAX_CONFIG_BYTES} bytes"
        )));
    }
    if read_existing_config(&expected.canonical_path)?.identity != *expected {
        return Err(std::io::Error::other(
            "config file changed since the published generation",
        ));
    }
    let parent = expected
        .canonical_path
        .parent()
        .ok_or_else(|| std::io::Error::other("config path has no parent"))?;
    let filename = expected
        .canonical_path
        .file_name()
        .ok_or_else(|| std::io::Error::other("config path has no filename"))?;
    let temporary_path = parent.join(format!(
        ".{}.momento-update.tmp",
        filename.to_string_lossy()
    ));
    let mut temporary_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary_path)?;
    let result = (|| {
        use std::io::Write;
        temporary_file.write_all(contents.as_bytes())?;
        temporary_file.sync_all()?;
        if read_existing_config(&expected.canonical_path)?.identity != *expected {
            return Err(std::io::Error::other(
                "config file changed before atomic publication",
            ));
        }
        std::fs::rename(&temporary_path, &expected.canonical_path)?;
        File::open(parent)?.sync_all()
    })();
    drop(temporary_file);
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error);
    }
    Ok(read_existing_config(&expected.canonical_path)?.identity)
}

pub(crate) fn recover_config_update_temporary(
    expected: &ConfigFileIdentity,
) -> std::io::Result<()> {
    let parent = expected
        .canonical_path
        .parent()
        .ok_or_else(|| std::io::Error::other("config path has no parent"))?;
    let filename = expected
        .canonical_path
        .file_name()
        .ok_or_else(|| std::io::Error::other("config path has no filename"))?;
    let temporary_path = parent.join(format!(
        ".{}.momento-update.tmp",
        filename.to_string_lossy()
    ));
    let metadata = match std::fs::symlink_metadata(&temporary_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES
    {
        return Err(std::io::Error::other(
            "reserved config update temporary is unsafe",
        ));
    }
    std::fs::remove_file(temporary_path)?;
    File::open(parent)?.sync_all()
}

pub fn read_existing_config(path: &Path) -> std::io::Result<BoundedConfigFile> {
    let canonical_path = validate_config_path(path, true)?;
    let mut file = File::open(&canonical_path)?;
    let before = file.metadata()?;
    validate_regular_config_file(&canonical_path, &before)?;

    let read_limit = MAX_CONFIG_BYTES
        .checked_add(1)
        .ok_or_else(|| std::io::Error::other("config read limit overflow"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(
            usize::try_from(read_limit).map_err(|_| {
                std::io::Error::other("config read limit does not fit this platform")
            })?,
        )
        .map_err(|error| {
            std::io::Error::other(format!("config buffer allocation failed: {error}"))
        })?;
    file.by_ref().take(read_limit).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(std::io::Error::other(format!(
            "config file exceeds {MAX_CONFIG_BYTES} bytes"
        )));
    }

    let after = file.metadata()?;
    if file_identity_fields(&before) != file_identity_fields(&after) {
        return Err(std::io::Error::other(
            "config file changed while it was being read",
        ));
    }
    let contents = String::from_utf8(bytes)
        .map_err(|_| std::io::Error::other("config file must contain valid UTF-8"))?;
    let sha256 = Sha256::digest(contents.as_bytes()).into();
    Ok(BoundedConfigFile {
        contents,
        identity: ConfigFileIdentity {
            canonical_path,
            device: after.dev(),
            inode: after.ino(),
            byte_size: after.len(),
            modified_seconds: after.mtime(),
            modified_nanoseconds: after.mtime_nsec(),
            sha256,
        },
    })
}

pub fn validate_config_path(path: &Path, require_file: bool) -> std::io::Result<PathBuf> {
    let path_bytes = path.as_os_str().as_bytes();
    if path_bytes.is_empty() {
        return Err(std::io::Error::other("config path must not be empty"));
    }
    if path_bytes.len() > MAX_CONFIG_PATH_BYTES {
        return Err(std::io::Error::other(format!(
            "config path exceeds {MAX_CONFIG_PATH_BYTES} bytes"
        )));
    }
    if path_bytes.contains(&0) {
        return Err(std::io::Error::other("config path contains NUL"));
    }
    if path_bytes.windows(2).any(|pair| pair == b"//") {
        return Err(std::io::Error::other(
            "config path contains an empty component",
        ));
    }

    let mut component_count = 0usize;
    let mut final_component_length = None;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(component) => {
                let length = component.as_bytes().len();
                if length == 0 || length > MAX_CONFIG_COMPONENT_BYTES {
                    return Err(std::io::Error::other(format!(
                        "config path component must contain 1..={MAX_CONFIG_COMPONENT_BYTES} bytes"
                    )));
                }
                component_count = component_count
                    .checked_add(1)
                    .ok_or_else(|| std::io::Error::other("config component count overflow"))?;
                final_component_length = Some(length);
            }
            Component::CurDir => {
                return Err(std::io::Error::other(
                    "config path must not contain a dot component",
                ));
            }
            Component::ParentDir => {
                return Err(std::io::Error::other(
                    "config path must not contain a parent component",
                ));
            }
            Component::Prefix(_) => {
                return Err(std::io::Error::other(
                    "config path contains an unsupported prefix",
                ));
            }
        }
    }
    if component_count == 0 || component_count > MAX_CONFIG_PATH_COMPONENTS {
        return Err(std::io::Error::other(format!(
            "config path must contain 1..={MAX_CONFIG_PATH_COMPONENTS} components"
        )));
    }
    if final_component_length.is_some_and(|length| length > MAX_CONFIG_FILENAME_BYTES) {
        return Err(std::io::Error::other(format!(
            "config filename exceeds {MAX_CONFIG_FILENAME_BYTES} bytes"
        )));
    }

    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    validate_no_symlinks(&absolute_path, require_file)?;
    Ok(absolute_path)
}

fn validate_no_symlinks(path: &Path, require_file: bool) -> std::io::Result<()> {
    let components = path.components().collect::<Vec<_>>();
    let mut current = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        if matches!(component, Component::RootDir) {
            continue;
        }
        let is_final = index + 1 == components.len();
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(std::io::Error::other(format!(
                        "config path must not contain a symlink: {}",
                        current.display()
                    )));
                }
                if !is_final && !metadata.is_dir() {
                    return Err(std::io::Error::other(format!(
                        "config path parent is not a directory: {}",
                        current.display()
                    )));
                }
            }
            Err(error)
                if is_final && !require_file && error.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn validate_regular_config_file(path: &Path, metadata: &std::fs::Metadata) -> std::io::Result<()> {
    if !metadata.is_file() {
        return Err(std::io::Error::other(format!(
            "config path is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(std::io::Error::other(format!(
            "config file exceeds {MAX_CONFIG_BYTES} bytes"
        )));
    }
    Ok(())
}

fn file_identity_fields(metadata: &std::fs::Metadata) -> (u64, u64, u64, i64, i64) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
    )
}
