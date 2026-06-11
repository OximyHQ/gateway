//! Cache analytics: hit/miss/bypass counters and cumulative dollars saved. A HIT
//! "saves" the cost the original call billed (`original_cost` on the entry) —
//! summed in integer µUSD, never floats. Counters are atomic so the hot path
//! records without a lock. P1.7 reads these for the dashboard's cache panel.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use gateway_spine::Usd;

#[derive(Default)]
pub struct CacheStats {
    hits: AtomicU64,
    misses: AtomicU64,
    bypasses: AtomicU64,
    saved_micros: AtomicI64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CacheStatsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub bypasses: u64,
    pub dollars_saved_micros: i64,
}

impl CacheStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a HIT and the dollars it saved (the original call's cost).
    pub fn record_hit(&self, saved: Usd) {
        self.hits.fetch_add(1, Ordering::Relaxed);
        self.saved_micros
            .fetch_add(saved.micros(), Ordering::Relaxed);
    }
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_bypass(&self) {
        self.bypasses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> CacheStatsSnapshot {
        CacheStatsSnapshot {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            bypasses: self.bypasses.load(Ordering::Relaxed),
            dollars_saved_micros: self.saved_micros.load(Ordering::Relaxed),
        }
    }
}

impl CacheStatsSnapshot {
    /// Hit rate over (hits + misses); BYPASS is excluded (a deliberate skip is not
    /// a cache failure). Returns 0.0 when there were no cacheable lookups.
    pub fn hit_rate(&self) -> f64 {
        let denom = self.hits + self.misses;
        if denom == 0 {
            0.0
        } else {
            self.hits as f64 / denom as f64
        }
    }

    pub fn dollars_saved(&self) -> Usd {
        Usd::from_micros(self.dollars_saved_micros)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_rate_excludes_bypass() {
        let s = CacheStats::new();
        s.record_hit(Usd::from_micros(1_000));
        s.record_hit(Usd::from_micros(2_000));
        s.record_miss();
        s.record_bypass();
        let snap = s.snapshot();
        assert_eq!(snap.hits, 2);
        assert_eq!(snap.misses, 1);
        assert_eq!(snap.bypasses, 1);
        // 2 hits / (2 + 1) = 0.666...
        assert!((snap.hit_rate() - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn dollars_saved_sums_in_micros() {
        let s = CacheStats::new();
        s.record_hit(Usd::from_micros(7_500));
        s.record_hit(Usd::from_micros(2_500));
        assert_eq!(s.snapshot().dollars_saved(), Usd::from_micros(10_000));
    }

    #[test]
    fn zero_lookups_is_zero_rate() {
        assert_eq!(CacheStats::new().snapshot().hit_rate(), 0.0);
    }
}
