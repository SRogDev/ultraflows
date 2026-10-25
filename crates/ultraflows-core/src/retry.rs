use std::time::Duration;

/// Retry policy, Temporal-shaped: initial interval, exponential backoff
/// coefficient, maximum interval, maximum attempts. No jitter in v0.1.0
/// (deterministic delays are easier to test and to demo).
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub backoff_factor: f64,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(500),
            backoff_factor: 2.0,
            max_delay: Duration::from_secs(8),
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
    fn exhaustion_is_exact() {
        let p = RetryPolicy::default();
        assert!(!p.exhausted(4));
        assert!(p.exhausted(5));
        assert!(p.exhausted(6));
    }
}
