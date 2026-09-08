use super::refill_while_draining;
use futures::stream::FuturesUnordered;

#[tokio::test]
async fn refill_keeps_polling_the_submission_it_is_waiting_for() {
    let (released, wait) = tokio::sync::oneshot::channel();
    let mut submissions = FuturesUnordered::new();
    submissions.push(async move {
        tokio::task::yield_now().await;
        released.send(()).unwrap();
        Ok(())
    });
    let refill = async move {
        wait.await.unwrap();
        Ok(vec![1])
    };
    let mut error = None;
    let jobs = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        refill_while_draining(refill, &mut submissions, &mut error),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(jobs, vec![1]);
    assert!(error.is_none());
}

#[tokio::test]
async fn refill_preserves_submission_failure_and_finishes_the_claim() {
    let (released, wait) = tokio::sync::oneshot::channel();
    let mut submissions = FuturesUnordered::new();
    submissions.push(async move {
        released.send(()).unwrap();
        Err("submission failed".to_string())
    });
    let mut error = None;
    let jobs = refill_while_draining(
        async move {
            wait.await.unwrap();
            Ok(vec![2])
        },
        &mut submissions,
        &mut error,
    )
    .await
    .unwrap();
    assert_eq!(jobs, vec![2]);
    assert_eq!(error.as_deref(), Some("submission failed"));
}

#[tokio::test]
async fn empty_window_propagates_refill_failure() {
    let mut submissions = FuturesUnordered::<std::future::Ready<Result<(), String>>>::new();
    let mut error = None;
    let result = refill_while_draining(
        async { Err::<(), _>("claim failed".to_string()) },
        &mut submissions,
        &mut error,
    )
    .await;
    assert_eq!(result.unwrap_err(), "claim failed");
}
