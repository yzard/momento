use std::collections::BTreeMap;
use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::config::MediaProcessConfig;
use crate::executor::{CpuExecutorHandle, FileIoExecutorHandle};
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::io::session::ChildDescriptorAccess;
use crate::io::session::ChildDescriptorLease;

const MAXIMUM_PERSISTED_PROCESS_DIAGNOSTIC_CHARACTERS: usize = 16 * 1024;
const MAXIMUM_CHILD_ARGUMENTS: usize = 256;
const MAXIMUM_CHILD_ARGUMENT_BYTES: usize = 64 * 1024;
const MAXIMUM_CHILD_DIAGNOSTIC_BYTES: usize = 8 * 1024 * 1024;
const MAXIMUM_CHILD_DESCRIPTORS: usize = 8;
const CHILD_DESCRIPTOR_MINIMUM: i32 = 10;
const CHILD_DESCRIPTOR_MAXIMUM: i32 = CHILD_DESCRIPTOR_MINIMUM + MAXIMUM_CHILD_DESCRIPTORS as i32;
const CHILD_TERMINATION_GRACE: Duration = Duration::from_secs(10);
const CHILD_EXEC_START_TIMEOUT: Duration = Duration::from_secs(30);
const MAXIMUM_CHILD_FILE_BYTES: u64 = 32 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaTool {
    ImageMagick,
    Identify,
    ExifTool,
    Ffmpeg {
        validated_media_duration: Option<Duration>,
    },
    Ffprobe,
}

impl MediaTool {
    pub(crate) fn executable(self) -> &'static OsStr {
        match self {
            Self::ImageMagick => OsStr::new("convert"),
            Self::Identify => OsStr::new("identify"),
            Self::ExifTool => OsStr::new("exiftool"),
            Self::Ffmpeg { .. } => OsStr::new("ffmpeg"),
            Self::Ffprobe => OsStr::new("ffprobe"),
        }
    }

    fn address_space_limit_bytes(self) -> u64 {
        match self {
            Self::ImageMagick | Self::Identify => 512 * 1024 * 1024,
            Self::ExifTool => 256 * 1024 * 1024,
            Self::Ffmpeg { .. } | Self::Ffprobe => 1024 * 1024 * 1024,
        }
    }

    fn total_runtime(self) -> Duration {
        match self {
            Self::ImageMagick | Self::Identify => Duration::from_secs(30 * 60),
            Self::ExifTool | Self::Ffprobe => Duration::from_secs(10 * 60),
            Self::Ffmpeg {
                validated_media_duration,
            } => ffmpeg_runtime_limit(validated_media_duration),
        }
    }
}

pub(crate) struct ChildProcessSpec {
    tool: MediaTool,
    arguments: Vec<OsString>,
    maximum_stdout_bytes: usize,
    maximum_stderr_bytes: usize,
    leases: Vec<ChildDescriptorLease>,
    stdout_child_fd: Option<i32>,
}

pub(crate) struct ChildProcessCompletion {
    pub result: Result<ExternalProcessOutput, ExternalProcessError>,
    pub leases: Vec<ChildDescriptorLease>,
}

pub(crate) enum StorageChildDescriptor {
    Read {
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
        child_fd: i32,
    },
    Write {
        storage_root: StorageRootId,
        path: NormalizedStoragePath,
        child_fd: i32,
        rollback_length: u64,
        require_non_empty: bool,
        maximum_bytes: u64,
    },
}

struct StorageMediaToolRequest {
    tool: MediaTool,
    arguments: Vec<OsString>,
    maximum_stdout_bytes: usize,
    maximum_stderr_bytes: usize,
    descriptors: Vec<StorageChildDescriptor>,
    stdout_child_fd: Option<i32>,
}

impl StorageChildDescriptor {
    fn child_fd(&self) -> i32 {
        match self {
            Self::Read { child_fd, .. } | Self::Write { child_fd, .. } => *child_fd,
        }
    }

    fn access(&self) -> ChildDescriptorAccess {
        match self {
            Self::Read { .. } => ChildDescriptorAccess::Read,
            Self::Write { .. } => ChildDescriptorAccess::Write,
        }
    }
}

pub(crate) async fn run_storage_media_tool(
    cpu: &CpuExecutorHandle,
    file_io: &FileIoExecutorHandle,
    tool: MediaTool,
    arguments: Vec<OsString>,
    maximum_stdout_bytes: usize,
    maximum_stderr_bytes: usize,
    descriptors: Vec<StorageChildDescriptor>,
) -> Result<ExternalProcessOutput, ExternalProcessError> {
    run_storage_media_tool_inner(
        cpu,
        file_io,
        StorageMediaToolRequest {
            tool,
            arguments,
            maximum_stdout_bytes,
            maximum_stderr_bytes,
            descriptors,
            stdout_child_fd: None,
        },
    )
    .await
}

pub(crate) async fn run_storage_media_tool_with_stdout(
    cpu: &CpuExecutorHandle,
    file_io: &FileIoExecutorHandle,
    tool: MediaTool,
    arguments: Vec<OsString>,
    maximum_stderr_bytes: usize,
    descriptors: Vec<StorageChildDescriptor>,
    stdout_child_fd: i32,
) -> Result<ExternalProcessOutput, ExternalProcessError> {
    run_storage_media_tool_inner(
        cpu,
        file_io,
        StorageMediaToolRequest {
            tool,
            arguments,
            maximum_stdout_bytes: 0,
            maximum_stderr_bytes,
            descriptors,
            stdout_child_fd: Some(stdout_child_fd),
        },
    )
    .await
}

async fn run_storage_media_tool_inner(
    cpu: &CpuExecutorHandle,
    file_io: &FileIoExecutorHandle,
    request: StorageMediaToolRequest,
) -> Result<ExternalProcessOutput, ExternalProcessError> {
    let StorageMediaToolRequest {
        tool,
        arguments,
        maximum_stdout_bytes,
        maximum_stderr_bytes,
        descriptors,
        stdout_child_fd,
    } = request;
    let mut leases = Vec::new();
    leases
        .try_reserve_exact(descriptors.len())
        .map_err(|error| ExternalProcessError::Setup(error.to_string()))?;
    if descriptors.iter().any(|descriptor| {
        matches!(
            descriptor,
            StorageChildDescriptor::Write { maximum_bytes, .. }
                if *maximum_bytes == 0 || *maximum_bytes > MAXIMUM_CHILD_FILE_BYTES
        )
    }) {
        return Err(ExternalProcessError::Setup(format!(
            "child output limit must be between 1 and {MAXIMUM_CHILD_FILE_BYTES} bytes"
        )));
    }
    for descriptor in &descriptors {
        let session = match descriptor {
            StorageChildDescriptor::Read {
                storage_root, path, ..
            } => file_io
                .open_storage_read_session_durable(*storage_root, path.clone())
                .await
                .map(|(session, _)| session),
            StorageChildDescriptor::Write {
                storage_root,
                path,
                rollback_length,
                ..
            } => {
                file_io
                    .open_storage_write_session_durable(
                        *storage_root,
                        path.clone(),
                        *rollback_length,
                    )
                    .await
            }
        }
        .map_err(|error| ExternalProcessError::Executor(error.to_string()))?;
        let lease = file_io
            .pin_storage_session_for_child_durable(
                session,
                descriptor.child_fd(),
                descriptor.access(),
            )
            .await
            .map_err(|error| ExternalProcessError::Executor(error.to_string()))?;
        leases.push(lease);
    }
    let mut spec = ChildProcessSpec::new(
        tool,
        arguments,
        maximum_stdout_bytes,
        maximum_stderr_bytes,
        leases,
    )
    .map_err(ExternalProcessError::Setup)?;
    if let Some(stdout_child_fd) = stdout_child_fd {
        spec = spec
            .redirect_stdout_to(stdout_child_fd)
            .map_err(ExternalProcessError::Setup)?;
    }
    let mut completion = cpu
        .supervise_child_process_durable(spec)
        .await
        .map_err(|error| ExternalProcessError::Executor(error.to_string()))?;
    if completion.leases.len() != descriptors.len() {
        return Err(ExternalProcessError::Setup(
            "child returned the wrong descriptor lease count".to_string(),
        ));
    }
    let mut returned = Vec::new();
    returned
        .try_reserve_exact(descriptors.len())
        .map_err(|error| ExternalProcessError::Setup(error.to_string()))?;
    for (descriptor, lease) in descriptors.into_iter().zip(completion.leases.drain(..)) {
        let session = file_io
            .return_storage_session_from_child_durable(lease)
            .await
            .map_err(|error| ExternalProcessError::Executor(error.to_string()))?;
        returned.push((descriptor, session));
    }
    let process_succeeded = completion
        .result
        .as_ref()
        .is_ok_and(|output| output.status.success());
    let mut cleanup_error = None;
    for (descriptor, session) in returned {
        let result = match descriptor {
            StorageChildDescriptor::Read { .. } => {
                file_io.close_storage_session_durable(session).await
            }
            StorageChildDescriptor::Write {
                require_non_empty,
                maximum_bytes,
                ..
            } if process_succeeded => {
                let inspected = file_io.inspect_storage_session_durable(session).await;
                match inspected {
                    Ok((session, snapshot))
                        if (!require_non_empty || snapshot.byte_size > 0)
                            && snapshot.byte_size <= maximum_bytes =>
                    {
                        file_io.commit_storage_session_durable(session).await
                    }
                    Ok((session, snapshot)) => {
                        let _ = file_io.abort_storage_session_durable(session).await;
                        let detail = if require_non_empty && snapshot.byte_size == 0 {
                            "child reported success but produced an empty output".to_string()
                        } else {
                            format!(
                                "child output contains {} bytes; maximum is {maximum_bytes}",
                                snapshot.byte_size
                            )
                        };
                        Err(crate::executor::ExecutorError::new(
                            crate::executor::ExecutorErrorKind::FileInvalidData,
                            "run_storage_media_tool",
                            detail,
                        ))
                    }
                    Err(error) => Err(error),
                }
            }
            StorageChildDescriptor::Write { .. } => {
                file_io.abort_storage_session_durable(session).await
            }
        };
        if cleanup_error.is_none() {
            cleanup_error = result.err();
        }
    }
    if let Some(error) = cleanup_error {
        return Err(ExternalProcessError::Executor(error.to_string()));
    }
    completion.result
}

impl ChildProcessSpec {
    pub(crate) fn new(
        tool: MediaTool,
        arguments: Vec<OsString>,
        maximum_stdout_bytes: usize,
        maximum_stderr_bytes: usize,
        leases: Vec<ChildDescriptorLease>,
    ) -> Result<Self, String> {
        validate_child_process_arguments(&arguments)?;
        if maximum_stdout_bytes > MAXIMUM_CHILD_DIAGNOSTIC_BYTES
            || maximum_stderr_bytes > MAXIMUM_CHILD_DIAGNOSTIC_BYTES
        {
            return Err(format!(
                "child diagnostic capture exceeds {} bytes",
                MAXIMUM_CHILD_DIAGNOSTIC_BYTES
            ));
        }
        validate_child_descriptor_leases(&leases)?;
        Ok(Self {
            tool,
            arguments,
            maximum_stdout_bytes,
            maximum_stderr_bytes,
            leases,
            stdout_child_fd: None,
        })
    }

    pub(crate) fn redirect_stdout_to(mut self, child_fd: i32) -> Result<Self, String> {
        if !self.leases.iter().any(|lease| {
            lease.child_fd() == child_fd && lease.access() == ChildDescriptorAccess::Write
        }) {
            return Err("child stdout target must be a declared writable descriptor".to_string());
        }
        self.stdout_child_fd = Some(child_fd);
        Ok(self)
    }

    pub(crate) fn maximum_input_bytes(&self) -> usize {
        self.arguments
            .iter()
            .map(|argument| argument.as_encoded_bytes().len())
            .sum::<usize>()
            + self.leases.len() * size_of::<ChildDescriptorLease>()
    }

    pub(crate) fn maximum_output_bytes(&self) -> usize {
        self.maximum_stdout_bytes
            .saturating_add(self.maximum_stderr_bytes)
            .saturating_add(self.leases.len() * size_of::<ChildDescriptorLease>())
    }

    pub(crate) fn run(self) -> ChildProcessCompletion {
        let Self {
            tool,
            arguments,
            maximum_stdout_bytes,
            maximum_stderr_bytes,
            leases,
            stdout_child_fd,
        } = self;
        let result = run_supervised_child(
            tool,
            &arguments,
            maximum_stdout_bytes,
            maximum_stderr_bytes,
            &leases,
            stdout_child_fd,
        );
        ChildProcessCompletion { result, leases }
    }
}

fn validate_child_process_arguments(arguments: &[OsString]) -> Result<(), String> {
    if arguments.len() > MAXIMUM_CHILD_ARGUMENTS {
        return Err(format!(
            "child has {} arguments; maximum is {MAXIMUM_CHILD_ARGUMENTS}",
            arguments.len()
        ));
    }
    let mut encoded_bytes = 0_usize;
    for argument in arguments {
        let bytes = argument.as_encoded_bytes();
        if bytes.contains(&0) {
            return Err("child argument contains NUL".to_string());
        }
        encoded_bytes = encoded_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| "child argument size overflowed".to_string())?;
    }
    if encoded_bytes > MAXIMUM_CHILD_ARGUMENT_BYTES {
        return Err(format!(
            "child arguments contain {encoded_bytes} bytes; maximum is {MAXIMUM_CHILD_ARGUMENT_BYTES}"
        ));
    }
    Ok(())
}

fn validate_child_descriptor_leases(leases: &[ChildDescriptorLease]) -> Result<(), String> {
    if leases.len() > MAXIMUM_CHILD_DESCRIPTORS {
        return Err(format!(
            "child has {} descriptors; maximum is {MAXIMUM_CHILD_DESCRIPTORS}",
            leases.len()
        ));
    }
    for (index, lease) in leases.iter().enumerate() {
        if !(CHILD_DESCRIPTOR_MINIMUM..CHILD_DESCRIPTOR_MAXIMUM).contains(&lease.child_fd()) {
            return Err(format!(
                "child descriptor {} is outside the reserved range {CHILD_DESCRIPTOR_MINIMUM}..{CHILD_DESCRIPTOR_MAXIMUM}",
                lease.child_fd()
            ));
        }
        if leases[..index]
            .iter()
            .any(|other| other.child_fd() == lease.child_fd())
        {
            return Err(format!(
                "child descriptor {} is declared more than once",
                lease.child_fd()
            ));
        }
        let _declared_access = lease.access();
    }
    Ok(())
}

fn ffmpeg_runtime_limit(validated_media_duration: Option<Duration>) -> Duration {
    const MINIMUM: Duration = Duration::from_secs(30 * 60);
    const EXTRA: Duration = Duration::from_secs(10 * 60);
    const MAXIMUM: Duration = Duration::from_secs(24 * 60 * 60);
    let Some(duration) = validated_media_duration else {
        return MAXIMUM;
    };
    duration
        .checked_mul(2)
        .and_then(|duration| duration.checked_add(EXTRA))
        .map(|duration| duration.max(MINIMUM).min(MAXIMUM))
        .unwrap_or(MAXIMUM)
}

fn run_supervised_child(
    tool: MediaTool,
    arguments: &[OsString],
    maximum_stdout_bytes: usize,
    maximum_stderr_bytes: usize,
    leases: &[ChildDescriptorLease],
    stdout_child_fd: Option<i32>,
) -> Result<ExternalProcessOutput, ExternalProcessError> {
    let mut descriptor_duplicates = Vec::new();
    descriptor_duplicates
        .try_reserve_exact(leases.len())
        .map_err(|error| ExternalProcessError::Setup(error.to_string()))?;
    for lease in leases {
        let duplicate = unsafe { libc::fcntl(lease.raw_fd(), libc::F_DUPFD_CLOEXEC, 64) };
        if duplicate < 0 {
            return Err(ExternalProcessError::Setup(
                io::Error::last_os_error().to_string(),
            ));
        }
        descriptor_duplicates.push((unsafe { OwnedFd::from_raw_fd(duplicate) }, lease.child_fd()));
    }
    let descriptor_actions = descriptor_duplicates
        .iter()
        .map(|(descriptor, child_fd)| (descriptor.as_raw_fd(), *child_fd))
        .collect::<Vec<_>>();
    let command = PreparedChildCommand::new(tool.executable(), tool_arguments(tool), arguments)?;
    let result = run_prepared_command(
        command,
        tool.executable(),
        ChildCaptureSpec {
            maximum_stdout_bytes,
            maximum_stderr_bytes,
            runtime_timeout: tool.total_runtime(),
        },
        ChildLaunchSpec {
            descriptor_actions,
            stdout_child_fd,
            address_space_limit_bytes: tool.address_space_limit_bytes(),
            file_size_limit_bytes: MAXIMUM_CHILD_FILE_BYTES,
            start_timeout: CHILD_EXEC_START_TIMEOUT,
        },
    );
    drop(descriptor_duplicates);
    result
}

fn tool_arguments(tool: MediaTool) -> Vec<OsString> {
    match tool {
        MediaTool::Ffmpeg { .. } => ffmpeg_single_thread_arguments(),
        MediaTool::Ffprobe => ffprobe_single_thread_arguments(),
        MediaTool::ImageMagick | MediaTool::Identify | MediaTool::ExifTool => Vec::new(),
    }
}

struct PreparedChildCommand {
    executable: CString,
    arguments: Vec<CString>,
    environment: Vec<CString>,
}

impl PreparedChildCommand {
    fn new(
        executable: &OsStr,
        tool_arguments: Vec<OsString>,
        arguments: &[OsString],
    ) -> Result<Self, ExternalProcessError> {
        let executable = os_string_to_c_string(executable)
            .map_err(|error| ExternalProcessError::Setup(error.to_string()))?;
        let total_arguments = tool_arguments
            .len()
            .checked_add(arguments.len())
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| ExternalProcessError::Setup("child argument count overflowed".into()))?;
        let mut prepared_arguments = Vec::new();
        prepared_arguments
            .try_reserve_exact(total_arguments)
            .map_err(|error| ExternalProcessError::Setup(error.to_string()))?;
        prepared_arguments.push(executable.clone());
        for argument in tool_arguments.iter().chain(arguments) {
            prepared_arguments.push(
                os_string_to_c_string(argument)
                    .map_err(|error| ExternalProcessError::Setup(error.to_string()))?,
            );
        }

        let mut environment = std::env::vars_os().collect::<BTreeMap<_, _>>();
        for name in [
            "OMP_NUM_THREADS",
            "OPENBLAS_NUM_THREADS",
            "MKL_NUM_THREADS",
            "VECLIB_MAXIMUM_THREADS",
            "NUMEXPR_NUM_THREADS",
            "MAGICK_THREAD_LIMIT",
        ] {
            environment.insert(OsString::from(name), OsString::from("1"));
        }
        let mut prepared_environment = Vec::new();
        prepared_environment
            .try_reserve_exact(environment.len())
            .map_err(|error| ExternalProcessError::Setup(error.to_string()))?;
        for (name, value) in environment {
            let mut entry = name;
            entry.push("=");
            entry.push(value);
            prepared_environment.push(
                os_string_to_c_string(&entry)
                    .map_err(|error| ExternalProcessError::Setup(error.to_string()))?,
            );
        }
        Ok(Self {
            executable,
            arguments: prepared_arguments,
            environment: prepared_environment,
        })
    }

    fn argument_pointers(&self) -> Vec<*const libc::c_char> {
        self.arguments
            .iter()
            .map(|argument| argument.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect()
    }

    fn environment_pointers(&self) -> Vec<*const libc::c_char> {
        self.environment
            .iter()
            .map(|entry| entry.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect()
    }
}

fn os_string_to_c_string(value: &OsStr) -> Result<CString, io::Error> {
    CString::new(value.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "value contains NUL"))
}

struct SupervisedChild {
    pid: libc::pid_t,
    stdout: Option<File>,
    stderr: Option<File>,
    exit_status: Option<ExitStatus>,
}

struct ChildCaptureSpec {
    maximum_stdout_bytes: usize,
    maximum_stderr_bytes: usize,
    runtime_timeout: Duration,
}

struct ChildLaunchSpec {
    descriptor_actions: Vec<(i32, i32)>,
    stdout_child_fd: Option<i32>,
    address_space_limit_bytes: u64,
    file_size_limit_bytes: u64,
    start_timeout: Duration,
}

impl SupervisedChild {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.exit_status {
            return Ok(Some(status));
        }
        let mut raw_status = 0;
        let result = unsafe { libc::waitpid(self.pid, &mut raw_status, libc::WNOHANG) };
        if result == 0 {
            return Ok(None);
        }
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        let status = ExitStatus::from_raw(raw_status);
        self.exit_status = Some(status);
        Ok(Some(status))
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.exit_status {
            return Ok(status);
        }
        loop {
            let mut raw_status = 0;
            let result = unsafe { libc::waitpid(self.pid, &mut raw_status, 0) };
            if result >= 0 {
                let status = ExitStatus::from_raw(raw_status);
                self.exit_status = Some(status);
                return Ok(status);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

#[derive(Debug)]
enum ChildStartError {
    Io(io::Error),
    TimedOut,
}

fn spawn_child_with_deadline(
    command: &PreparedChildCommand,
    launch_spec: ChildLaunchSpec,
) -> Result<SupervisedChild, ChildStartError> {
    let argument_pointers = command.argument_pointers();
    let environment_pointers = command.environment_pointers();
    let (stdout_read, stdout_write) = create_cloexec_pipe().map_err(ChildStartError::Io)?;
    let (stderr_read, stderr_write) = create_cloexec_pipe().map_err(ChildStartError::Io)?;
    let (exec_read, exec_write) = create_cloexec_pipe().map_err(ChildStartError::Io)?;
    let null_fd = open_null_descriptor().map_err(ChildStartError::Io)?;
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(ChildStartError::Io(io::Error::last_os_error()));
    }
    if pid == 0 {
        drop(stdout_read);
        drop(stderr_read);
        drop(exec_read);
        let context = ChildForkContext {
            executable: command.executable.as_ptr(),
            arguments: argument_pointers.as_ptr(),
            environment: environment_pointers.as_ptr(),
            null_fd: null_fd.as_raw_fd(),
            stdout_fd: stdout_write.as_raw_fd(),
            stderr_fd: stderr_write.as_raw_fd(),
            exec_status_fd: exec_write.as_raw_fd(),
            descriptor_actions: &launch_spec.descriptor_actions,
            stdout_child_fd: launch_spec.stdout_child_fd,
            address_space_limit_bytes: launch_spec.address_space_limit_bytes,
            file_size_limit_bytes: launch_spec.file_size_limit_bytes,
        };
        unsafe { launch_child_process(&context) };
    }
    drop(stdout_write);
    drop(stderr_write);
    drop(exec_write);
    drop(null_fd);
    unsafe {
        libc::setpgid(pid, pid);
    }
    let mut child = SupervisedChild {
        pid,
        stdout: Some(File::from(stdout_read)),
        stderr: Some(File::from(stderr_read)),
        exit_status: None,
    };
    match wait_for_exec_handshake(exec_read.as_raw_fd(), launch_spec.start_timeout) {
        Ok(()) => Ok(child),
        Err(ChildStartError::Io(error)) => {
            signal_process_group(pid, libc::SIGKILL);
            let _ = child.wait();
            Err(ChildStartError::Io(error))
        }
        Err(ChildStartError::TimedOut) => {
            signal_process_group(pid, libc::SIGKILL);
            let _ = child.wait();
            Err(ChildStartError::TimedOut)
        }
    }
}

fn create_cloexec_pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut descriptors = [-1_i32; 2];
    if unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe {
        (
            OwnedFd::from_raw_fd(descriptors[0]),
            OwnedFd::from_raw_fd(descriptors[1]),
        )
    })
}

fn open_null_descriptor() -> io::Result<OwnedFd> {
    let path = c"/dev/null";
    let descriptor = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(descriptor) })
}

fn wait_for_exec_handshake(descriptor: i32, timeout: Duration) -> Result<(), ChildStartError> {
    let started = Instant::now();
    let mut error_bytes = [0_u8; size_of::<i32>()];
    let mut received = 0_usize;
    loop {
        let remaining = timeout
            .checked_sub(started.elapsed())
            .ok_or(ChildStartError::TimedOut)?;
        let timeout_millis = i32::try_from(remaining.as_millis().max(1)).unwrap_or(i32::MAX);
        let mut poll_descriptor = libc::pollfd {
            fd: descriptor,
            events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut poll_descriptor, 1, timeout_millis) };
        if result == 0 {
            return Err(ChildStartError::TimedOut);
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(ChildStartError::Io(error));
        }
        let read = unsafe {
            libc::read(
                descriptor,
                error_bytes[received..].as_mut_ptr().cast(),
                error_bytes.len() - received,
            )
        };
        if read == 0 {
            if received == 0 {
                return Ok(());
            }
            return Err(ChildStartError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "child exec handshake was truncated",
            )));
        }
        if read < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(ChildStartError::Io(error));
        }
        received += read as usize;
        if received == error_bytes.len() {
            return Err(ChildStartError::Io(io::Error::from_raw_os_error(
                i32::from_ne_bytes(error_bytes),
            )));
        }
    }
}

fn signal_process_group(pid: libc::pid_t, signal: i32) {
    unsafe {
        libc::kill(-pid, signal);
        libc::kill(pid, signal);
    }
}

struct ChildForkContext<'a> {
    executable: *const libc::c_char,
    arguments: *const *const libc::c_char,
    environment: *const *const libc::c_char,
    null_fd: i32,
    stdout_fd: i32,
    stderr_fd: i32,
    exec_status_fd: i32,
    descriptor_actions: &'a [(i32, i32)],
    stdout_child_fd: Option<i32>,
    address_space_limit_bytes: u64,
    file_size_limit_bytes: u64,
}

unsafe fn launch_child_process(context: &ChildForkContext<'_>) -> ! {
    let result = (|| -> io::Result<()> {
        if unsafe { libc::setpgid(0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        set_child_resource_limit(
            ChildResourceLimit::AddressSpace,
            context.address_space_limit_bytes,
        )?;
        set_child_resource_limit(ChildResourceLimit::FileSize, context.file_size_limit_bytes)?;
        mark_child_descriptors_close_on_exec()?;
        duplicate_child_descriptor(context.null_fd, libc::STDIN_FILENO)?;
        duplicate_child_descriptor(context.stdout_fd, libc::STDOUT_FILENO)?;
        duplicate_child_descriptor(context.stderr_fd, libc::STDERR_FILENO)?;
        for (source, destination) in context.descriptor_actions {
            duplicate_child_descriptor(*source, *destination)?;
        }
        if let Some(stdout_child_fd) = context.stdout_child_fd {
            duplicate_child_descriptor(stdout_child_fd, libc::STDOUT_FILENO)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        write_exec_error_and_exit(
            context.exec_status_fd,
            error.raw_os_error().unwrap_or(libc::EIO),
        );
    }
    unsafe {
        execvpe(context.executable, context.arguments, context.environment);
    }
    write_exec_error_and_exit(
        context.exec_status_fd,
        io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO),
    );
}

fn duplicate_child_descriptor(source: i32, destination: i32) -> io::Result<()> {
    if unsafe { libc::dup2(source, destination) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn write_exec_error_and_exit(descriptor: i32, error: i32) -> ! {
    let bytes = error.to_ne_bytes();
    unsafe {
        libc::write(descriptor, bytes.as_ptr().cast(), bytes.len());
        libc::_exit(127);
    }
}

unsafe extern "C" {
    fn execvpe(
        file: *const libc::c_char,
        argv: *const *const libc::c_char,
        envp: *const *const libc::c_char,
    ) -> libc::c_int;
}

fn run_prepared_command(
    command: PreparedChildCommand,
    executable: &OsStr,
    capture_spec: ChildCaptureSpec,
    launch_spec: ChildLaunchSpec,
) -> Result<ExternalProcessOutput, ExternalProcessError> {
    let start_timeout = launch_spec.start_timeout;
    let child =
        spawn_child_with_deadline(&command, launch_spec).map_err(|source| match source {
            ChildStartError::Io(source) => ExternalProcessError::Start {
                executable: executable.to_string_lossy().into_owned(),
                source,
            },
            ChildStartError::TimedOut => ExternalProcessError::StartTimedOut {
                executable: executable.to_string_lossy().into_owned(),
                timeout: start_timeout,
            },
        })?;
    let mut child = ChildProcessGuard::new(child);
    let mut stdout = child
        .child_mut()
        .stdout
        .take()
        .ok_or(ExternalProcessError::MissingOutputPipe("stdout"))?;
    let mut stderr = child
        .child_mut()
        .stderr
        .take()
        .ok_or(ExternalProcessError::MissingOutputPipe("stderr"))?;
    set_nonblocking(stdout.as_raw_fd()).map_err(|source| ExternalProcessError::Read {
        stream: "stdout",
        source,
    })?;
    set_nonblocking(stderr.as_raw_fd()).map_err(|source| ExternalProcessError::Read {
        stream: "stderr",
        source,
    })?;
    let mut stdout_capture = BoundedPipeCapture::new(capture_spec.maximum_stdout_bytes);
    let mut stderr_capture = BoundedPipeCapture::new(capture_spec.maximum_stderr_bytes);
    let status = supervise_child_pipes(
        child.child_mut(),
        &mut stdout,
        &mut stderr,
        &mut stdout_capture,
        &mut stderr_capture,
        capture_spec.runtime_timeout,
    )?;
    child.disarm();
    Ok(ExternalProcessOutput {
        status,
        stdout: stdout_capture.bytes,
        stderr: stderr_capture.bytes,
        stdout_truncated: stdout_capture.truncated,
        stderr_truncated: stderr_capture.truncated,
    })
}

struct ChildProcessGuard {
    child: Option<SupervisedChild>,
}

impl ChildProcessGuard {
    fn new(child: SupervisedChild) -> Self {
        Self { child: Some(child) }
    }

    fn child_mut(&mut self) -> &mut SupervisedChild {
        self.child.as_mut().expect("child guard is armed")
    }

    fn disarm(&mut self) {
        self.child = None;
    }
}

impl Drop for ChildProcessGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            terminate_process_group_immediately(child);
            let _ = child.wait();
        }
    }
}

struct BoundedPipeCapture {
    bytes: Vec<u8>,
    maximum_bytes: usize,
    truncated: bool,
    finished: bool,
}

impl BoundedPipeCapture {
    fn new(maximum_bytes: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(maximum_bytes.min(16 * 1024)),
            maximum_bytes,
            truncated: false,
            finished: false,
        }
    }

    fn drain(&mut self, stream: &mut impl Read) -> io::Result<()> {
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => {
                    self.finished = true;
                    return Ok(());
                }
                Ok(read) => {
                    let retained = self
                        .maximum_bytes
                        .saturating_sub(self.bytes.len())
                        .min(read);
                    self.bytes.extend_from_slice(&buffer[..retained]);
                    self.truncated |= retained < read;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }
}

fn supervise_child_pipes(
    child: &mut SupervisedChild,
    stdout: &mut File,
    stderr: &mut File,
    stdout_capture: &mut BoundedPipeCapture,
    stderr_capture: &mut BoundedPipeCapture,
    timeout: Duration,
) -> Result<ExitStatus, ExternalProcessError> {
    let started = Instant::now();
    let mut exit_status = None;
    loop {
        stdout_capture
            .drain(stdout)
            .map_err(|source| ExternalProcessError::Read {
                stream: "stdout",
                source,
            })?;
        stderr_capture
            .drain(stderr)
            .map_err(|source| ExternalProcessError::Read {
                stream: "stderr",
                source,
            })?;
        if exit_status.is_none() {
            exit_status = child.try_wait().map_err(ExternalProcessError::Wait)?;
        }
        if stdout_capture.finished && stderr_capture.finished {
            return exit_status
                .map(Ok)
                .unwrap_or_else(|| child.wait().map_err(ExternalProcessError::Wait));
        }
        if started.elapsed() >= timeout {
            terminate_process_group(child, stdout, stderr, stdout_capture, stderr_capture)?;
            return Err(ExternalProcessError::TimedOut(timeout));
        }
        poll_child_pipes(
            stdout.as_raw_fd(),
            stderr.as_raw_fd(),
            stdout_capture.finished,
            stderr_capture.finished,
        )
        .map_err(|source| ExternalProcessError::Read {
            stream: "stdout/stderr",
            source,
        })?;
    }
}

fn set_nonblocking(file_descriptor: std::os::fd::RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(file_descriptor, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(file_descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn poll_child_pipes(
    stdout: std::os::fd::RawFd,
    stderr: std::os::fd::RawFd,
    stdout_finished: bool,
    stderr_finished: bool,
) -> io::Result<()> {
    let mut descriptors = [
        libc::pollfd {
            fd: if stdout_finished { -1 } else { stdout },
            events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
            revents: 0,
        },
        libc::pollfd {
            fd: if stderr_finished { -1 } else { stderr },
            events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
            revents: 0,
        },
    ];
    let result = unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, 50) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    Ok(())
}

fn terminate_process_group(
    child: &mut SupervisedChild,
    stdout: &mut File,
    stderr: &mut File,
    stdout_capture: &mut BoundedPipeCapture,
    stderr_capture: &mut BoundedPipeCapture,
) -> Result<(), ExternalProcessError> {
    signal_process_group(child.pid, libc::SIGTERM);
    let started = Instant::now();
    while started.elapsed() < CHILD_TERMINATION_GRACE {
        stdout_capture
            .drain(stdout)
            .map_err(|source| ExternalProcessError::Read {
                stream: "stdout",
                source,
            })?;
        stderr_capture
            .drain(stderr)
            .map_err(|source| ExternalProcessError::Read {
                stream: "stderr",
                source,
            })?;
        if child
            .try_wait()
            .map_err(ExternalProcessError::Wait)?
            .is_some()
        {
            return Ok(());
        }
        poll_child_pipes(
            stdout.as_raw_fd(),
            stderr.as_raw_fd(),
            stdout_capture.finished,
            stderr_capture.finished,
        )
        .map_err(|source| ExternalProcessError::Read {
            stream: "stdout/stderr",
            source,
        })?;
    }
    terminate_process_group_immediately(child);
    child.wait().map_err(ExternalProcessError::Wait)?;
    Ok(())
}

fn terminate_process_group_immediately(child: &mut SupervisedChild) {
    signal_process_group(child.pid, libc::SIGKILL);
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
    #[error("{executable} did not complete exec startup within {timeout:?}")]
    StartTimedOut {
        executable: String,
        timeout: Duration,
    },
    #[error("failed while waiting for external process: {0}")]
    Wait(#[source] io::Error),
    #[error("external process exceeded its {0:?} runtime limit")]
    TimedOut(Duration),
    #[error("failed to prepare external process: {0}")]
    Setup(String),
    #[error("external process executor failed: {0}")]
    Executor(String),
    #[error("external process did not expose its {0} pipe")]
    MissingOutputPipe(&'static str),
    #[error("failed to read external process {stream}: {source}")]
    Read {
        stream: &'static str,
        #[source]
        source: io::Error,
    },
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
        "decoded image dimension {actual_dimension} exceeds the {maximum_dimension} pixel limit"
    )]
    DimensionLimitExceeded {
        actual_dimension: u64,
        maximum_dimension: u64,
    },
    #[error(
        "decoded image contains {actual_pixels} pixels, exceeding the {maximum_pixels} pixel limit"
    )]
    PixelLimitExceeded {
        actual_pixels: u64,
        maximum_pixels: u64,
    },
}

pub fn image_magick_resource_arguments(config: &MediaProcessConfig) -> Vec<OsString> {
    const IMAGEMAGICK_MEMORY_LIMIT_MEBIBYTES: u64 = 256;
    vec![
        "-limit".into(),
        "memory".into(),
        format!("{IMAGEMAGICK_MEMORY_LIMIT_MEBIBYTES}MiB").into(),
        "-limit".into(),
        "map".into(),
        "0MiB".into(),
        "-limit".into(),
        "disk".into(),
        "4096MiB".into(),
        "-limit".into(),
        "area".into(),
        format!("{}P", config.maximum_decoded_image_pixels).into(),
        "-limit".into(),
        "thread".into(),
        "1".into(),
    ]
}

pub fn ffmpeg_single_thread_arguments() -> Vec<OsString> {
    vec![
        "-threads".into(),
        "1".into(),
        "-filter_threads".into(),
        "1".into(),
        "-filter_complex_threads".into(),
        "1".into(),
    ]
}

pub fn ffprobe_single_thread_arguments() -> Vec<OsString> {
    vec!["-threads".into(), "1".into()]
}

pub async fn validate_storage_image_dimensions(
    cpu: &CpuExecutorHandle,
    file_io: &FileIoExecutorHandle,
    storage_root: StorageRootId,
    path: NormalizedStoragePath,
    config: &MediaProcessConfig,
) -> Result<(), ImageDimensionError> {
    inspect_storage_image_dimensions(cpu, file_io, storage_root, path, config)
        .await
        .map(|_| ())
}

pub async fn inspect_storage_image_dimensions(
    cpu: &CpuExecutorHandle,
    file_io: &FileIoExecutorHandle,
    storage_root: StorageRootId,
    path: NormalizedStoragePath,
    config: &MediaProcessConfig,
) -> Result<(u32, u32), ImageDimensionError> {
    const INPUT_DESCRIPTOR: i32 = 10;
    let mut descriptor_path = OsString::from(format!("/proc/self/fd/{INPUT_DESCRIPTOR}"));
    descriptor_path.push("[0]");
    let mut arguments = image_magick_resource_arguments(config);
    arguments.extend([
        OsString::from("-ping"),
        OsString::from("-format"),
        OsString::from("%w %h"),
        descriptor_path,
    ]);
    let output = run_storage_media_tool(
        cpu,
        file_io,
        MediaTool::Identify,
        arguments,
        128,
        config.maximum_stderr_bytes,
        vec![StorageChildDescriptor::Read {
            storage_root,
            path,
            child_fd: INPUT_DESCRIPTOR,
        }],
    )
    .await
    .map_err(ImageDimensionError::Process)?;
    validate_dimension_output(output, config.maximum_decoded_image_pixels)
}

pub async fn inspect_storage_oriented_image_dimensions(
    cpu: &CpuExecutorHandle,
    file_io: &FileIoExecutorHandle,
    storage_root: StorageRootId,
    path: NormalizedStoragePath,
    config: &MediaProcessConfig,
) -> Result<(u32, u32), ImageDimensionError> {
    const INPUT_DESCRIPTOR: i32 = 10;

    let mut source = OsString::from(format!("/proc/self/fd/{INPUT_DESCRIPTOR}"));
    source.push("[0]");
    let mut arguments = image_magick_resource_arguments(config);
    arguments.extend([
        source,
        OsString::from("-auto-orient"),
        OsString::from("-format"),
        OsString::from("%w %h"),
        OsString::from("info:"),
    ]);
    let output = run_storage_media_tool(
        cpu,
        file_io,
        MediaTool::ImageMagick,
        arguments,
        128,
        config.maximum_stderr_bytes,
        vec![StorageChildDescriptor::Read {
            storage_root,
            path,
            child_fd: INPUT_DESCRIPTOR,
        }],
    )
    .await
    .map_err(ImageDimensionError::Process)?;
    validate_dimension_output(output, config.maximum_decoded_image_pixels)
}

fn validate_dimension_output(
    output: ExternalProcessOutput,
    maximum_pixels: u64,
) -> Result<(u32, u32), ImageDimensionError> {
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
    validate_image_geometry(width, height, maximum_pixels)
}

pub fn validate_image_geometry(
    width: u64,
    height: u64,
    maximum_pixels: u64,
) -> Result<(u32, u32), ImageDimensionError> {
    const MAX_IMAGE_DIMENSION: u64 = 262_144;
    if width == 0 || height == 0 {
        return Err(ImageDimensionError::InvalidOutput);
    }
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(ImageDimensionError::DimensionLimitExceeded {
            actual_dimension: width.max(height),
            maximum_dimension: MAX_IMAGE_DIMENSION,
        });
    }
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
    let width = u32::try_from(width).map_err(|_| ImageDimensionError::InvalidOutput)?;
    let height = u32::try_from(height).map_err(|_| ImageDimensionError::InvalidOutput)?;
    Ok((width, height))
}

pub fn os_arguments(arguments: &[&OsStr]) -> Vec<OsString> {
    arguments
        .iter()
        .map(|argument| (*argument).to_os_string())
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum ChildResourceLimit {
    AddressSpace,
    FileSize,
}

#[cfg(target_env = "gnu")]
type RlimitResource = libc::__rlimit_resource_t;
#[cfg(target_env = "musl")]
type RlimitResource = libc::c_int;

fn set_child_resource_limit(resource: ChildResourceLimit, bytes: u64) -> io::Result<()> {
    let resource: RlimitResource = match resource {
        ChildResourceLimit::AddressSpace => libc::RLIMIT_AS,
        ChildResourceLimit::FileSize => libc::RLIMIT_FSIZE,
    };
    let limit = libc::rlimit {
        rlim_cur: bytes,
        rlim_max: bytes,
    };
    if unsafe { libc::setrlimit(resource, &limit) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn mark_child_descriptors_close_on_exec() -> io::Result<()> {
    let result = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3_u32,
            u32::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(libc::ENOSYS) {
        return Err(error);
    }
    mark_child_descriptors_close_on_exec_fallback()
}

fn mark_child_descriptors_close_on_exec_fallback() -> io::Result<()> {
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let limit = unsafe { limit.assume_init() };
    let maximum = limit.rlim_cur.min(1_048_576) as i32;
    for descriptor in 3..maximum {
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
        if flags < 0 {
            continue;
        }
        if unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_launch_spec() -> ChildLaunchSpec {
        ChildLaunchSpec {
            descriptor_actions: Vec::new(),
            stdout_child_fd: None,
            address_space_limit_bytes: 64 * 1024 * 1024,
            file_size_limit_bytes: 1024 * 1024,
            start_timeout: Duration::from_secs(1),
        }
    }

    #[test]
    fn exec_handshake_closes_only_after_successful_exec() {
        let command = PreparedChildCommand::new(OsStr::new("true"), Vec::new(), &[])
            .expect("prepared true command");
        let mut child = spawn_child_with_deadline(&command, test_launch_spec())
            .expect("successful exec handshake");

        assert!(child.wait().expect("wait for true").success());
    }

    #[test]
    fn exec_handshake_returns_the_child_exec_error() {
        let command = PreparedChildCommand::new(
            OsStr::new("momento-command-that-does-not-exist"),
            Vec::new(),
            &[],
        )
        .expect("prepared missing command");
        let result = spawn_child_with_deadline(&command, test_launch_spec());

        assert!(matches!(
            result,
            Err(ChildStartError::Io(ref source)) if source.kind() == io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn exec_handshake_has_a_bounded_start_deadline() {
        let (read, _write) = create_cloexec_pipe().expect("exec status pipe");
        let started = Instant::now();
        let result = wait_for_exec_handshake(read.as_raw_fd(), Duration::from_millis(10));

        assert!(matches!(result, Err(ChildStartError::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
