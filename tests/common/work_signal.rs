use momento_common::work_signal::WorkSignal;

#[tokio::test]
async fn notification_before_wait_is_observed() {
    let signal = WorkSignal::default();
    let observed = signal.version();

    signal.notify();

    assert_ne!(signal.wait_for_change(observed).await, observed);
}

#[tokio::test]
async fn waiter_is_released_by_notification() {
    let signal = WorkSignal::default();
    let observed = signal.version();
    let waiting_signal = signal.clone();
    let waiter = tokio::spawn(async move { waiting_signal.wait_for_change(observed).await });

    tokio::task::yield_now().await;
    signal.notify();

    assert_ne!(waiter.await.expect("work signal waiter"), observed);
}

#[tokio::test]
async fn multiple_notifications_advance_the_version() {
    let signal = WorkSignal::default();
    let observed = signal.version();

    signal.notify();
    signal.notify();

    assert_eq!(signal.wait_for_change(observed).await, observed + 2);
}
