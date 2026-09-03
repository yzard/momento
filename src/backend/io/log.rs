use std::collections::BinaryHeap;
use std::ffi::CString;
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use std::os::unix::fs::MetadataExt;

use crossbeam_channel::{Receiver, Sender, TrySendError};

use crate::io::session::rename_descriptor_entry;
use crate::io::space_budget::{DataDirSpaceBudget, SpaceAdmission, MAX_SPACE_RECONSTRUCTION_PAGE};

pub const MAX_LOG_EVENT_BYTES: usize = 64 * 1024;
pub const MAX_LOG_ROTATION_BYTES: u64 = 128 * 1024 * 1024;
const MAX_LOG_DRAIN_BATCH: usize = 64;
const MAX_LOG_DRAIN_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSeverity {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogSeverity {
    const fn index(self) -> usize {
        match self {
            Self::Debug => 0,
            Self::Info => 1,
            Self::Warn => 2,
            Self::Error => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DroppedLogEvents {
    pub debug: u64,
    pub info: u64,
    pub warn: u64,
    pub error: u64,
}

#[derive(Debug)]
pub(crate) struct LogEvent {
    severity: LogSeverity,
    bytes: Vec<u8>,
}

struct DroppedCounters {
    values: [AtomicU64; 4],
    last_health_diagnostic_second: AtomicU64,
}

impl DroppedCounters {
    fn new() -> Self {
        Self {
            values: std::array::from_fn(|_| AtomicU64::new(0)),
            last_health_diagnostic_second: AtomicU64::new(0),
        }
    }

    fn increment(&self, severity: LogSeverity) {
        self.values[severity.index()].fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> DroppedLogEvents {
        DroppedLogEvents {
            debug: self.values[0].load(Ordering::Relaxed),
            info: self.values[1].load(Ordering::Relaxed),
            warn: self.values[2].load(Ordering::Relaxed),
            error: self.values[3].load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone)]
pub struct LogEventProducer {
    sender: Sender<LogEvent>,
    dropped: Arc<DroppedCounters>,
}

pub(crate) struct LogEventConsumer {
    receiver: Receiver<LogEvent>,
    dropped: Arc<DroppedCounters>,
}

impl LogEventConsumer {
    pub(crate) fn receiver(&self) -> &Receiver<LogEvent> {
        &self.receiver
    }

    pub(crate) fn record_drop(&self, severity: LogSeverity) {
        self.dropped.increment(severity);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        let previous = self
            .dropped
            .last_health_diagnostic_second
            .load(Ordering::Relaxed);
        if now.saturating_sub(previous) < 60 {
            return;
        }
        if self
            .dropped
            .last_health_diagnostic_second
            .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            let dropped = self.dropped.snapshot();
            eprintln!(
                "Momento file log sink unavailable; dropped events: debug={}, info={}, warn={}, error={}",
                dropped.debug, dropped.info, dropped.warn, dropped.error
            );
        }
    }
}

pub(crate) fn bounded_log_ring(
    capacity: usize,
) -> std::io::Result<(LogEventProducer, LogEventConsumer)> {
    if capacity == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "log event capacity must be positive",
        ));
    }
    let (sender, receiver) = crossbeam_channel::bounded(capacity);
    let dropped = Arc::new(DroppedCounters::new());
    Ok((
        LogEventProducer {
            sender,
            dropped: Arc::clone(&dropped),
        },
        LogEventConsumer { receiver, dropped },
    ))
}

impl LogEventProducer {
    pub fn try_emit(&self, severity: LogSeverity, bytes: Vec<u8>) {
        if bytes.is_empty() || bytes.len() > MAX_LOG_EVENT_BYTES {
            self.dropped.increment(severity);
            return;
        }
        if let Err(error) = self.sender.try_send(LogEvent { severity, bytes }) {
            match error {
                TrySendError::Full(event) | TrySendError::Disconnected(event) => {
                    self.dropped.increment(event.severity);
                }
            }
        }
    }

    pub fn dropped_events(&self) -> DroppedLogEvents {
        self.dropped.snapshot()
    }
}

pub(crate) struct RuntimeLogWriter {
    file: Option<File>,
    date: Option<chrono::NaiveDate>,
    logical_bytes: u64,
    reservation_sequence: u64,
    pending_event: Option<LogEvent>,
}

impl RuntimeLogWriter {
    pub(crate) fn new() -> Self {
        Self {
            file: None,
            date: None,
            logical_bytes: 0,
            reservation_sequence: 0,
            pending_event: None,
        }
    }

    pub(crate) fn drain(
        &mut self,
        consumer: &LogEventConsumer,
        logs: &File,
        budget: &DataDirSpaceBudget,
    ) {
        self.drain_batch(consumer, logs, budget, None);
    }

    pub(crate) fn drain_all(
        &mut self,
        consumer: &LogEventConsumer,
        logs: &File,
        budget: &DataDirSpaceBudget,
    ) {
        while self.pending_event.is_some() || !consumer.receiver.is_empty() {
            self.drain(consumer, logs, budget);
        }
    }

    pub(crate) fn append_received(
        &mut self,
        consumer: &LogEventConsumer,
        logs: &File,
        budget: &DataDirSpaceBudget,
        event: LogEvent,
    ) {
        self.drain_batch(consumer, logs, budget, Some(event));
    }

    fn drain_batch(
        &mut self,
        consumer: &LogEventConsumer,
        logs: &File,
        budget: &DataDirSpaceBudget,
        mut received_event: Option<LogEvent>,
    ) {
        let mut event_count = 0_usize;
        let mut encoded_bytes = 0_usize;
        while event_count < MAX_LOG_DRAIN_BATCH {
            let event = if let Some(event) = self.pending_event.take() {
                event
            } else if let Some(event) = received_event.take() {
                event
            } else {
                let Ok(event) = consumer.receiver.try_recv() else {
                    break;
                };
                event
            };
            let next_bytes = encoded_bytes.saturating_add(event.bytes.len());
            if event_count > 0 && next_bytes > MAX_LOG_DRAIN_BYTES {
                self.pending_event = Some(event);
                break;
            }
            encoded_bytes = next_bytes;
            event_count += 1;
            if self.append(logs, budget, &event.bytes).is_err() {
                consumer.record_drop(event.severity);
            }
        }
    }

    pub(crate) fn flush(&mut self) -> std::io::Result<()> {
        if let Some(file) = &self.file {
            file.sync_data()?;
        }
        Ok(())
    }

    fn append(
        &mut self,
        logs: &File,
        budget: &DataDirSpaceBudget,
        bytes: &[u8],
    ) -> std::io::Result<()> {
        let date = chrono::Utc::now().date_naive();
        self.reservation_sequence = self
            .reservation_sequence
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("log reservation sequence overflow"))?;
        let peak = (bytes.len() as u64)
            .checked_add(
                budget
                    .filesystem_entry_metadata_bytes()
                    .map_err(std::io::Error::other)?,
            )
            .ok_or_else(|| std::io::Error::other("log reservation size overflow"))?;
        let reservation_id = format!("log-append-{}", self.reservation_sequence);
        let token = self.reserve_with_pruning(logs, budget, reservation_id, peak, date)?;
        let baseline = budget
            .snapshot()
            .map_err(std::io::Error::other)?
            .log_allocated_bytes;
        self.prepare_file(logs, date, bytes.len() as u64)?;
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("log file was not opened"))?;
        let before = allocated_file_bytes(file)?;
        let write_result = file.write_all(bytes).and_then(|()| file.sync_data());
        let after = allocated_file_bytes(file)?;
        let allocated = baseline
            .checked_add(after.saturating_sub(before))
            .ok_or_else(|| std::io::Error::other("log allocation overflow"))?;
        token
            .publish_ephemeral_log_allocation(allocated)
            .map_err(std::io::Error::other)?;
        write_result?;
        self.logical_bytes = self
            .logical_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| std::io::Error::other("log logical size overflow"))?;
        Ok(())
    }

    fn reserve_with_pruning(
        &mut self,
        logs: &File,
        budget: &DataDirSpaceBudget,
        reservation_id: String,
        peak: u64,
        date: chrono::NaiveDate,
    ) -> std::io::Result<crate::io::space_budget::ProvisionalSpaceToken> {
        match budget
            .reserve_log(reservation_id.clone(), peak)
            .map_err(std::io::Error::other)?
        {
            SpaceAdmission::Fits(token) => return Ok(token),
            SpaceAdmission::ExceedsHardLimit { .. } => {
                return Err(std::io::Error::other(
                    "one log event exceeds the Log-class hard limit",
                ));
            }
            SpaceAdmission::TemporarilyUnavailable { .. } => {}
        }

        self.flush()?;
        self.file.take();
        let excluded = current_log_filename(date);
        loop {
            let snapshot = budget.snapshot().map_err(std::io::Error::other)?;
            let target = snapshot.log_quota_bytes.saturating_sub(peak);
            if !prune_oldest_closed_rotations_batch_excluding(
                logs,
                snapshot.log_allocated_bytes,
                target,
                Some(&excluded),
            )? {
                return Err(std::io::Error::other("log capacity is unavailable"));
            }
            let allocated = measure_retained_log_allocation(logs)?;
            budget
                .publish_runtime_log_cleanup_allocation(allocated)
                .map_err(std::io::Error::other)?;
            match budget
                .reserve_log(reservation_id.clone(), peak)
                .map_err(std::io::Error::other)?
            {
                SpaceAdmission::Fits(token) => return Ok(token),
                SpaceAdmission::ExceedsHardLimit { .. } => {
                    return Err(std::io::Error::other(
                        "one log event exceeds the Log-class hard limit",
                    ));
                }
                SpaceAdmission::TemporarilyUnavailable { .. } => {}
            }
        }
    }

    fn prepare_file(
        &mut self,
        logs: &File,
        date: chrono::NaiveDate,
        next_bytes: u64,
    ) -> std::io::Result<()> {
        let date_changed = self.date.is_some_and(|current| current != date);
        let size_rotation = self.file.is_some()
            && self
                .logical_bytes
                .checked_add(next_bytes)
                .is_none_or(|total| total > MAX_LOG_ROTATION_BYTES);
        if date_changed || size_rotation {
            self.flush()?;
            self.file.take();
            if size_rotation {
                rotate_current_file(logs, date)?;
            }
            self.logical_bytes = 0;
        }
        if self.file.is_none() {
            let filename = current_log_filename(date);
            let file = open_log_file(logs, &filename)?;
            self.logical_bytes = file.metadata()?.len();
            self.file = Some(file);
            self.date = Some(date);
        }
        Ok(())
    }
}

fn current_log_filename(date: chrono::NaiveDate) -> String {
    format!("momento-api.{date}.log")
}

fn open_log_file(directory: &File, filename: &str) -> std::io::Result<File> {
    let filename = CString::new(filename)
        .map_err(|_| std::io::Error::other("log filename contains a null byte"))?;
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            filename.as_ptr(),
            libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o640,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn rotate_current_file(directory: &File, date: chrono::NaiveDate) -> std::io::Result<()> {
    let source = CString::new(current_log_filename(date))
        .map_err(|_| std::io::Error::other("log filename contains a null byte"))?;
    for sequence in 1..=MAX_SPACE_RECONSTRUCTION_PAGE {
        let destination = CString::new(format!("momento-api.{date}.{sequence:03}.log"))
            .map_err(|_| std::io::Error::other("log filename contains a null byte"))?;
        match rename_descriptor_entry(
            directory.as_raw_fd(),
            &source,
            directory.as_raw_fd(),
            &destination,
            libc::RENAME_NOREPLACE,
        ) {
            Ok(()) => {
                directory.sync_all()?;
                return Ok(());
            }
            Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::other(
        "daily log rotation sequence is exhausted",
    ))
}

fn allocated_file_bytes(file: &File) -> std::io::Result<u64> {
    file.metadata()?
        .blocks()
        .checked_mul(512)
        .ok_or_else(|| std::io::Error::other("log allocation overflow"))
}

pub fn measure_retained_log_allocation(directory: &File) -> std::io::Result<u64> {
    let directory_path = format!("/proc/self/fd/{}", directory.as_raw_fd());
    let mut allocated = 0_u64;
    let mut entries_in_continuation = 0_usize;
    for entry in std::fs::read_dir(directory_path)? {
        entries_in_continuation = entries_in_continuation
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("log entry count overflow"))?;
        if entries_in_continuation == MAX_SPACE_RECONSTRUCTION_PAGE {
            std::thread::yield_now();
            entries_in_continuation = 0;
        }
        let entry = entry?;
        let metadata = entry.path().symlink_metadata()?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(std::io::Error::other(
                "Logs contains an unsupported non-regular entry",
            ));
        }
        let bytes = metadata
            .blocks()
            .checked_mul(512)
            .ok_or_else(|| std::io::Error::other("log allocation overflow"))?;
        allocated = allocated
            .checked_add(bytes)
            .ok_or_else(|| std::io::Error::other("log allocation overflow"))?;
    }
    Ok(allocated)
}

pub fn prune_oldest_closed_rotations_batch(
    directory: &File,
    current_allocated_bytes: u64,
    target_allocated_bytes: u64,
) -> std::io::Result<bool> {
    prune_oldest_closed_rotations_batch_excluding(
        directory,
        current_allocated_bytes,
        target_allocated_bytes,
        None,
    )
}

fn prune_oldest_closed_rotations_batch_excluding(
    directory: &File,
    current_allocated_bytes: u64,
    target_allocated_bytes: u64,
    excluded_filename: Option<&str>,
) -> std::io::Result<bool> {
    if current_allocated_bytes <= target_allocated_bytes {
        return Ok(false);
    }
    let directory_path = format!("/proc/self/fd/{}", directory.as_raw_fd());
    let mut candidates = BinaryHeap::new();
    candidates
        .try_reserve_exact(MAX_SPACE_RECONSTRUCTION_PAGE)
        .map_err(|_| std::io::Error::other("could not reserve bounded log cleanup batch"))?;
    let mut entries_in_continuation = 0_usize;
    for entry in std::fs::read_dir(directory_path)? {
        entries_in_continuation = entries_in_continuation
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("log entry count overflow"))?;
        if entries_in_continuation == MAX_SPACE_RECONSTRUCTION_PAGE {
            std::thread::yield_now();
            entries_in_continuation = 0;
        }
        let entry = entry?;
        let filename = entry.file_name();
        let filename_text = filename
            .to_str()
            .ok_or_else(|| std::io::Error::other("Logs contains a non-UTF-8 entry"))?;
        let metadata = entry.path().symlink_metadata()?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(std::io::Error::other(
                "Logs contains an unsupported non-regular entry",
            ));
        }
        if !is_momento_log_rotation(filename_text) {
            continue;
        }
        if excluded_filename == Some(filename_text) {
            continue;
        }
        let allocated = metadata
            .blocks()
            .checked_mul(512)
            .ok_or_else(|| std::io::Error::other("log allocation overflow"))?;
        let candidate = (
            metadata.mtime(),
            metadata.mtime_nsec(),
            filename,
            allocated,
            metadata.dev(),
            metadata.ino(),
        );
        if candidates.len() < MAX_SPACE_RECONSTRUCTION_PAGE {
            candidates.push(candidate);
        } else if candidates
            .peek()
            .is_some_and(|newest_retained| candidate < *newest_retained)
        {
            candidates.pop();
            candidates.push(candidate);
        }
    }
    let mut retained = current_allocated_bytes;
    let mut removed = false;
    for (_, _, filename, allocated, expected_device, expected_inode) in candidates.into_sorted_vec()
    {
        if retained <= target_allocated_bytes {
            break;
        }
        let filename = CString::new(filename.as_encoded_bytes())
            .map_err(|_| std::io::Error::other("log filename contains a null byte"))?;
        validate_cleanup_candidate(
            directory,
            &filename,
            expected_device,
            expected_inode,
            allocated,
        )?;
        let result = unsafe { libc::unlinkat(directory.as_raw_fd(), filename.as_ptr(), 0) };
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        retained = retained.saturating_sub(allocated);
        removed = true;
    }
    if removed {
        directory.sync_all()?;
    }
    Ok(removed)
}

fn validate_cleanup_candidate(
    directory: &File,
    filename: &CString,
    expected_device: u64,
    expected_inode: u64,
    expected_allocated: u64,
) -> std::io::Result<()> {
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            filename.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let status = unsafe { status.assume_init() };
    let allocated = u64::try_from(status.st_blocks)
        .ok()
        .and_then(|blocks| blocks.checked_mul(512))
        .ok_or_else(|| std::io::Error::other("log allocation overflow"))?;
    if status.st_mode & libc::S_IFMT != libc::S_IFREG
        || status.st_dev != expected_device
        || status.st_ino != expected_inode
        || allocated != expected_allocated
    {
        return Err(std::io::Error::other(
            "log rotation changed during quota recovery",
        ));
    }
    Ok(())
}

fn is_momento_log_rotation(filename: &str) -> bool {
    let Some(stem) = filename
        .strip_prefix("momento-api.")
        .and_then(|value| value.strip_suffix(".log"))
    else {
        return false;
    };
    let date = stem.get(..10).unwrap_or_default();
    if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return false;
    }
    let suffix = &stem[date.len()..];
    suffix.is_empty()
        || suffix.strip_prefix('.').is_some_and(|sequence| {
            sequence.len() == 3 && sequence.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::space_budget::FilesystemSpaceSnapshot;

    const GIBIBYTE: u64 = 1024 * 1024 * 1024;

    fn running_budget() -> DataDirSpaceBudget {
        let budget = DataDirSpaceBudget::from_snapshot(FilesystemSpaceSnapshot {
            filesystem_id: "log-drain-test".to_string(),
            total_bytes: 100 * GIBIBYTE,
            free_bytes: 100 * GIBIBYTE,
            fragment_size: 4096,
        })
        .expect("log drain budget");
        let mut reconstruction = budget.begin_reconstruction();
        reconstruction.set_allocated_bytes(0, 0);
        reconstruction.publish().expect("log budget recovery");
        budget.mark_running().expect("running log budget");
        budget
    }

    #[test]
    fn drain_batch_retains_one_event_at_the_byte_boundary() {
        let directory = tempfile::tempdir().expect("log directory");
        let logs = File::open(directory.path()).expect("logs handle");
        let budget = running_budget();
        let (producer, consumer) = bounded_log_ring(8).expect("log ring");
        for _ in 0..5 {
            producer.try_emit(LogSeverity::Info, vec![b'x'; MAX_LOG_EVENT_BYTES]);
        }
        let mut writer = RuntimeLogWriter::new();

        writer.drain(&consumer, &logs, &budget);
        assert_eq!(writer.logical_bytes, MAX_LOG_DRAIN_BYTES as u64);
        assert!(writer.pending_event.is_some());
        assert!(consumer.receiver.is_empty());

        writer.drain(&consumer, &logs, &budget);
        assert_eq!(
            writer.logical_bytes,
            (MAX_LOG_DRAIN_BYTES + MAX_LOG_EVENT_BYTES) as u64
        );
        assert!(writer.pending_event.is_none());
    }

    #[test]
    fn drain_batch_stops_at_the_event_count_boundary() {
        let directory = tempfile::tempdir().expect("log directory");
        let logs = File::open(directory.path()).expect("logs handle");
        let budget = running_budget();
        let (producer, consumer) = bounded_log_ring(65).expect("log ring");
        for _ in 0..65 {
            producer.try_emit(LogSeverity::Info, vec![b'x']);
        }
        let mut writer = RuntimeLogWriter::new();

        writer.drain(&consumer, &logs, &budget);
        assert_eq!(writer.logical_bytes, MAX_LOG_DRAIN_BATCH as u64);
        assert_eq!(consumer.receiver.len(), 1);

        writer.drain(&consumer, &logs, &budget);
        assert_eq!(writer.logical_bytes, 65);
        assert!(consumer.receiver.is_empty());
    }
}
