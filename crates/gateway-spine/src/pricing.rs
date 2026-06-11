//! Model prices, stored as i64 µUSD-per-million-tokens so cost is exact integer
//! arithmetic. Example: $3.00 / 1M input tokens → 3_000_000 µUSD per million →
//! `input_per_mtok = 3_000_000`. $0.075 / 1M → `75_000`.

use crate::money::Usd;
use crate::usage::TokenUsage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ModelPrice {
    pub input_per_mtok: i64,
    pub output_per_mtok: i64,
    pub cache_read_per_mtok: i64,
    pub cache_write_per_mtok: i64,
}

impl ModelPrice {
    /// Cost of a usage record. Each line item is rounded half-up to the µUSD
    /// independently and summed. i128 intermediate prevents overflow.
    pub fn cost(&self, u: &TokenUsage) -> Usd {
        fn line(price_per_mtok: i64, tokens: i64) -> i64 {
            let numerator = price_per_mtok as i128 * tokens as i128;
            // round half up; tokens and prices are non-negative
            let micros = (numerator + 500_000) / 1_000_000;
            micros as i64
        }
        Usd::from_micros(
            line(self.input_per_mtok, u.input_tokens)
                + line(self.output_per_mtok, u.output_tokens)
                + line(self.cache_read_per_mtok, u.cache_read_tokens)
                + line(self.cache_write_per_mtok, u.cache_write_tokens),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // GPT-4o-class: $2.50/M in, $10.00/M out.
    fn price() -> ModelPrice {
        ModelPrice {
            input_per_mtok: 2_500_000,
            output_per_mtok: 10_000_000,
            cache_read_per_mtok: 1_250_000,
            cache_write_per_mtok: 0,
        }
    }

    #[test]
    fn cost_of_round_numbers() {
        // 1M input + 1M output = $2.50 + $10.00 = $12.50
        let u = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..Default::default()
        };
        assert_eq!(price().cost(&u), Usd::from_dollars_f64(12.5));
    }

    #[test]
    fn cost_of_small_call() {
        // 1000 in + 500 out = $0.0025 + $0.005 = $0.0075 = 7_500 µUSD
        let u = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        };
        assert_eq!(price().cost(&u), Usd::from_micros(7_500));
    }

    #[test]
    fn cache_reads_are_discounted() {
        // 10_000 cache-read tokens at $1.25/M = $0.0125 = 12_500 µUSD
        let u = TokenUsage {
            cache_read_tokens: 10_000,
            ..Default::default()
        };
        assert_eq!(price().cost(&u), Usd::from_micros(12_500));
    }

    #[test]
    fn zero_usage_is_zero_cost() {
        assert_eq!(price().cost(&TokenUsage::default()), Usd::ZERO);
    }

    #[test]
    fn sub_micro_prices_round_half_up() {
        // $0.075/M input, 1 token → 0.075 µUSD → rounds to 0; 7 tokens → 0.525 → 1
        let p = ModelPrice {
            input_per_mtok: 75_000,
            ..Default::default()
        };
        assert_eq!(
            p.cost(&TokenUsage {
                input_tokens: 1,
                ..Default::default()
            }),
            Usd::from_micros(0)
        );
        assert_eq!(
            p.cost(&TokenUsage {
                input_tokens: 7,
                ..Default::default()
            }),
            Usd::from_micros(1)
        );
    }
}
