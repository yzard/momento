use super::*;

#[tokio::test]
async fn result_consumer_refills_on_arrival_and_completion_while_first_result_is_slow() {
    use crate::processor::ai::operation::AiFeature;
    let tasks = AiFeature::ALL.map(AiFeature::inference_task);
    for task in tasks {
        assert_result_consumer_refills(task, vec![task; 4]).await;
    }
    // A slow face result must not hold back OCR, clustering or other result types.
    assert_result_consumer_refills("face_detection", tasks.to_vec()).await;
}

async fn assert_result_consumer_refills(slow_task: &'static str, later_tasks: Vec<&'static str>) {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    let queue = RefCell::new(VecDeque::from([(0, slow_task)]));
    let active = Cell::new(0);
    let peak = Cell::new(0);
    let completed = Cell::new(0);
    let finished_tasks = RefCell::new(Vec::new());
    let first_started = tokio::sync::Notify::new();
    let progress = tokio::sync::Notify::new();
    let release_first = tokio::sync::Notify::new();
    let (version, changed) = tokio::sync::watch::channel(0_u64);
    let consumer = drain_result_queue(
        3,
        0,
        |limit| {
            let mut queue = queue.borrow_mut();
            let count = limit.min(queue.len());
            std::future::ready(Ok(queue.drain(..count).collect()))
        },
        |(id, task)| {
            active.set(active.get() + 1);
            peak.set(peak.get().max(active.get()));
            let (active, completed, first_started, progress, release_first) = (
                &active,
                &completed,
                &first_started,
                &progress,
                &release_first,
            );
            let finished_tasks = &finished_tasks;
            async move {
                if id == 0 {
                    first_started.notify_one();
                    release_first.notified().await;
                }
                active.set(active.get() - 1);
                completed.set(completed.get() + 1);
                finished_tasks.borrow_mut().push(task);
                progress.notify_one();
                Ok(1)
            }
        },
        |observed| {
            let mut changed = changed.clone();
            async move { *changed.wait_for(|value| *value != observed).await.unwrap() }
        },
    );
    let arrivals = async {
        first_started.notified().await;
        queue.borrow_mut().extend(
            later_tasks
                .iter()
                .copied()
                .enumerate()
                .map(|(id, task)| (id + 1, task)),
        );
        version.send(1).unwrap();
        // All later results must finish without waiting for the original result.
        while completed.get() < later_tasks.len() {
            progress.notified().await;
        }
        assert_eq!(active.get(), 1);
        assert_eq!(peak.get(), 3);
        release_first.notify_one();
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(consumer, arrivals)
    })
    .await
    .expect("result consumer failed to refill vacant slots");
    assert_eq!(result.unwrap(), later_tasks.len() + 1);
    assert_eq!(active.get(), 0);
    assert!(queue.borrow().is_empty());
    let mut expected = later_tasks;
    expected.push(slow_task);
    expected.sort_unstable();
    finished_tasks.borrow_mut().sort_unstable();
    assert_eq!(*finished_tasks.borrow(), expected);
}

#[tokio::test]
async fn result_consumer_drains_claimed_work_before_returning_an_error() {
    use std::cell::Cell;
    for fail_claim in [false, true] {
        let claims = Cell::new(0);
        let finished = Cell::new(0);
        let result = drain_result_queue(
            3,
            0,
            |_| {
                claims.set(claims.get() + 1);
                std::future::ready(if claims.get() == 1 {
                    Ok(if fail_claim { vec![1] } else { vec![0, 1, 2] })
                } else {
                    Err(AppError::DatabaseBusy)
                })
            },
            |id| {
                let finished = &finished;
                async move {
                    if id == 0 {
                        return Err(AppError::DatabaseBusy);
                    }
                    tokio::task::yield_now().await;
                    finished.set(finished.get() + 1);
                    Ok(1)
                }
            },
            |observed| async move {
                if fail_claim && observed == 0 {
                    return 1;
                }
                std::future::pending::<u64>().await
            },
        )
        .await;
        assert!(matches!(result, Err(AppError::DatabaseBusy)));
        assert_eq!(finished.get(), if fail_claim { 1 } else { 2 });
        assert_eq!(claims.get(), if fail_claim { 2 } else { 1 });
    }
}

#[tokio::test]
async fn empty_result_queue_is_checked_once_without_starting_lanes() {
    let mut claims = 0;
    let result = drain_result_queue(
        256,
        0,
        |_| {
            claims += 1;
            std::future::ready(Ok(Vec::<()>::new()))
        },
        |_| async { panic!("empty queue must not start a lane") },
        |_| std::future::pending::<u64>(),
    )
    .await;
    assert_eq!(result.unwrap(), 0);
    assert_eq!(claims, 1);
}

#[test]
fn staging_detaches_only_bounded_non_face_results() {
    for task in [
        "ocr",
        "image_tagging",
        "image_aesthetics",
        "image_clustering",
        "screenshot_detection",
        "document_detection",
    ] {
        assert!(can_detach_result_staging(task, 8192, 32));
        assert!(!can_detach_result_staging(task, 8193, 32));
        assert!(!can_detach_result_staging(task, 8192, 33));
    }
    assert!(!can_detach_result_staging("face_detection", 24, 1));
}
