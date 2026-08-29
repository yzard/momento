use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;

use thiserror::Error;

use crate::config::MediaProcessConfig;

const MAXIMUM_PERSISTED_PROCESS_DIAGNOSTIC_CHARACTERS: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub struct ExternalProcess {
    executable: OsString,
    arguments: Vec<OsString>,
    maximum_stdout_bytes: usize,
    maximum_stderr_bytes: usize,
}

impl ExternalProcess {
    pub fn new(
        executable: impl Into<OsString>,
        arguments: Vec<OsString>,
        maximum_stdout_bytes: usize,
        maximum_stderr_bytes: usize,
    ) -> Self {
        Self {
            executable: executable.into(),
            arguments,
            maximum_stdout_bytes,
            maximum_stderr_bytes,
        }
    }

    pub async fn run(self) -> Result<ExternalProcessOutput, ExternalProcessError> {
        tokio::task::spawn_blocking(move || self.run_blocking())
            .await
            .map_err(|error| ExternalProcessError::Join(error.to_string()))?
    }

    pub fn run_blocking(self) -> Result<ExternalProcessOutput, ExternalProcessError> {
        let mut command = Command::new(&self.executable);
        command
            .args(&self.arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_process_group(&mut command);

        let mut child = command
            .spawn()
            .map_err(|source| ExternalProcessError::Start {
                executable: self.executable.to_string_lossy().into_owned(),
                source,
            })?;
        let process_id = child.id();
        let stdout_reader = child
            .stdout
            .take()
            .ok_or(ExternalProcessError::MissingOutputPipe("stdout"))?;
        let stderr_reader = child
            .stderr
            .take()
            .ok_or(ExternalProcessError::MissingOutputPipe("stderr"))?;
        let stdout_thread = capture_output(stdout_reader, self.maximum_stdout_bytes);
        let stderr_thread = capture_output(stderr_reader, self.maximum_stderr_bytes);

        let status = match child.wait() {
            Ok(status) => status,
            Err(error) => {
                kill_process_group(&mut child, process_id);
                let _ = child.wait();
                return Err(ExternalProcessError::Wait(error));
            }
        };
        let stdout = join_output(stdout_thread, "stdout")?;
        let stderr = join_output(stderr_thread, "stderr")?;

        Ok(ExternalProcessOutput {
            status,
            stdout: stdout.bytes,
            stderr: stderr.bytes,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        })
    }
}

#[derive(Debug)]
pub struct ExternalProcessOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl ExternalProcessOutput {
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }

    pub fn failure_detail(&self, executable: &str) -> String {
        let stderr = self.stderr_text();
        let (stream_name, diagnostic) = if !stderr.trim().is_empty() {
            ("stderr", bounded_error_detail(stderr.trim()))
        } else {
            let stdout = String::from_utf8_lossy(&self.stdout);
            if stdout.trim().is_empty() {
                ("stderr", "<empty>".to_string())
            } else {
                ("stdout", bounded_error_detail(stdout.trim()))
            }
        };
        let capture_truncated = if stream_name == "stderr" {
            self.stderr_truncated
        } else {
            self.stdout_truncated
        };
        let capture_suffix = if capture_truncated {
            format!(" ({stream_name} capture truncated)")
        } else {
            String::new()
        };
        format!(
            "{executable} exited with {}; {stream_name}: {diagnostic}{capture_suffix}",
            self.status
        )
    }
}

pub fn bounded_error_detail(detail: &str) -> String {
    let mut characters = detail.chars();
    let bounded = characters
        .by_ref()
        .take(MAXIMUM_PERSISTED_PROCESS_DIAGNOSTIC_CHARACTERS)
        .collect::<String>();
    if characters.next().is_some() {
        format!("{bounded} (diagnostic truncated)")
    } else {
        bounded
    }
}

#[derive(Debug, Error)]
pub enum ExternalProcessError {
    #[error("failed to start {executable}: {source}")]
    Start {
        executable: String,
        #[source]
        source: io::Error,
    },
    #[error("failed while waiting for external process: {0}")]
    Wait(#[source] io::Error),
    #[error("external process did not expose its {0} pipe")]
    MissingOutputPipe(&'static str),
    #[error("failed to read external process {stream}: {source}")]
    Read {
        stream: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("external process {0} reader thread panicked")]
    ReaderPanic(&'static str),
    #[error("external process blocking task failed: {0}")]
    Join(String),
}

#[derive(Debug, Error)]
pub enum ImageDimensionError {
    #[error(transparent)]
    Process(#[from] ExternalProcessError),
    #[error("ImageMagick could not inspect image dimensions: {0}")]
    Inspection(String),
    #[error("ImageMagick dimension output exceeded its limit")]
    OutputTooLarge,
    #[error("ImageMagick returned invalid image dimensions")]
    InvalidOutput,
    #[error(
        "decoded image contains {actual_pixels} pixels, exceeding the {maximum_pixels} pixel limit"
    )]
    PixelLimitExceeded {
        actual_pixels: u64,
        maximum_pixels: u64,
    },
}

pub fn image_magick_resource_arguments(config: &MediaProcessConfig) -> Vec<OsString> {
    vec![
        "-limit".into(),
        "memory".into(),
        format!("{}MiB", config.imagemagick_memory_limit_mebibytes).into(),
        "-limit".into(),
        "map".into(),
        format!("{}MiB", config.imagemagick_map_limit_mebibytes).into(),
        "-limit".into(),
        "disk".into(),
        format!("{}MiB", config.imagemagick_disk_limit_mebibytes).into(),
        "-limit".into(),
        "area".into(),
        format!("{}P", config.maximum_decoded_image_pixels).into(),
        "-limit".into(),
        "thread".into(),
        config.imagemagick_maximum_threads.to_string().into(),
    ]
}

pub async fn validate_image_dimensions(
    image_path: &std::path::Path,
    config: &MediaProcessConfig,
) -> Result<(), ImageDimensionError> {
    let image_path = image_frame_path(image_path);
    let mut arguments = image_magick_resource_arguments(config);
    arguments.extend([
        OsString::from("-ping"),
        OsString::from("-format"),
        OsString::from("%w %h"),
        image_path,
    ]);
    validate_dimension_output(
        ExternalProcess::new("identify", arguments, 128, config.maximum_stderr_bytes)
            .run()
            .await?,
        config.maximum_decoded_image_pixels,
    )
}

pub fn validate_image_dimensions_blocking(
    image_path: &std::path::Path,
    config: &MediaProcessConfig,
) -> Result<(), ImageDimensionError> {
    let image_path = image_frame_path(image_path);
    let mut arguments = image_magick_resource_arguments(config);
    arguments.extend([
        OsString::from("-ping"),
        OsString::from("-format"),
        OsString::from("%w %h"),
        image_path,
    ]);
    validate_dimension_output(
        ExternalProcess::new("identify", arguments, 128, config.maximum_stderr_bytes)
            .run_blocking()?,
        config.maximum_decoded_image_pixels,
    )
}

fn validate_dimension_output(
    output: ExternalProcessOutput,
    maximum_pixels: u64,
) -> Result<(), ImageDimensionError> {
    if !output.status.success() {
        return Err(ImageDimensionError::Inspection(
            output.failure_detail("identify"),
        ));
    }
    if output.stdout_truncated {
        return Err(ImageDimensionError::OutputTooLarge);
    }
    let (width, height) = std::str::from_utf8(&output.stdout)
        .ok()
        .and_then(|output| {
            let mut fields = output.split_whitespace();
            let width = fields.next()?.parse::<u64>().ok()?;
            let height = fields.next()?.parse::<u64>().ok()?;
            (fields.next().is_none() && width > 0 && height > 0).then_some((width, height))
        })
        .ok_or(ImageDimensionError::InvalidOutput)?;
    let actual_pixels =
        width
            .checked_mul(height)
            .ok_or(ImageDimensionError::PixelLimitExceeded {
                actual_pixels: u64::MAX,
                maximum_pixels,
            })?;
    if actual_pixels > maximum_pixels {
        return Err(ImageDimensionError::PixelLimitExceeded {
            actual_pixels,
            maximum_pixels,
        });
    }
    Ok(())
}

fn image_frame_path(image_path: &std::path::Path) -> OsString {
    let mut image_frame_path = image_path.as_os_str().to_os_string();
    image_frame_path.push("[0]");
    image_frame_path
}

pub fn os_arguments(arguments: &[&OsStr]) -> Vec<OsString> {
    arguments
        .iter()
        .map(|argument| (*argument).to_os_string())
        .collect()
}

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn capture_output(
    mut output: impl Read + Send + 'static,
    maximum_bytes: usize,
) -> JoinHandle<io::Result<CapturedOutput>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::with_capacity(maximum_bytes.min(64 * 1024));
        let mut buffer = [0_u8; 16 * 1024];
        let mut truncated = false;
        loop {
            let read_length = output.read(&mut buffer)?;
            if read_length == 0 {
                break;
            }
            let remaining = maximum_bytes.saturating_sub(bytes.len());
            let retained_length = remaining.min(read_length);
            bytes.extend_from_slice(&buffer[..retained_length]);
            truncated |= retained_length < read_length;
        }
        Ok(CapturedOutput { bytes, truncated })
    })
}

fn join_output(
    output_thread: JoinHandle<io::Result<CapturedOutput>>,
    stream: &'static str,
) -> Result<CapturedOutput, ExternalProcessError> {
    output_thread
        .join()
        .map_err(|_| ExternalProcessError::ReaderPanic(stream))?
        .map_err(|source| ExternalProcessError::Read { stream, source })
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn kill_process_group(_child: &mut Child, process_id: u32) {
    signal_process_group(process_id, libc::SIGKILL);
}

#[cfg(not(unix))]
fn kill_process_group(child: &mut Child, _process_id: u32) {
    let _ = child.kill();
}

#[cfg(unix)]
fn signal_process_group(process_id: u32, signal: libc::c_int) {
    if let Ok(process_id) = i32::try_from(process_id) {
        // SAFETY: kill receives a valid negative process-group ID and a fixed signal constant.
        unsafe {
            libc::kill(-process_id, signal);
        }
    }
}
