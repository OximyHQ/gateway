//! Response-header value formatting for the always-on cost + overhead surface.
//! `x-overhead-duration-ms` is the gateway's self-overhead (NOT upstream time);
//! `usage.cost` is rendered as a fixed-6-decimal USD string from integer µUSD so
//! it round-trips exactly (no float formatting drift). These strings are the
//! single source of truth for the header, the logs row display, and the
//! dashboard.

use gateway_spine::Usd;

pub const OVERHEAD_HEADER: &str = "x-overhead-duration-ms";
pub const COST_HEADER: &str = "x-oximy-cost-usd";
pub const SERVED_BY_HEADER: &str = "x-served-by";
pub const FALLBACK_HEADER: &str = "x-fallback";
pub const CACHE_HEADER: &str = "x-cache";

/// Render a `Usd` as a fixed 6-decimal USD string, e.g. 7_500 µUSD → "0.007500".
/// Integer-only: splits whole dollars and the µUSD remainder, zero-pads to 6.
pub fn cost_usd_string(cost: Usd) -> String {
    let micros = cost.micros();
    let sign = if micros < 0 { "-" } else { "" };
    let abs = micros.unsigned_abs();
    let dollars = abs / 1_000_000;
    let frac = abs % 1_000_000;
    format!("{sign}{dollars}.{frac:06}")
}

/// Render an overhead duration (ms) for the header.
pub fn overhead_ms_string(overhead_ms: i64) -> String {
    overhead_ms.max(0).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_formats_six_decimals() {
        assert_eq!(cost_usd_string(Usd::from_micros(7_500)), "0.007500");
        assert_eq!(cost_usd_string(Usd::from_micros(1_000_000)), "1.000000");
        assert_eq!(cost_usd_string(Usd::from_micros(12_500_000)), "12.500000");
        assert_eq!(cost_usd_string(Usd::ZERO), "0.000000");
    }

    #[test]
    fn cost_handles_negative_defensively() {
        assert_eq!(cost_usd_string(Usd::from_micros(-500)), "-0.000500");
    }

    #[test]
    fn overhead_clamps_negative_to_zero() {
        assert_eq!(overhead_ms_string(42), "42");
        assert_eq!(overhead_ms_string(-3), "0");
    }
}
