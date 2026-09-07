use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{Mutex, Notify};

/// One shared admission budget. Reservations must live with executor work,
/// not with the caller waiting for its response.
pub(crate) struct MemoryBudget {
    maximum: u64,
    used: AtomicU64,
    turn: Mutex<()>,
    changed: Notify,
}

impl MemoryBudget {
    pub(crate) fn maximum(&self) -> u64 {
        self.maximum
    }

    pub(crate) fn new(maximum: u64) -> Arc<Self> {
        Arc::new(Self {
            maximum,
            used: AtomicU64::new(0),
            turn: Mutex::new(()),
            changed: Notify::new(),
        })
    }

    pub(crate) async fn acquire(self: &Arc<Self>, bytes: u64) -> Result<MemoryReservation, String> {
        if bytes == 0 || bytes > self.maximum {
            tracing::error!(
                error_code = "magick_memory_quota_exceeded",
                requested_bytes = bytes,
                quota_bytes = self.maximum,
                retryable = false,
                "ImageMagick request cannot fit the configured total quota; increase media_process.magick_memory_quota_bytes and restart before retrying"
            );
            return Err(format!(
                "ImageMagick memory request {bytes} bytes exceeds magick_memory_quota_bytes {}",
                self.maximum
            ));
        }
        let started = std::time::Instant::now();
        let mut logged = false;
        let _turn = match self.turn.try_lock() {
            Ok(turn) => turn,
            Err(_) => {
                self.log_wait(bytes, "fifo_wait");
                logged = true;
                self.turn.lock().await
            }
        };
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self
                .used
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                    used.checked_add(bytes)
                        .filter(|total| *total <= self.maximum)
                })
                .is_ok()
            {
                if logged {
                    tracing::info!(
                        event = "magick_memory_quota_acquired",
                        requested_bytes = bytes,
                        quota_bytes = self.maximum,
                        waited_ms = started.elapsed().as_millis() as u64,
                        "ImageMagick memory quota available; resuming work"
                    );
                }
                return Ok(MemoryReservation {
                    budget: Arc::clone(self),
                    bytes,
                });
            }
            if !logged {
                self.log_wait(bytes, "capacity_in_use");
                logged = true;
            }
            changed.await;
        }
    }

    fn log_wait(&self, bytes: u64, reason: &str) {
        tracing::info!(
            error_code = "magick_memory_quota_busy",
            requested_bytes = bytes,
            used_bytes = self.used.load(Ordering::Acquire),
            quota_bytes = self.maximum,
            reason,
            "ImageMagick quota temporarily occupied; waiting asynchronously, not failing the job"
        );
    }
}

pub(crate) struct MemoryReservation {
    budget: Arc<MemoryBudget>,
    bytes: u64,
}

impl Drop for MemoryReservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
        self.budget.changed.notify_one();
    }
}

#[cfg(test)]
#[path = "../../../tests/backend/runtime/memory.rs"]
mod tests;
