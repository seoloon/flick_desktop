//! Time sources. All elapsed-time maths use a monotonic clock; the server's
//! wall-clock timestamps are only ever mapped onto it through the estimated
//! offset ([`crate::sync::ClockSync`]), so system clock changes cannot move
//! playback.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;

/// Monotonic milliseconds since an arbitrary origin.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> f64;
}

#[derive(Debug)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self { origin: Instant::now() }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn now_ms(&self) -> f64 {
        self.origin.elapsed().as_secs_f64() * 1000.0
    }
}

/// Deterministic clock for tests.
#[derive(Debug, Clone, Default)]
pub struct ManualClock(Arc<Mutex<f64>>);

impl ManualClock {
    pub fn advance(&self, ms: f64) {
        *self.0.lock() += ms;
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> f64 {
        *self.0.lock()
    }
}
