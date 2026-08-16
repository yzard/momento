use std::future::Future;
use std::num::NonZeroUsize;

use futures::stream::{FuturesUnordered, StreamExt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RollingWindowControl {
    Continue,
    Stop,
}

pub async fn run_rolling_window<
    Job,
    Output,
    FetchError,
    Fetch,
    Process,
    ProcessFuture,
    Complete,
    CompleteFuture,
>(
    max_in_flight: NonZeroUsize,
    mut fetch: Fetch,
    process: Process,
    complete: Complete,
) -> Result<usize, FetchError>
where
    Fetch: FnMut(usize) -> Result<Vec<Job>, FetchError>,
    Process: Fn(Job) -> ProcessFuture,
    ProcessFuture: Future<Output = Output>,
    Complete: Fn(Output) -> CompleteFuture,
    CompleteFuture: Future<Output = RollingWindowControl>,
{
    let mut in_flight = FuturesUnordered::new();
    let mut accepting = true;
    let mut completed = 0;
    loop {
        if accepting {
            let capacity = max_in_flight.get() - in_flight.len();
            if capacity > 0 {
                for job in fetch(capacity)? {
                    in_flight.push(process(job));
                }
            }
        }
        let Some(output) = in_flight.next().await else {
            return Ok(completed);
        };
        completed += 1;
        if complete(output).await == RollingWindowControl::Stop {
            accepting = false;
        }
    }
}
