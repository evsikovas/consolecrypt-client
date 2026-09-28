//! Exponential backoff with jitter.

use std::time::Duration;

/// Retry schedule: `min(max, initial · multiplier^(attempt-1))`, then
/// "equal jitter" (uniform in `[d/2, d]`), never below a server-requested
/// `Retry-After`.
#[derive(Debug, Clone, PartialEq)]
pub struct BackoffConfig {
    pub initial: Duration,
    pub max: Duration,
    pub multiplier: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial: Duration::from_millis(500),
            max: Duration::from_secs(60),
            multiplier: 2.0,
        }
    }
}

impl BackoffConfig {
    /// Delay before retry number `attempt` (1-based).
    pub fn delay(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        let exp = self
            .multiplier
            .max(1.0)
            .powi(attempt.saturating_sub(1).min(64) as i32);
        let base = self.initial.as_secs_f64() * exp;
        let capped = base.min(self.max.as_secs_f64()).max(0.0);
        let jittered = capped / 2.0 + capped / 2.0 * unit_random();
        let d = Duration::from_secs_f64(jittered);
        match retry_after {
            Some(r) if r > d => r,
            _ => d,
        }
    }
}

/// Uniform in `[0, 1)` from the OS CSPRNG (via UUIDv4); jitter only.
fn unit_random() -> f64 {
    let bits = (uuid::Uuid::new_v4().as_u128() >> 75) as u64; // 53 random-ish bits
    bits as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_caps_and_honours_retry_after() {
        let b = BackoffConfig {
            initial: Duration::from_millis(100),
            max: Duration::from_secs(1),
            multiplier: 2.0,
        };
        for _ in 0..50 {
            let d1 = b.delay(1, None);
            assert!(d1 >= Duration::from_millis(50) && d1 <= Duration::from_millis(100));
            let d5 = b.delay(5, None);
            assert!(d5 >= Duration::from_millis(500) && d5 <= Duration::from_secs(1));
            assert!(b.delay(30, None) <= Duration::from_secs(1));
        }
        assert_eq!(
            b.delay(1, Some(Duration::from_secs(7))),
            Duration::from_secs(7)
        );
    }
}
