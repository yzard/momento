use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct WorkSignal {
    shared: Arc<WorkSignalShared>,
}

#[derive(Default)]
struct WorkSignalShared {
    version: AtomicU64,
    changed: Notify,
}

impl WorkSignal {
    pub fn version(&self) -> u64 {
        self.shared.version.load(Ordering::Acquire)
    }

    pub fn notify(&self) {
        self.shared.version.fetch_add(1, Ordering::AcqRel);
        self.shared.changed.notify_waiters();
    }

    pub async fn wait_for_change(&self, observed_version: u64) -> u64 {
        loop {
            let changed = self.shared.changed.notified();
            let current_version = self.version();
            if current_version != observed_version {
                return current_version;
            }
            changed.await;
        }
    }
}
