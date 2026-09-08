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

/// Probe only the extensionless queue file, never the user-supplied name or MIME.
/// TIFF-based RAW and ISO-BMFF images require container inspection beyond magic bytes.
pub async fn detect_input_mime(path: &Path, header: &[u8]) -> Result<String, String> {
    if let Some(mime) = encoded_image_mime_type(header) {
        return Ok(mime.to_string());
    }
    let mut child = tokio::process::Command::new("exiftool")
        .args(["-j", "-FileType", "-MIMEType"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("content format probe could not start: {error}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or("format probe stdout unavailable")?
        .take(65537);
    let mut stderr = child
        .stderr
        .take()
        .ok_or("format probe stderr unavailable")?
        .take(65537);
    let mut output = Vec::new();
    let mut diagnostic = Vec::new();
    let (_, _, status) = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::try_join!(
            stdout.read_to_end(&mut output),
            stderr.read_to_end(&mut diagnostic),
            child.wait()
        )
    })
    .await
    .map_err(|_| "content format probe timed out".to_string())?
    .map_err(|error| format!("content format probe failed: {error}"))?;
    if !status.success() || output.len() > 65536 || diagnostic.len() > 65536 {
        return Err(format!(
            "content format probe failed ({status}): {}",
            String::from_utf8_lossy(&diagnostic)
        ));
    }
    let records: Vec<serde_json::Value> = serde_json::from_slice(&output)
        .map_err(|error| format!("invalid content format probe response: {error}"))?;
    let record = records
        .first()
        .ok_or("content format probe returned no record")?;
    let file_type = record["FileType"].as_str().unwrap_or("");
    let mime = match file_type {
        "DNG" => "image/x-adobe-dng",
        "CR2" | "CR3" | "CRW" | "NEF" | "NRW" | "ARW" | "SR2" | "SRF" | "RW2" | "RWL" | "ORF"
        | "RAF" | "PEF" | "SRW" | "RAW" | "ERF" | "MRW" | "MOS" | "KDC" | "DCR" | "3FR" | "FFF"
        | "IIQ" => "image/x-raw",
        _ => record["MIMEType"].as_str().unwrap_or(""),
    };
    if !mime.starts_with("image/") {
        return Err(format!("unsupported or unrecognized input content: file_type={file_type:?}, detected_mime={mime:?}"));
    }
    Ok(mime.to_string())
}

/// Recognize non-RAW encodings without trusting a camera filename or MIME label.
/// TIFF is deliberately absent: many genuine RAW formats use a TIFF container.
pub fn encoded_image_mime_type(header: &[u8]) -> Option<&'static str> {
    if header.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if header.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if header.starts_with(b"RIFF") && header.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    None
}

pub const RAW_NORMALIZATION_ARGUMENTS: [&str; 9] =
    ["-dngsdk", "-w", "+M", "-o", "1", "-q", "3", "-T", "-Z"];

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
        .args(RAW_NORMALIZATION_ARGUMENTS)
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
