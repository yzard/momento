use super::*;

// These tests exercise the same tracing callsites with different thread-local
// subscribers. Keep their first-use registration out of competing test threads.
static MEMORY_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Default)]
struct LogBuffer(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn quota_logs_distinguish_impossible_requests_from_waiting_and_resumption() {
    let _test_guard = MEMORY_TEST_LOCK.lock().await;
    use tracing::instrument::WithSubscriber;
    let buffer = LogBuffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    async {
        let budget = MemoryBudget::new(100);
        let held = budget.acquire(100).await.unwrap();
        assert!(budget.acquire(101).await.is_err());
        let mut waiting = Box::pin(budget.acquire(90));
        assert!(futures::poll!(&mut waiting).is_pending());
        let mut fifo_waiting = Box::pin(budget.acquire(10));
        assert!(futures::poll!(&mut fifo_waiting).is_pending());
        drop(held);
        let first = waiting.await.unwrap();
        let second = fifo_waiting.await.unwrap();
        assert_eq!(budget.used.load(Ordering::Acquire), 100);
        drop((first, second));
    }
    .with_subscriber(subscriber)
    .await;
    let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    for field in [
        "magick_memory_quota_exceeded",
        "retryable=false",
        "requested_bytes=101",
        "quota_bytes=100",
        "magick_memory_quota_busy",
        "capacity_in_use",
        "fifo_wait",
        "used_bytes=100",
        "magick_memory_quota_acquired",
        "waited_ms=",
    ] {
        assert!(log.contains(field), "missing {field}: {log}");
    }
    assert_eq!(log.matches("magick_memory_quota_busy").count(), 2);
    assert_eq!(log.matches("magick_memory_quota_acquired").count(), 2);
}

#[tokio::test]
async fn shared_budget_is_bounded_fifo_and_cancellation_safe() {
    let _test_guard = MEMORY_TEST_LOCK.lock().await;
    let budget = MemoryBudget::new(100);
    let held = budget.acquire(90).await.unwrap();
    let mut first = Box::pin(budget.acquire(30));
    let mut second = Box::pin(budget.acquire(10));
    assert!(futures::poll!(&mut first).is_pending());
    assert!(futures::poll!(&mut second).is_pending());
    drop(first);
    let second = second.await.unwrap();
    assert_eq!(budget.used.load(Ordering::Acquire), 100);
    drop(held);
    drop(second);
    assert_eq!(budget.used.load(Ordering::Acquire), 0);
    assert!(budget.acquire(101).await.is_err());
    assert!(budget.acquire(0).await.is_err());
}

#[tokio::test]
async fn releasing_a_running_reservation_wakes_waiting_work() {
    let _test_guard = MEMORY_TEST_LOCK.lock().await;
    let budget = MemoryBudget::new(100);
    let held = budget.acquire(100).await.unwrap();
    let mut waiting = Box::pin(budget.acquire(100));
    assert!(futures::poll!(&mut waiting).is_pending());
    drop(held);
    let next = tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(budget.used.load(Ordering::Acquire), 100);
    drop(next);
    assert_eq!(budget.used.load(Ordering::Acquire), 0);
}
