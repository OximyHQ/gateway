//! Per-target state for latency-aware EWMA and passive circuit-breaker
//! cooldown. These are owned by `Router` and updated after every call.

use std::sync::Mutex;

use gateway_spine::Clock;

/// Per-target mutable state tracked by the router.
#[derive(Debug)]
pub struct TargetState {
    /// EWMA latency estimate in milliseconds. `None` = no data yet.
    ewma_ms: Option<f64>,
    /// Consecutive failure counter (reset on success).
    consecutive_failures: u32,
    /// Wall-clock time (ms) when the target entered cooldown.
    /// `None` = healthy.
    cooldown_since_ms: Option<i64>,
}

impl Default for TargetState {
    fn default() -> Self {
        Self::new()
    }
}

impl TargetState {
    pub fn new() -> Self {
        TargetState {
            ewma_ms: None,
            consecutive_failures: 0,
            cooldown_since_ms: None,
        }
    }

    /// EWMA smoothing factor (α). Higher = more weight to recent samples.
    const ALPHA: f64 = 0.2;

    /// Record a successful call; update EWMA with the observed latency.
    pub fn record_success(&mut self, latency_ms: f64) {
        self.consecutive_failures = 0;
        self.cooldown_since_ms = None;
        self.ewma_ms = Some(match self.ewma_ms {
            None => latency_ms,
            Some(prev) => Self::ALPHA * latency_ms + (1.0 - Self::ALPHA) * prev,
        });
    }

    /// Record a failure. Returns `true` if this pushed the target into
    /// cooldown (threshold crossed).
    pub fn record_failure(&mut self, failure_threshold: u32, clock: &dyn Clock) -> bool {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= failure_threshold && self.cooldown_since_ms.is_none() {
            self.cooldown_since_ms = Some(clock.now_ms());
            return true;
        }
        false
    }

    /// Returns `true` if the target is currently in cooldown (and the window
    /// has not yet expired).
    pub fn is_in_cooldown(&self, cooldown_ms: u64, clock: &dyn Clock) -> bool {
        if let Some(since) = self.cooldown_since_ms {
            let elapsed = (clock.now_ms() - since).max(0) as u64;
            elapsed < cooldown_ms
        } else {
            false
        }
    }

    /// Try to recover: if the cooldown window has elapsed, clear the state.
    /// Returns `true` if the target is now healthy (either was never in
    /// cooldown, or the window just expired).
    pub fn maybe_recover(&mut self, cooldown_ms: u64, clock: &dyn Clock) -> bool {
        if let Some(since) = self.cooldown_since_ms {
            let elapsed = (clock.now_ms() - since).max(0) as u64;
            if elapsed >= cooldown_ms {
                // Cooldown window expired — auto-recover.
                self.cooldown_since_ms = None;
                self.consecutive_failures = 0;
                return true;
            }
            return false;
        }
        true
    }

    pub fn ewma_ms(&self) -> Option<f64> {
        self.ewma_ms
    }
}

/// Thread-safe wrapper around per-target state, one slot per `Route::targets`.
pub struct TargetStateVec {
    states: Vec<Mutex<TargetState>>,
}

impl TargetStateVec {
    pub fn new(n: usize) -> Self {
        TargetStateVec {
            states: (0..n).map(|_| Mutex::new(TargetState::new())).collect(),
        }
    }

    pub fn record_success(&self, idx: usize, latency_ms: f64) {
        if let Some(m) = self.states.get(idx) {
            m.lock().unwrap().record_success(latency_ms);
        }
    }

    pub fn record_failure(&self, idx: usize, failure_threshold: u32, clock: &dyn Clock) {
        if let Some(m) = self.states.get(idx) {
            m.lock().unwrap().record_failure(failure_threshold, clock);
        }
    }

    pub fn is_in_cooldown(&self, idx: usize, cooldown_ms: u64, clock: &dyn Clock) -> bool {
        if let Some(m) = self.states.get(idx) {
            m.lock().unwrap().is_in_cooldown(cooldown_ms, clock)
        } else {
            false
        }
    }

    pub fn maybe_recover(&self, idx: usize, cooldown_ms: u64, clock: &dyn Clock) -> bool {
        if let Some(m) = self.states.get(idx) {
            m.lock().unwrap().maybe_recover(cooldown_ms, clock)
        } else {
            true
        }
    }

    pub fn ewma_ms(&self, idx: usize) -> Option<f64> {
        self.states.get(idx)?.lock().unwrap().ewma_ms()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_spine::MockClock;

    #[test]
    fn ewma_converges_to_latest_on_first_sample() {
        let mut s = TargetState::new();
        let clock = MockClock::new(0);
        s.record_success(100.0);
        assert_eq!(s.ewma_ms(), Some(100.0));
        s.record_success(200.0);
        let v = s.ewma_ms().unwrap();
        // ALPHA=0.2: 0.2*200 + 0.8*100 = 120
        assert!((v - 120.0).abs() < 1e-9);
        // still healthy
        assert!(!s.is_in_cooldown(30_000, &clock));
    }

    #[test]
    fn cooldown_triggered_after_threshold() {
        let mut s = TargetState::new();
        let clock = MockClock::new(1000);
        assert!(!s.record_failure(3, &clock));
        assert!(!s.record_failure(3, &clock));
        let tripped = s.record_failure(3, &clock);
        assert!(tripped);
        assert!(s.is_in_cooldown(30_000, &clock));
    }

    #[test]
    fn cooldown_recovers_after_window() {
        let mut s = TargetState::new();
        let clock = MockClock::new(0);
        s.record_failure(1, &clock);
        assert!(s.is_in_cooldown(5_000, &clock));
        clock.advance(5_001);
        assert!(!s.is_in_cooldown(5_000, &clock));
        assert!(s.maybe_recover(5_000, &clock));
    }

    #[test]
    fn success_clears_consecutive_failures() {
        let mut s = TargetState::new();
        let clock = MockClock::new(0);
        s.record_failure(5, &clock);
        s.record_failure(5, &clock);
        s.record_success(50.0);
        // now should NOT be in cooldown even after more failures
        assert!(!s.is_in_cooldown(30_000, &clock));
        assert_eq!(s.ewma_ms(), Some(50.0));
    }
}
