use super::*;
use std::time::Duration;

fn ingress(capacity: usize) -> (SchedulerIngress, mpsc::Receiver<SchedulerCommand>) {
    let (sender, receiver) = mpsc::channel(capacity);
    (
        SchedulerIngress {
            sender,
            pending: Arc::new(AtomicUsize::new(0)),
            maximum_pending: capacity,
            changed: Arc::new(Notify::new()),
        },
        receiver,
    )
}

#[tokio::test]
async fn sqlite_dispatch_uses_operation_semantics_not_load_names() {
    let (ingress, mut receiver) = ingress(2);
    let sqlite = SqliteExecutorHandle::new(ingress);
    for read_only in [true, false] {
        let operation = async {
            if read_only {
                sqlite.probe_durable(1).await.map(|_| ())
            } else {
                // This "load" also claims/advances durable work and must write.
                sqlite
                    .load_deduplicate_finalization_work()
                    .await
                    .map(|_| ())
            }
        };
        tokio::pin!(operation);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut operation)
                .await
                .is_err()
        );
        let SchedulerCommand::Sqlite { command, .. } = receiver.recv().await.unwrap() else {
            panic!("SQLite command");
        };
        assert_eq!(command.is_read_only(), read_only);
        command.reject(ExecutorError::shutting_down("test"));
        assert_eq!(
            operation.await.unwrap_err().kind,
            crate::executor::ExecutorErrorKind::ShuttingDown
        );
    }
}

#[tokio::test]
async fn request_ingress_waits_without_exceeding_bound_and_cancels_cleanly() {
    let (ingress, _receiver) = ingress(1);
    let reservation = ingress.reserve("held").unwrap();
    assert!(tokio::time::timeout(
        Duration::from_millis(20),
        ingress.reserve_for(SubmissionMode::Request, "cancelled")
    )
    .await
    .is_err());
    assert_eq!(ingress.pending.load(Ordering::Acquire), 1);
    let waiting = ingress.reserve_for(SubmissionMode::Request, "request");
    tokio::pin!(waiting);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut waiting)
            .await
            .is_err()
    );
    drop(reservation);
    let acquired = tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ingress.pending.load(Ordering::Acquire), 1);
    drop(acquired);
    assert_eq!(ingress.pending.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn closing_ingress_wakes_capacity_waiters() {
    let (ingress, receiver) = ingress(1);
    let _reservation = ingress.reserve("held").unwrap();
    let waiting = ingress.reserve_for(SubmissionMode::Request, "request");
    tokio::pin!(waiting);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut waiting)
            .await
            .is_err()
    );
    drop(receiver);
    let error = tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .unwrap()
        .err()
        .unwrap();
    assert_eq!(error.kind, crate::executor::ExecutorErrorKind::ShuttingDown);
}

#[tokio::test]
async fn full_cpu_fifo_keeps_request_pending_until_capacity_returns() {
    let (ingress, mut receiver) = ingress(2);
    let cpu = CpuExecutorHandle::new(ingress, 2, 4 * 1024 * 1024 * 1024);
    let occupying = cpu.probe_durable(1);
    tokio::pin!(occupying);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut occupying)
            .await
            .is_err()
    );
    let SchedulerCommand::Cpu {
        command: occupying_command,
        reservation: occupying_reservation,
        ..
    } = receiver.recv().await.unwrap()
    else {
        panic!("CPU command");
    };
    let (sender, worker) = crossbeam_channel::bounded(1);
    assert!(sender.try_send(occupying_command).is_ok());
    drop(occupying_reservation);
    let operation = cpu.serialize_control_response(crate::executor::ControlResponse::from(
        crate::executor::MessageResponse {
            message: "ready".to_string(),
        },
    ));
    tokio::pin!(operation);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut operation)
            .await
            .is_err()
    );
    let SchedulerCommand::Cpu {
        command,
        mode,
        operation: name,
        reservation,
    } = receiver.recv().await.unwrap()
    else {
        panic!("CPU command");
    };
    let mut waiters = VecDeque::new();
    submit_cpu(command, mode, name, reservation, &sender, &mut waiters);
    assert_eq!(waiters.len(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut operation)
            .await
            .is_err()
    );
    drop(worker.try_recv().unwrap());
    flush_cpu(&sender, &mut waiters);
    assert!(waiters.is_empty());
    // The waiting request now owns the freed FIFO slot, rather than receiving
    // Overloaded. End the synthetic worker without executing business code.
    worker
        .try_recv()
        .unwrap()
        .reject(ExecutorError::shutting_down("test"));
    assert_eq!(
        operation.await.unwrap_err().kind,
        crate::executor::ExecutorErrorKind::ShuttingDown
    );
}
