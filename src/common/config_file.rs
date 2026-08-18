use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMPORARY_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn config_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn write_temporary_config(path: &Path, contents: &str) -> io::Result<std::path::PathBuf> {
    let parent = config_parent(path);
    fs::create_dir_all(parent)?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "config path has no filename")
    })?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let counter = TEMPORARY_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary_path = parent.join(format!(
        ".{}.{}.{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id(),
        timestamp,
        counter
    ));
    let mut temporary_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary_path)?;
    let write_result = (|| -> io::Result<()> {
        temporary_file.write_all(contents.as_bytes())?;
        temporary_file.sync_all()
    })();
    drop(temporary_file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    Ok(temporary_path)
}

pub fn write_new_config(path: &Path, contents: &str) -> io::Result<()> {
    let temporary_path = write_temporary_config(path, contents)?;
    let write_result = fs::hard_link(&temporary_path, path);
    let cleanup_result = fs::remove_file(&temporary_path);

    write_result?;
    cleanup_result
}

pub fn replace_config(path: &Path, contents: &str) -> io::Result<()> {
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("config file not found: {}", path.display()),
        ));
    }
    let temporary_path = write_temporary_config(path, contents)?;
    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    fs::File::open(config_parent(path))?.sync_all()
}
