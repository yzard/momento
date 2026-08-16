use std::collections::VecDeque;
use std::convert::Infallible;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use momento_common::rolling::{run_rolling_window, RollingWindowControl};

#[tokio::test]
async fn refills_a_slot_before_the_slowest_initial_job_finishes() {
    let pending = Arc::new(Mutex::new(VecDeque::from([
        (1, 10_u64),
        (2, 100_u64),
        (3, 10_u64),
    ])));
    let events = Arc::new(Mutex::new(Vec::new()));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));

    let completed = run_rolling_window(
        NonZeroUsize::new(2).expect("non-zero window"),
        {
            let pending = Arc::clone(&pending);
            move |capacity| {
                let mut pending = pending.lock().expect("pending jobs");
                Ok::<_, Infallible>((0..capacity).filter_map(|_| pending.pop_front()).collect())
            }
        },
        {
            let events = Arc::clone(&events);
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            move |(job, delay_ms)| {
                let events = Arc::clone(&events);
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                async move {
                    let current = active.fetch_add(1, Ordering::Relaxed) + 1;
                    peak.fetch_max(current, Ordering::Relaxed);
                    events.lock().expect("events").push(("start", job));
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    active.fetch_sub(1, Ordering::Relaxed);
                    job
                }
            }
        },
        {
            let events = Arc::clone(&events);
            move |job| {
                let events = Arc::clone(&events);
                async move {
                    events.lock().expect("events").push(("finish", job));
                    RollingWindowControl::Continue
                }
            }
        },
    )
    .await
    .expect("rolling window");

    let events = events.lock().expect("events");
    let third_start = events
        .iter()
        .position(|event| *event == ("start", 3))
        .expect("third job start");
    let second_finish = events
        .iter()
        .position(|event| *event == ("finish", 2))
        .expect("second job finish");
    assert_eq!(completed, 3);
    assert_eq!(peak.load(Ordering::Relaxed), 2);
    assert!(third_start < second_finish);
}

#[tokio::test]
async fn stop_drains_active_jobs_without_fetching_replacements() {
    let pending = Arc::new(Mutex::new(VecDeque::from([1, 2, 3, 4])));
    let started = Arc::new(Mutex::new(Vec::new()));

    let completed = run_rolling_window(
        NonZeroUsize::new(2).expect("non-zero window"),
        {
            let pending = Arc::clone(&pending);
            move |capacity| {
                let mut pending = pending.lock().expect("pending jobs");
                Ok::<_, Infallible>((0..capacity).filter_map(|_| pending.pop_front()).collect())
            }
        },
        {
            let started = Arc::clone(&started);
            move |job| {
                let started = Arc::clone(&started);
                async move {
                    started.lock().expect("started jobs").push(job);
                    job
                }
            }
        },
        |_| async { RollingWindowControl::Stop },
    )
    .await
    .expect("rolling window");

    assert_eq!(completed, 2);
    assert_eq!(started.lock().expect("started jobs").as_slice(), &[1, 2]);
    assert_eq!(pending.lock().expect("pending jobs").len(), 2);
}
