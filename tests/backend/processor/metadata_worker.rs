use super::drain_metadata_window;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

#[test]
fn rollback_wait_logs_transitions_not_every_recovery_wakeup() {
    let mut previous = 0;
    assert!(!super::rollback_wait_changed(&mut previous, 0));
    assert!(super::rollback_wait_changed(&mut previous, 2));
    assert!(!super::rollback_wait_changed(&mut previous, 2));
    assert!(super::rollback_wait_changed(&mut previous, 1));
    assert!(super::rollback_wait_changed(&mut previous, 0));
    assert!(!super::rollback_wait_changed(&mut previous, 0));
}

#[tokio::test]
async fn work_notification_refills_idle_lanes_before_slow_job_finishes() {
    let available = AtomicUsize::new(1);
    let started = AtomicUsize::new(0);
    let finished = AtomicUsize::new(0);
    let wake = Notify::new();
    let wake = &wake;
    let release = Notify::new();
    let window = drain_metadata_window(
        3,
        0,
        || async {
            if available
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_err()
            {
                return Ok(false);
            }
            started.fetch_add(1, Ordering::SeqCst);
            release.notified().await;
            finished.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        },
        |version| async move {
            wake.notified().await;
            version + 1
        },
    );
    tokio::pin!(window);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut window)
            .await
            .is_err()
    );
    assert_eq!(started.load(Ordering::SeqCst), 1);
    available.store(5, Ordering::SeqCst);
    wake.notify_one();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut window)
            .await
            .is_err()
    );
    assert_eq!(started.load(Ordering::SeqCst), 3);
    assert_eq!(finished.load(Ordering::SeqCst), 0);
    // One completion refills immediately even while the other two are slow.
    release.notify_one();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut window)
            .await
            .is_err()
    );
    assert_eq!(started.load(Ordering::SeqCst), 4);
    assert_eq!(finished.load(Ordering::SeqCst), 1);
    available.store(0, Ordering::SeqCst);
    release.notify_waiters();
    window.await.unwrap();
    assert_eq!(finished.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn window_drains_active_work_before_returning_an_error() {
    let calls = AtomicUsize::new(0);
    let settled = AtomicUsize::new(0);
    let release = Notify::new();
    let window = drain_metadata_window(
        2,
        0,
        || async {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err("claim failed".to_string());
            }
            release.notified().await;
            settled.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        },
        |_| std::future::pending(),
    );
    tokio::pin!(window);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut window)
            .await
            .is_err()
    );
    release.notify_one();
    assert_eq!(window.await.unwrap_err(), "claim failed");
    assert_eq!(settled.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
