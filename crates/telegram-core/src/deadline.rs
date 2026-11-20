//! Transport-independent drain timing.

use std::sync::{Arc, OnceLock};
use tokio::time::Instant;

/// A clonable handle to the first absolute drain deadline.
#[derive(Clone, Debug, Default)]
pub struct DrainDeadline(Arc<OnceLock<Instant>>);

impl DrainDeadline {
    /// Set the deadline once and return the first value.
    pub fn start(&self, deadline: Instant) -> Instant {
        *self.0.get_or_init(|| deadline)
    }

    /// Return the deadline if draining has started.
    pub fn get(&self) -> Option<Instant> {
        self.0.get().copied()
    }
}
