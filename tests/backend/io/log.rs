use std::fs::File;
use std::io::Write;

use chrono::{Duration, NaiveDate};
use filetime::{set_file_times, FileTime};
use momento_api::io::log::{measure_retained_log_allocation, prune_oldest_closed_rotations_batch};

fn write_allocated_file(path: &std::path::Path, modified_seconds: i64) {
    let mut file = File::create(path).expect("create log fixture");
    file.write_all(&[7_u8; 4096]).expect("write log fixture");
    file.sync_all().expect("sync log fixture");
    let modified = FileTime::from_unix_time(modified_seconds, 0);
    set_file_times(path, modified, modified).expect("set log fixture time");
}

#[test]
fn log_cleanup_uses_bounded_oldest_batches_and_preserves_unowned_files() {
    let directory = tempfile::tempdir().expect("log directory");
    let first_date = NaiveDate::from_ymd_opt(2020, 1, 1).expect("first date");
    for offset in 0..300_i64 {
        let date = first_date
            .checked_add_signed(Duration::days(offset))
            .expect("log date");
        write_allocated_file(
            &directory.path().join(format!("momento-api.{date}.log")),
            offset + 100,
        );
    }
    let unowned = directory.path().join("llm-service.2020-01-01.log");
    write_allocated_file(&unowned, 1);
    let directory_handle = File::open(directory.path()).expect("open log directory");
    let initial = measure_retained_log_allocation(&directory_handle).expect("initial allocation");

    assert!(
        prune_oldest_closed_rotations_batch(&directory_handle, initial, 0)
            .expect("first cleanup batch")
    );
    assert!(!directory.path().join("momento-api.2020-01-01.log").exists());
    let first_retained_date = first_date
        .checked_add_signed(Duration::days(256))
        .expect("first retained date");
    assert!(directory
        .path()
        .join(format!("momento-api.{first_retained_date}.log"))
        .exists());
    assert!(unowned.exists());

    let after_first =
        measure_retained_log_allocation(&directory_handle).expect("allocation after first batch");
    assert!(after_first < initial);
    assert!(
        prune_oldest_closed_rotations_batch(&directory_handle, after_first, 0)
            .expect("second cleanup batch")
    );
    let retained = measure_retained_log_allocation(&directory_handle).expect("retained allocation");
    assert!(retained > 0, "the unowned log remains allocated");
    assert!(
        !prune_oldest_closed_rotations_batch(&directory_handle, retained, 0)
            .expect("no owned rotations remain")
    );
    assert!(unowned.exists());
}

#[cfg(unix)]
#[test]
fn log_inventory_rejects_symlink_entries() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().expect("log directory");
    let target = directory.path().join("outside.log");
    write_allocated_file(&target, 1);
    symlink(&target, directory.path().join("momento-api.2020-01-01.log")).expect("log symlink");
    let directory_handle = File::open(directory.path()).expect("open log directory");
    assert!(measure_retained_log_allocation(&directory_handle).is_err());
}
