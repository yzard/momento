//! Explicit repository-owned temporary storage for tests, independent of TMPDIR.
use std::path::PathBuf;
use std::sync::OnceLock;

static PROCESS_DIRECTORY: OnceLock<tempfile::TempDir> = OnceLock::new();

extern "C" {
    fn atexit(callback: extern "C" fn()) -> std::ffi::c_int;
}

extern "C" fn cleanup_process_directory() {
    let Some(directory) = PROCESS_DIRECTORY.get() else {
        return;
    };
    if let Err(error) = std::fs::remove_dir_all(directory.path()) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!(
                "Could not clean test temporary directory {}: {error}",
                directory.path().display()
            );
        }
    }
}

pub fn root() -> PathBuf {
    PROCESS_DIRECTORY
        .get_or_init(|| {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/tmp");
            std::fs::create_dir_all(&root).expect("create repository test temporary root");
            let root = root
                .canonicalize()
                .expect("resolve repository test temporary root");
            let directory = tempfile::Builder::new()
                .prefix("rust-tests-")
                .tempdir_in(root)
                .expect("create process-owned test temporary directory");
            // Rust statics are not dropped at exit. The C exit handler also runs when
            // libtest exits unsuccessfully, reclaiming fixtures retained by statics.
            // SAFETY: the callback has C ABI, takes no arguments and lives for the process.
            assert_eq!(
                unsafe { atexit(cleanup_process_directory) },
                0,
                "register test cleanup"
            );
            directory
        })
        .path()
        .to_owned()
}

pub fn tempdir() -> std::io::Result<tempfile::TempDir> {
    tempfile::tempdir_in(root())
}

pub fn tempfile() -> std::io::Result<tempfile::NamedTempFile> {
    tempfile::NamedTempFile::new_in(root())
}

#[cfg(test)]
#[path = "../../tests/test_support/temporary.rs"]
mod tests;
