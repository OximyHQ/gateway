//! Per-key fixed-window RPM/TPM plus a live parallel counter. Acquire checks all
//! configured dimensions atomically; `release_parallel` is called when a request
//! finishes. Window is one minute, reset lazily on first acquire of a new minute.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::clock::Clock;
use crate::error::{RateDimension, SpineError};
use crate::key::RateLimits;

const WINDOW_MS: i64 = 60_000;

#[derive(Debug, Default, Clone, Copy)]
struct Window {
    window_start_ms: i64,
    requests: i64,
    tokens: i64,
    parallel: i64,
}

pub struct RateLimiter<C: Clock> {
    clock: C,
    inner: Mutex<HashMap<String, Window>>,
}

impl<C: Clock> RateLimiter<C> {
    pub fn new(clock: C) -> Self {
        Self {
            clock,
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Acquire one request slot + `est_tokens`. Fail-closed on any breached
    /// dimension; on success the counters are incremented (caller must later
    /// call `release_parallel`).
    pub fn acquire(
        &self,
        key_id: &str,
        limits: &RateLimits,
        est_tokens: i64,
    ) -> Result<(), SpineError> {
        let now = self.clock.now_ms();
        let mut g = self.inner.lock().unwrap();
        let w = g.entry(key_id.to_string()).or_default();

        // Roll the window (parallel is NOT reset — it tracks live in-flight work).
        if now - w.window_start_ms >= WINDOW_MS {
            w.window_start_ms = now;
            w.requests = 0;
            w.tokens = 0;
        }

        if let Some(rpm) = limits.rpm
            && w.requests + 1 > rpm
        {
            return Err(SpineError::RateLimited {
                key_id: key_id.into(),
                dimension: RateDimension::Requests,
            });
        }
        if let Some(tpm) = limits.tpm
            && w.tokens + est_tokens > tpm
        {
            return Err(SpineError::RateLimited {
                key_id: key_id.into(),
                dimension: RateDimension::Tokens,
            });
        }
        if let Some(maxp) = limits.max_parallel
            && w.parallel + 1 > maxp
        {
            return Err(SpineError::RateLimited {
                key_id: key_id.into(),
                dimension: RateDimension::Parallel,
            });
        }

        w.requests += 1;
        w.tokens += est_tokens;
        w.parallel += 1;
        Ok(())
    }

    pub fn release_parallel(&self, key_id: &str) {
        let mut g = self.inner.lock().unwrap();
        if let Some(w) = g.get_mut(key_id)
            && w.parallel > 0
        {
            w.parallel -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::MockClock;

    fn limits(rpm: Option<i64>, tpm: Option<i64>, par: Option<i64>) -> RateLimits {
        RateLimits {
            rpm,
            tpm,
            max_parallel: par,
        }
    }

    #[test]
    fn rpm_blocks_after_limit_then_resets_next_window() {
        let clock = MockClock::new(0);
        let rl = RateLimiter::new(clock);
        let lim = limits(Some(2), None, None);

        assert!(rl.acquire("k", &lim, 0).is_ok());
        rl.release_parallel("k");
        assert!(rl.acquire("k", &lim, 0).is_ok());
        rl.release_parallel("k");
        // third in the same minute → blocked
        assert!(matches!(
            rl.acquire("k", &lim, 0),
            Err(SpineError::RateLimited {
                dimension: RateDimension::Requests,
                ..
            })
        ));

        // next minute resets
        rl.clock.advance(60_000);
        assert!(rl.acquire("k", &lim, 0).is_ok());
    }

    #[test]
    fn tpm_counts_estimated_tokens() {
        let rl = RateLimiter::new(MockClock::new(0));
        let lim = limits(None, Some(1000), None);
        assert!(rl.acquire("k", &lim, 700).is_ok());
        rl.release_parallel("k");
        // 700 + 400 > 1000 → blocked on tokens
        assert!(matches!(
            rl.acquire("k", &lim, 400),
            Err(SpineError::RateLimited {
                dimension: RateDimension::Tokens,
                ..
            })
        ));
    }

    #[test]
    fn parallel_limit_tracks_in_flight() {
        let rl = RateLimiter::new(MockClock::new(0));
        let lim = limits(None, None, Some(1));
        assert!(rl.acquire("k", &lim, 0).is_ok());
        // second concurrent → blocked
        assert!(matches!(
            rl.acquire("k", &lim, 0),
            Err(SpineError::RateLimited {
                dimension: RateDimension::Parallel,
                ..
            })
        ));
        // first finishes → slot frees
        rl.release_parallel("k");
        assert!(rl.acquire("k", &lim, 0).is_ok());
    }

    #[test]
    fn no_limits_means_unlimited() {
        let rl = RateLimiter::new(MockClock::new(0));
        let lim = RateLimits::default();
        for _ in 0..10_000 {
            assert!(rl.acquire("k", &lim, 1_000_000).is_ok());
            rl.release_parallel("k");
        }
    }
}
