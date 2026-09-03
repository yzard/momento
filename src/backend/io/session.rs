use std::ffi::CStr;
use std::fs::File;
use std::os::fd::{AsRawFd, RawFd};
use std::sync::{Arc, Mutex};

use crossbeam_channel::Sender;

const AT_STATX_DONT_SYNC: libc::c_int = 0x4000;
const STATX_TYPE: u32 = 0x0001;
const STATX_MODE: u32 = 0x0002;
const STATX_NLINK: u32 = 0x0004;
const STATX_UID: u32 = 0x0008;
const STATX_GID: u32 = 0x0010;
const STATX_ATIME: u32 = 0x0020;
const STATX_MTIME: u32 = 0x0040;
const STATX_CTIME: u32 = 0x0080;
const STATX_INO: u32 = 0x0100;
const STATX_SIZE: u32 = 0x0200;
const STATX_BLOCKS: u32 = 0x0400;
const STATX_BASIC_STATS: u32 = STATX_TYPE
    | STATX_MODE
    | STATX_NLINK
    | STATX_UID
    | STATX_GID
    | STATX_ATIME
    | STATX_MTIME
    | STATX_CTIME
    | STATX_INO
    | STATX_SIZE
    | STATX_BLOCKS;
const STATX_MNT_ID: u32 = 0x1000;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct LinuxStatxTimestamp {
    seconds: i64,
    nanoseconds: u32,
    reserved: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct LinuxDescriptorStatx {
    pub mask: u32,
    block_size: u32,
    attributes: u64,
    link_count: u32,
    user_id: u32,
    group_id: u32,
    pub mode: u16,
    spare_zero: u16,
    pub inode: u64,
    pub size: u64,
    blocks: u64,
    attributes_mask: u64,
    accessed_at: LinuxStatxTimestamp,
    created_at: LinuxStatxTimestamp,
    changed_at: LinuxStatxTimestamp,
    modified_at: LinuxStatxTimestamp,
    device_special_major: u32,
    device_special_minor: u32,
    pub device_major: u32,
    pub device_minor: u32,
    pub mount_id: u64,
    direct_io_memory_alignment: u32,
    direct_io_offset_alignment: u32,
    spare: [u64; 12],
}

const _: [(); 256] = [(); std::mem::size_of::<LinuxDescriptorStatx>()];

impl LinuxDescriptorStatx {
    pub(crate) fn has_mount_id(&self) -> bool {
        self.mask & STATX_MNT_ID != 0
    }
}

pub(crate) struct RegisteredFile {
    pub file: File,
    pub rollback_length: Option<u64>,
    pub child_access: Option<ChildDescriptorAccess>,
}

struct FileHandleSlot {
    generation: u64,
    file: Option<RegisteredFile>,
    in_flight: bool,
    child_pinned: bool,
    close_requested: bool,
}

pub(crate) struct FileHandleRegistry {
    slots: Mutex<Vec<FileHandleSlot>>,
    close_wake: Sender<()>,
}

pub struct StorageFileSession {
    registry: Arc<FileHandleRegistry>,
    slot: usize,
    generation: u64,
    armed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildDescriptorAccess {
    Read,
    Write,
}

pub(crate) struct ChildDescriptorLease {
    registry: Arc<FileHandleRegistry>,
    slot: usize,
    generation: u64,
    raw_fd: RawFd,
    child_fd: RawFd,
    access: ChildDescriptorAccess,
    armed: bool,
}

impl std::fmt::Debug for StorageFileSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StorageFileSession")
            .field("slot", &self.slot)
            .field("generation", &self.generation)
            .field("armed", &self.armed)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageFileSnapshot {
    pub byte_size: u64,
    pub device_major: u32,
    pub device_minor: u32,
    pub mount_id: u64,
    pub inode: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: u32,
    pub changed_seconds: i64,
    pub changed_nanoseconds: u32,
}

impl StorageFileSnapshot {
    pub fn identity_version(&self) -> String {
        format!(
            "v1:{:08x}:{:08x}:{:016x}:{:016x}:{:016x}:{:016x}:{:08x}",
            self.device_major,
            self.device_minor,
            self.mount_id,
            self.inode,
            self.byte_size,
            self.modified_seconds,
            self.modified_nanoseconds,
        )
    }
}

pub(crate) struct FileHandleUse {
    registry: Arc<FileHandleRegistry>,
    slot: usize,
    generation: u64,
    file: Option<RegisteredFile>,
}

impl FileHandleRegistry {
    pub(crate) fn new(capacity: usize, close_wake: Sender<()>) -> std::io::Result<Self> {
        if capacity == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "file handle registry capacity must be positive",
            ));
        }
        let mut slots = Vec::new();
        slots.try_reserve_exact(capacity).map_err(|error| {
            std::io::Error::other(format!("failed to reserve file handle registry: {error}"))
        })?;
        for _ in 0..capacity {
            slots.push(FileHandleSlot {
                generation: 0,
                file: None,
                in_flight: false,
                child_pinned: false,
                close_requested: false,
            });
        }
        Ok(Self {
            slots: Mutex::new(slots),
            close_wake,
        })
    }

    pub(crate) fn register(
        self: &Arc<Self>,
        file: RegisteredFile,
    ) -> std::io::Result<StorageFileSession> {
        self.sweep_close_requests();
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| std::io::Error::other("file handle registry is poisoned"))?;
        let (slot_index, slot) = slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| {
                slot.file.is_none()
                    && !slot.in_flight
                    && !slot.child_pinned
                    && !slot.close_requested
            })
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "file handle registry capacity is exhausted",
                )
            })?;
        slot.generation = slot
            .generation
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("file handle registry generation is exhausted"))?;
        slot.file = Some(file);
        Ok(StorageFileSession {
            registry: Arc::clone(self),
            slot: slot_index,
            generation: slot.generation,
            armed: true,
        })
    }

    pub(crate) fn begin(
        self: &Arc<Self>,
        mut token: StorageFileSession,
    ) -> std::io::Result<FileHandleUse> {
        token.armed = false;
        let slot_index = token.slot;
        let generation = token.generation;
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| std::io::Error::other("file handle registry is poisoned"))?;
        let slot = slots.get_mut(slot_index).ok_or_else(invalid_session)?;
        if slot.generation != generation
            || slot.in_flight
            || slot.child_pinned
            || slot.close_requested
            || slot.file.is_none()
        {
            return Err(invalid_session());
        }
        let file = slot.file.take().ok_or_else(invalid_session)?;
        slot.in_flight = true;
        drop(slots);
        Ok(FileHandleUse {
            registry: Arc::clone(self),
            slot: slot_index,
            generation,
            file: Some(file),
        })
    }

    pub(crate) fn sweep_close_requests(&self) {
        let mut closing = Vec::new();
        let Ok(mut slots) = self.slots.lock() else {
            return;
        };
        for slot in slots.iter_mut() {
            if slot.close_requested && !slot.in_flight && !slot.child_pinned {
                if let Some(file) = slot.file.take() {
                    closing.push(file);
                }
                slot.close_requested = false;
            }
        }
        drop(slots);
        for file in closing {
            let _ = rollback_registered_file(&file);
        }
    }

    fn request_close(&self, slot_index: usize, generation: u64) {
        let Ok(mut slots) = self.slots.lock() else {
            return;
        };
        let Some(slot) = slots.get_mut(slot_index) else {
            return;
        };
        if slot.generation != generation || (slot.file.is_none() && !slot.in_flight) {
            return;
        }
        slot.close_requested = true;
        drop(slots);
        let _ = self.close_wake.try_send(());
    }

    pub(crate) fn pin_for_child(
        self: &Arc<Self>,
        mut token: StorageFileSession,
        child_fd: RawFd,
        access: ChildDescriptorAccess,
    ) -> std::io::Result<ChildDescriptorLease> {
        if child_fd < 3 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "child descriptor must not replace standard input, output, or error",
            ));
        }
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| std::io::Error::other("file handle registry is poisoned"))?;
        let slot = slots.get_mut(token.slot).ok_or_else(invalid_session)?;
        if slot.generation != token.generation
            || slot.in_flight
            || slot.child_pinned
            || slot.close_requested
        {
            return Err(invalid_session());
        }
        let registered = slot.file.as_ref().ok_or_else(invalid_session)?;
        if registered.child_access != Some(access) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "storage session access does not match the requested child descriptor access",
            ));
        }
        let raw_fd = registered.file.as_raw_fd();
        slot.child_pinned = true;
        token.armed = false;
        Ok(ChildDescriptorLease {
            registry: Arc::clone(self),
            slot: token.slot,
            generation: token.generation,
            raw_fd,
            child_fd,
            access,
            armed: true,
        })
    }

    pub(crate) fn return_from_child(
        self: &Arc<Self>,
        mut lease: ChildDescriptorLease,
    ) -> std::io::Result<StorageFileSession> {
        if !Arc::ptr_eq(self, &lease.registry) {
            return Err(invalid_session());
        }
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| std::io::Error::other("file handle registry is poisoned"))?;
        let slot = slots.get_mut(lease.slot).ok_or_else(invalid_session)?;
        if slot.generation != lease.generation
            || slot.in_flight
            || !slot.child_pinned
            || slot.file.is_none()
        {
            return Err(invalid_session());
        }
        slot.child_pinned = false;
        lease.armed = false;
        let close_requested = slot.close_requested;
        drop(slots);
        if close_requested {
            self.sweep_close_requests();
            return Err(invalid_session());
        }
        Ok(StorageFileSession {
            registry: Arc::clone(self),
            slot: lease.slot,
            generation: lease.generation,
            armed: true,
        })
    }
}

impl StorageFileSession {
    pub(crate) fn belongs_to(&self, registry: &Arc<FileHandleRegistry>) -> bool {
        Arc::ptr_eq(&self.registry, registry)
    }
}

impl ChildDescriptorLease {
    pub(crate) fn raw_fd(&self) -> RawFd {
        self.raw_fd
    }

    pub(crate) fn child_fd(&self) -> RawFd {
        self.child_fd
    }

    pub(crate) fn access(&self) -> ChildDescriptorAccess {
        self.access
    }
}

impl Drop for ChildDescriptorLease {
    fn drop(&mut self) {
        if self.armed {
            self.registry.request_close(self.slot, self.generation);
        }
    }
}

pub(crate) fn snapshot_regular_file(file: &File) -> std::io::Result<StorageFileSnapshot> {
    let status = descriptor_statx(file)?;
    let required = STATX_TYPE | STATX_SIZE | STATX_INO | STATX_MTIME | STATX_CTIME | STATX_MNT_ID;
    if status.mask & required != required || u32::from(status.mode) & libc::S_IFMT != libc::S_IFREG
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "opened storage descriptor is not a snapshot-capable regular file",
        ));
    }
    Ok(StorageFileSnapshot {
        byte_size: status.size,
        device_major: status.device_major,
        device_minor: status.device_minor,
        mount_id: status.mount_id,
        inode: status.inode,
        modified_seconds: status.modified_at.seconds,
        modified_nanoseconds: status.modified_at.nanoseconds,
        changed_seconds: status.changed_at.seconds,
        changed_nanoseconds: status.changed_at.nanoseconds,
    })
}

pub(crate) fn descriptor_statx(file: &File) -> std::io::Result<LinuxDescriptorStatx> {
    let mut status = std::mem::MaybeUninit::<LinuxDescriptorStatx>::zeroed();
    // SAFETY: Linux defines `statx` as a stable 256-byte UAPI structure. The layout above mirrors
    // that structure, the descriptor and empty path remain valid for the call, and the kernel writes
    // only within the provided initialized allocation.
    let result = unsafe {
        libc::syscall(
            libc::SYS_statx,
            file.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH | AT_STATX_DONT_SYNC,
            STATX_BASIC_STATS | STATX_MNT_ID,
            status.as_mut_ptr(),
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: a successful `statx` call initialized the complete output structure.
    Ok(unsafe { status.assume_init() })
}

pub(crate) fn rename_descriptor_entry(
    source_directory_fd: RawFd,
    source_name: &CStr,
    destination_directory_fd: RawFd,
    destination_name: &CStr,
    flags: libc::c_uint,
) -> std::io::Result<()> {
    // SAFETY: both names are valid C strings for the duration of the call. Linux copies them before
    // returning, and the directory descriptors are borrowed from live `File` owners.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            source_directory_fd,
            source_name.as_ptr(),
            destination_directory_fd,
            destination_name.as_ptr(),
            flags,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

impl Drop for StorageFileSession {
    fn drop(&mut self) {
        if self.armed {
            self.registry.request_close(self.slot, self.generation);
        }
    }
}

impl FileHandleUse {
    pub(crate) fn file_mut(&mut self) -> std::io::Result<&mut File> {
        self.file
            .as_mut()
            .map(|file| &mut file.file)
            .ok_or_else(invalid_session)
    }

    pub(crate) fn finish(mut self) -> std::io::Result<StorageFileSession> {
        let mut slots = self
            .registry
            .slots
            .lock()
            .map_err(|_| std::io::Error::other("file handle registry is poisoned"))?;
        let slot = slots.get_mut(self.slot).ok_or_else(invalid_session)?;
        if slot.generation != self.generation
            || !slot.in_flight
            || slot.child_pinned
            || slot.file.is_some()
        {
            return Err(invalid_session());
        }
        let file = self.file.take().ok_or_else(invalid_session)?;
        slot.in_flight = false;
        slot.file = Some(file);
        let close_requested = slot.close_requested;
        drop(slots);
        if close_requested {
            self.registry.sweep_close_requests();
            return Err(invalid_session());
        }
        Ok(StorageFileSession {
            registry: Arc::clone(&self.registry),
            slot: self.slot,
            generation: self.generation,
            armed: true,
        })
    }

    pub(crate) fn commit(mut self) -> std::io::Result<()> {
        let file = self.file.take().ok_or_else(invalid_session)?;
        if let Err(error) = sync_file_data(&file.file) {
            let rollback = rollback_registered_file(&file);
            let release = self.release_slot();
            drop(file);
            return Err(combine_cleanup_errors(error, rollback, release));
        }
        if let Err(error) = self.release_slot() {
            let rollback = rollback_registered_file(&file);
            drop(file);
            return Err(combine_rollback_error(error, rollback));
        }
        drop(file);
        Ok(())
    }

    pub(crate) fn abort(mut self) -> std::io::Result<()> {
        let file = self.file.take().ok_or_else(invalid_session)?;
        let rollback_result = rollback_registered_file(&file);
        self.release_slot()?;
        drop(file);
        rollback_result
    }

    pub(crate) fn close(mut self) -> std::io::Result<()> {
        let file = self.file.take().ok_or_else(invalid_session)?;
        self.release_slot()?;
        drop(file);
        Ok(())
    }

    fn release_slot(&self) -> std::io::Result<()> {
        let mut slots = self
            .registry
            .slots
            .lock()
            .map_err(|_| std::io::Error::other("file handle registry is poisoned"))?;
        let slot = slots.get_mut(self.slot).ok_or_else(invalid_session)?;
        if slot.generation != self.generation
            || !slot.in_flight
            || slot.child_pinned
            || slot.file.is_some()
        {
            return Err(invalid_session());
        }
        slot.in_flight = false;
        slot.close_requested = false;
        Ok(())
    }
}

impl Drop for FileHandleUse {
    fn drop(&mut self) {
        let Some(file) = self.file.take() else {
            return;
        };
        let _ = rollback_registered_file(&file);
        if let Ok(mut slots) = self.registry.slots.lock() {
            if let Some(slot) = slots.get_mut(self.slot) {
                if slot.generation == self.generation {
                    slot.in_flight = false;
                    slot.close_requested = false;
                }
            }
        }
    }
}

fn rollback_registered_file(file: &RegisteredFile) -> std::io::Result<()> {
    let Some(length) = file.rollback_length else {
        return Ok(());
    };
    let length = i64::try_from(length).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "file rollback offset exceeds the platform limit",
        )
    })?;
    if unsafe { libc::ftruncate(file.file.as_raw_fd(), length) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    sync_file_data(&file.file)
}

fn sync_file_data(file: &File) -> std::io::Result<()> {
    if unsafe { libc::fdatasync(file.as_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn invalid_session() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "file session token is stale or already in use",
    )
}

fn combine_rollback_error(
    primary: std::io::Error,
    rollback: std::io::Result<()>,
) -> std::io::Error {
    match rollback {
        Ok(()) => primary,
        Err(rollback) => std::io::Error::new(
            primary.kind(),
            format!("{primary}; rollback also failed: {rollback}"),
        ),
    }
}

fn combine_cleanup_errors(
    primary: std::io::Error,
    rollback: std::io::Result<()>,
    release: std::io::Result<()>,
) -> std::io::Error {
    let error = combine_rollback_error(primary, rollback);
    match release {
        Ok(()) => error,
        Err(release) => std::io::Error::new(
            error.kind(),
            format!("{error}; registry release also failed: {release}"),
        ),
    }
}
