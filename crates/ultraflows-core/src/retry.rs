use std::time::Duration;

/// Retry policy, Temporal-shaped: initial interval, exponential backoff
/// coefficient, maximum interval, maximum attempts. No jitter in v0.1.0
/// (deterministic delays are easier to test and to demo).
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub backoff_factor: f64,
    pub max_delay: Duration,
    /// When true, the delay is deterministically jittered (see
    /// `delay_with_jitter`): same seed gives the same delay, so retry
    /// timing stays testable. Trigger's `randomize` option.
    #[serde(default)]
    pub randomize: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(500),
            backoff_factor: 2.0,
            max_delay: Duration::from_secs(8),
            randomize: false,
        }
    }
}

impl RetryPolicy {
    /// Delay after `attempt` (1-based) has failed.
    pub fn delay_after(&self, attempt: u32) -> Duration {
        let exp = attempt.saturating_sub(1) as i32;
        let scaled = self.initial_delay.as_secs_f64() * self.backoff_factor.powi(exp);
        Duration::from_secs_f64(scaled.min(self.max_delay.as_secs_f64()))
    }

    /// Delay after `attempt` with jitter applied when `randomize` is
    /// set. The jitter is deterministic in `seed` (splitmix64): it
    /// spreads retries without making timing untestable or pulling in
    /// a randomness dependency. Result is in [0.5 * delay, delay].
    pub fn delay_with_jitter(&self, attempt: u32, seed: u64) -> Duration {
        let base = self.delay_after(attempt);
        if !self.randomize {
            return base;
        }
        let mut x = seed.wrapping_add(u64::from(attempt).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^= x >> 31;
        let frac = (x as f64) / (u64::MAX as f64);
        Duration::from_secs_f64(base.as_secs_f64() * (0.5 + 0.5 * frac))
    }

    pub fn exhausted(&self, attempt: u32) -> bool {
        attempt >= self.max_attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_exponentially_and_caps() {
        let p = RetryPolicy::default();
        assert_eq!(p.delay_after(1), Duration::from_millis(500));
        assert_eq!(p.delay_after(2), Duration::from_secs(1));
        assert_eq!(p.delay_after(3), Duration::from_secs(2));
        assert_eq!(p.delay_after(4), Duration::from_secs(4));
        assert_eq!(p.delay_after(5), Duration::from_secs(8));
        assert_eq!(p.delay_after(99), Duration::from_secs(8));
    }

    #[test]
    fn jitter_is_deterministic_and_bounded() {
        let p = RetryPolicy {
            randomize: true,
            ..Default::default()
        };
        let d1 = p.delay_with_jitter(2, 42);
        let d2 = p.delay_with_jitter(2, 42);
        assert_eq!(d1, d2);
        assert!(d1 >= Duration::from_millis(500) && d1 <= Duration::from_secs(1));
        let d3 = p.delay_with_jitter(2, 43);
        assert!(d3 >= Duration::from_millis(500) && d3 <= Duration::from_secs(1));
    }

    #[test]
    fn exhaustion_is_exact() {
        let p = RetryPolicy::default();
        assert!(!p.exhausted(4));
        assert!(p.exhausted(5));
        assert!(p.exhausted(6));
    }
}
