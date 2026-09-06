use super::*;

#[test]
fn storage_workers_reserve_one_reader_one_writer_and_share_the_rest() {
    let (read_sender, read) = crossbeam_channel::bounded(4);
    let (write_sender, write) = crossbeam_channel::bounded(4);
    for index in 0..4 {
        let (worker_read, worker_write) = worker_receivers(index, &read, &write);
        read_sender.send("read").unwrap();
        write_sender.send("write").unwrap();
        if index == 1 {
            assert!(worker_read.try_recv().is_err());
            assert_eq!(read.try_recv().unwrap(), "read");
        } else {
            assert_eq!(worker_read.try_recv().unwrap(), "read");
        }
        if index == 0 {
            assert!(worker_write.try_recv().is_err());
            assert_eq!(write.try_recv().unwrap(), "write");
        } else {
            assert_eq!(worker_write.try_recv().unwrap(), "write");
        }
    }
}

#[test]
fn shared_storage_workers_alternate_backlogs_and_use_the_nonempty_lane() {
    let (read_sender, read) = crossbeam_channel::bounded(4);
    let (write_sender, write) = crossbeam_channel::bounded(4);
    for _ in 0..4 {
        read_sender.send(true).unwrap();
        write_sender.send(false).unwrap();
    }
    let mut prefer_read = true;
    for index in 0..8 {
        let is_read = try_preferred_command(&read, &write, prefer_read).unwrap();
        assert_eq!(is_read, index % 2 == 0);
        prefer_read = !is_read;
    }
    write_sender.send(false).unwrap();
    assert!(!try_preferred_command(&read, &write, true).unwrap());
    read_sender.send(true).unwrap();
    assert!(try_preferred_command(&read, &write, false).unwrap());
    assert!(try_preferred_command(&read, &write, false).is_err());
    drop(read_sender);
    drop(write_sender);
    assert_eq!(
        try_preferred_command(&read, &write, true).unwrap_err(),
        crossbeam_channel::TryRecvError::Disconnected
    );
}

#[test]
fn storage_classification_keeps_mutations_off_the_reader() {
    let path = NormalizedStoragePath::parse("test").unwrap();
    for (operation, read_only) in [
        (FileOperation::Probe { sequence: 0 }, true),
        (
            FileOperation::OpenStorageReadSession {
                storage_root: StorageRootId::Originals,
                path: path.clone(),
            },
            true,
        ),
        (
            FileOperation::OpenStorageDirectorySession {
                storage_root: StorageRootId::Originals,
                path: None,
            },
            true,
        ),
        (
            FileOperation::OpenStorageWriteSession {
                storage_root: StorageRootId::Previews,
                path: path.clone(),
                rollback_length: 0,
            },
            false,
        ),
        (
            FileOperation::CreateStorageDirectory {
                storage_root: StorageRootId::Previews,
                path: path.clone(),
            },
            false,
        ),
        (
            FileOperation::AtomicReplaceStorageFile {
                storage_root: StorageRootId::Previews,
                temporary_path: path.clone(),
                destination_path: path.clone(),
                contents: vec![],
            },
            false,
        ),
        (
            FileOperation::SetStorageModifiedTime {
                storage_root: StorageRootId::Previews,
                path,
                seconds: 0,
                nanoseconds: 0,
            },
            false,
        ),
    ] {
        assert_eq!(operation.is_read_only(), read_only, "{}", operation.name());
    }
}
