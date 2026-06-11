//! Time as an injectable dependency so rate-limit/expiry logic is testable
//! without sleeping. Unix epoch millis.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}

/// Test clock: starts at a fixed value, advances only when told to.
#[derive(Debug)]
pub struct MockClock {
    ms: AtomicI64,
}

impl MockClock {
    pub fn new(start_ms: i64) -> Self {
        Self {
            ms: AtomicI64::new(start_ms),
        }
    }
    pub fn advance(&self, by_ms: i64) {
        self.ms.fetch_add(by_ms, Ordering::SeqCst);
    }
}

impl Clock for MockClock {
    fn now_ms(&self) -> i64 {
        self.ms.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_clock_advances() {
        let c = MockClock::new(1000);
        assert_eq!(c.now_ms(), 1000);
        c.advance(500);
        assert_eq!(c.now_ms(), 1500);
    }

    #[test]
    fn system_clock_is_positive() {
        assert!(SystemClock.now_ms() > 0);
    }
}
