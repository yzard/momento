use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

const NORMALIZATION_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MAXIMUM_STDERR_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NormalizedInputDescriptor {
    pub byte_size: u64,
    pub content_hash: String,
}

pub fn requires_raw_normalization(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "image/x-adobe-dng"
            | "image/x-canon-cr2"
            | "image/x-canon-cr3"
            | "image/x-nikon-nef"
            | "image/x-sony-arw"
            | "image/x-panasonic-rw2"
            | "image/x-olympus-orf"
            | "image/x-fuji-raf"
            | "image/x-pentax-pef"
            | "image/x-samsung-srw"
            | "image/x-raw"
    )
}

pub async fn ensure_raw_normalized(
    source_path: &Path,
    normalized_path: &Path,
    job_id: &str,
    sequence: u32,
) -> Result<NormalizedInputDescriptor, String> {
    let descriptor_path = normalized_path.with_extension("json");
    if let Some(descriptor) = read_published_descriptor(normalized_path, &descriptor_path).await? {
        return Ok(descriptor);
    }

    remove_if_present(normalized_path).await?;
    remove_if_present(&descriptor_path).await?;
    let temporary_path =
        normalized_path.with_file_name(format!(".normalized-{job_id}-{sequence}.tiff"));
    remove_if_present(&temporary_path).await?;

    let mut child = tokio::process::Command::new("dcraw_emu")
        .arg("-w")
        .arg("+M")
        .arg("-o")
        .arg("1")
        .arg("-q")
        .arg("3")
        .arg("-T")
        .arg("-Z")
        .arg(&temporary_path)
        .arg(source_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("could not start RAW normalization: {error}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "RAW normalization stderr pipe is unavailable".to_string())?;
    let completion = tokio::time::timeout(NORMALIZATION_TIMEOUT, async move {
        let mut bounded_stderr = Vec::new();
        let mut bounded_reader = stderr.take(MAXIMUM_STDERR_BYTES + 1);
        let (_, status) = tokio::try_join!(
            bounded_reader.read_to_end(&mut bounded_stderr),
            child.wait()
        )?;
        Ok::<_, std::io::Error>((status, bounded_stderr))
    })
    .await
    .map_err(|_| "RAW normalization exceeded 30 minutes".to_string())?
    .map_err(|error| format!("RAW normalization process failed: {error}"))?;
    let (status, stderr) = completion;
    if !status.success() {
        remove_if_present(&temporary_path).await?;
        return Err(format!(
            "RAW normalization failed with {status}: {}",
            bounded_stderr_text(&stderr)
        ));
    }

    let normalized = inspect_and_hash(&temporary_path).await?;
    let normalized_file = tokio::fs::File::open(&temporary_path)
        .await
        .map_err(|error| format!("could not open normalized RAW output: {error}"))?;
    normalized_file
        .sync_all()
        .await
        .map_err(|error| format!("could not sync normalized RAW output: {error}"))?;
    tokio::fs::rename(&temporary_path, normalized_path)
        .await
        .map_err(|error| format!("could not publish normalized RAW output: {error}"))?;
    sync_directory(
        normalized_path
            .parent()
            .ok_or_else(|| "normalized RAW output has no parent directory".to_string())?,
    )?;

    let descriptor_bytes = serde_json::to_vec(&normalized)
        .map_err(|error| format!("could not encode normalized RAW descriptor: {error}"))?;
    let descriptor_temporary_path = descriptor_path.with_extension("json.tmp");
    tokio::fs::write(&descriptor_temporary_path, descriptor_bytes)
        .await
        .map_err(|error| format!("could not write normalized RAW descriptor: {error}"))?;
    let descriptor_file = tokio::fs::File::open(&descriptor_temporary_path)
        .await
        .map_err(|error| format!("could not open normalized RAW descriptor: {error}"))?;
    descriptor_file
        .sync_all()
        .await
        .map_err(|error| format!("could not sync normalized RAW descriptor: {error}"))?;
    tokio::fs::rename(&descriptor_temporary_path, &descriptor_path)
        .await
        .map_err(|error| format!("could not publish normalized RAW descriptor: {error}"))?;
    sync_directory(
        descriptor_path
            .parent()
            .ok_or_else(|| "normalized RAW descriptor has no parent directory".to_string())?,
    )?;
    Ok(normalized)
}

async fn read_published_descriptor(
    normalized_path: &Path,
    descriptor_path: &Path,
) -> Result<Option<NormalizedInputDescriptor>, String> {
    let descriptor_bytes = match tokio::fs::read(descriptor_path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read normalized RAW descriptor: {error}")),
    };
    let descriptor = serde_json::from_slice::<NormalizedInputDescriptor>(&descriptor_bytes)
        .map_err(|error| format!("normalized RAW descriptor is invalid: {error}"))?;
    if descriptor.byte_size == 0
        || descriptor.content_hash.len() != 64
        || !descriptor
            .content_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("normalized RAW descriptor fields are invalid".to_string());
    }
    let metadata = tokio::fs::symlink_metadata(normalized_path)
        .await
        .map_err(|error| format!("normalized RAW output is unavailable: {error}"))?;
    if !metadata.file_type().is_file() || metadata.len() != descriptor.byte_size {
        return Err("normalized RAW output does not match its descriptor".to_string());
    }
    Ok(Some(descriptor))
}

async fn inspect_and_hash(path: &Path) -> Result<NormalizedInputDescriptor, String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("could not open normalized RAW output: {error}"))?;
    let metadata = file
        .metadata()
        .await
        .map_err(|error| format!("could not inspect normalized RAW output: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err("RAW normalization produced no image".to_string());
    }
    let mut hasher = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let bytes_read = file
            .read(&mut chunk)
            .await
            .map_err(|error| format!("could not hash normalized RAW output: {error}"))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&chunk[..bytes_read]);
    }
    Ok(NormalizedInputDescriptor {
        byte_size: metadata.len(),
        content_hash: format!("{:x}", hasher.finalize()),
    })
}

async fn remove_if_present(path: &Path) -> Result<(), String> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "could not remove stale RAW normalization file: {error}"
        )),
    }
}

fn bounded_stderr_text(stderr: &[u8]) -> String {
    let bounded = if stderr.len() > MAXIMUM_STDERR_BYTES as usize {
        &stderr[..MAXIMUM_STDERR_BYTES as usize]
    } else {
        stderr
    };
    String::from_utf8_lossy(bounded).trim().to_string()
}

fn sync_directory(path: &Path) -> Result<(), String> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("could not sync RAW normalization directory: {error}"))
}

pub fn runtime_input_path(job_path: &Path, sequence: u32, normalized: bool) -> PathBuf {
    if normalized {
        job_path.join(format!("normalized-input-{sequence}.tiff"))
    } else {
        job_path.join(format!("input-{sequence}"))
    }
}
