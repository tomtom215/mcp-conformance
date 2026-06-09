// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Deterministic retry/backoff policy.
//!
//! Exponential backoff with bounded multiplicative jitter and `Retry-After` honoring.
//! Determinism is a design requirement, not an accident: randomness enters only as the
//! caller-supplied `jitter_unit`, so tests (and trace replays) can reproduce every
//! delay exactly. The eventual host wires a real RNG in; CI wires constants.

use core::fmt;
use core::time::Duration;

/// Retry schedule: exponential backoff, multiplicative jitter, hard caps.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    initial_delay: Duration,
    multiplier: f64,
    max_delay: Duration,
    max_retries: u32,
    jitter_fraction: f64,
}

/// Error produced when constructing an invalid [`RetryPolicy`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RetryPolicyError {
    /// `multiplier` must be ≥ 1.0 and finite (backoff must not shrink).
    InvalidMultiplier,
    /// `jitter_fraction` must be in `0.0..1.0`.
    InvalidJitterFraction,
    /// `initial_delay` must be non-zero and ≤ `max_delay`.
    InvalidDelayBounds,
}

impl fmt::Display for RetryPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::InvalidMultiplier => "multiplier must be finite and >= 1.0",
            Self::InvalidJitterFraction => "jitter_fraction must be in 0.0..1.0",
            Self::InvalidDelayBounds => "initial_delay must be non-zero and <= max_delay",
        };
        f.write_str(text)
    }
}

impl core::error::Error for RetryPolicyError {}

impl Default for RetryPolicy {
    /// 250 ms initial, doubling, capped at 30 s, 5 retries, 20% jitter — conservative
    /// enough for local conformance runs, bounded enough that a wedged SUT fails a CI
    /// job in seconds rather than minutes.
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_millis(250),
            multiplier: 2.0,
            max_delay: Duration::from_secs(30),
            max_retries: 5,
            jitter_fraction: 0.2,
        }
    }
}

impl RetryPolicy {
    /// Builds a validated policy.
    ///
    /// # Errors
    ///
    /// Returns [`RetryPolicyError`] when `multiplier < 1.0` or non-finite,
    /// `jitter_fraction` outside `0.0..1.0`, `initial_delay` zero, or
    /// `initial_delay > max_delay`.
    pub fn new(
        initial_delay: Duration,
        multiplier: f64,
        max_delay: Duration,
        max_retries: u32,
        jitter_fraction: f64,
    ) -> Result<Self, RetryPolicyError> {
        if !multiplier.is_finite() || multiplier < 1.0 {
            return Err(RetryPolicyError::InvalidMultiplier);
        }
        if !jitter_fraction.is_finite() || !(0.0..1.0).contains(&jitter_fraction) {
            return Err(RetryPolicyError::InvalidJitterFraction);
        }
        if initial_delay.is_zero() || initial_delay > max_delay {
            return Err(RetryPolicyError::InvalidDelayBounds);
        }
        Ok(Self {
            initial_delay,
            multiplier,
            max_delay,
            max_retries,
            jitter_fraction,
        })
    }

    /// The maximum number of retries before giving up.
    #[must_use]
    pub const fn max_retries(&self) -> u32 {
        self.max_retries
    }

    /// The delay before retry number `retry` (1-based), or `None` when the budget is
    /// exhausted (or `retry` is 0, which is not a retry).
    ///
    /// `jitter_unit` is clamped to `0.0..=1.0`; `0.0` means no jitter, `1.0` means the
    /// full configured reduction. Jitter only ever *shrinks* the delay (multiplicative,
    /// `base * (1 - jitter_fraction * unit)`), so the un-jittered value is the
    /// worst-case bound schedulers can rely on.
    #[must_use]
    pub fn delay_for_retry(&self, retry: u32, jitter_unit: f64) -> Option<Duration> {
        if retry == 0 || retry > self.max_retries {
            return None;
        }
        let base = self.base_delay(retry);
        let unit = if jitter_unit.is_finite() {
            jitter_unit.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let factor = self.jitter_fraction.mul_add(-unit, 1.0);
        let jittered = base.as_secs_f64() * factor;
        // factor ∈ (0, 1] and base ≤ max_delay, so this cannot overflow; the checked
        // constructor keeps the function total anyway.
        Duration::try_from_secs_f64(jittered).ok()
    }

    /// The delay before retry number `retry` when the server sent `Retry-After`.
    ///
    /// The server's instruction wins over the computed backoff but is clamped to
    /// `max_delay` (a hostile or broken server must not park the host for an hour),
    /// and the retry budget still applies. No jitter: the server named an exact time.
    #[must_use]
    pub fn delay_honoring_retry_after(
        &self,
        retry: u32,
        retry_after: Duration,
    ) -> Option<Duration> {
        if retry == 0 || retry > self.max_retries {
            return None;
        }
        Some(retry_after.min(self.max_delay))
    }

    fn base_delay(&self, retry: u32) -> Duration {
        // retry ≥ 1 here; exponent is retry - 1 (first retry waits initial_delay).
        let exponent = retry - 1;
        let scale = if exponent >= 64 {
            f64::INFINITY // saturates to max_delay below
        } else {
            // exponent < 64 always fits i32; unwrap_or keeps the expression total.
            self.multiplier
                .powi(i32::try_from(exponent).unwrap_or(i32::MAX))
        };
        let scaled = self.initial_delay.as_secs_f64() * scale;
        if scaled.is_finite() {
            Duration::try_from_secs_f64(scaled)
                .map_or(self.max_delay, |delay| delay.min(self.max_delay))
        } else {
            self.max_delay
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn schedule_doubles_then_caps() {
        let policy = RetryPolicy::new(
            Duration::from_millis(100),
            2.0,
            Duration::from_millis(450),
            6,
            0.0,
        )
        .unwrap();
        let delays: Vec<Option<Duration>> = (0..=7)
            .map(|retry| policy.delay_for_retry(retry, 0.0))
            .collect();
        assert_eq!(
            delays,
            vec![
                None,                             // retry 0 is not a retry
                Some(Duration::from_millis(100)), // 100 * 2^0
                Some(Duration::from_millis(200)), // 100 * 2^1
                Some(Duration::from_millis(400)), // 100 * 2^2
                Some(Duration::from_millis(450)), // capped
                Some(Duration::from_millis(450)), // capped
                Some(Duration::from_millis(450)), // capped
                None,                             // budget exhausted
            ]
        );
    }

    #[test]
    fn jitter_only_shrinks_and_is_deterministic() {
        let policy = RetryPolicy::default();
        let base = policy.delay_for_retry(3, 0.0).unwrap();
        let jittered = policy.delay_for_retry(3, 1.0).unwrap();
        assert!(jittered < base);
        assert_eq!(
            policy.delay_for_retry(3, 0.7),
            policy.delay_for_retry(3, 0.7)
        );
        // Non-finite jitter inputs degrade to no jitter rather than poisoning the math.
        assert_eq!(policy.delay_for_retry(3, f64::NAN).unwrap(), base);
    }

    #[test]
    fn retry_after_wins_but_is_clamped_and_budgeted() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.delay_honoring_retry_after(1, Duration::from_secs(2)),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            policy.delay_honoring_retry_after(1, Duration::from_secs(3600)),
            Some(Duration::from_secs(30)),
            "a server cannot park the host beyond max_delay"
        );
        assert_eq!(
            policy.delay_honoring_retry_after(99, Duration::from_secs(1)),
            None,
            "Retry-After does not extend the retry budget"
        );
    }

    #[test]
    fn constructor_rejects_degenerate_policies() {
        let ms = Duration::from_millis;
        assert_eq!(
            RetryPolicy::new(ms(100), 0.5, ms(1000), 3, 0.0),
            Err(RetryPolicyError::InvalidMultiplier)
        );
        assert_eq!(
            RetryPolicy::new(ms(100), 2.0, ms(1000), 3, 1.0),
            Err(RetryPolicyError::InvalidJitterFraction)
        );
        assert_eq!(
            RetryPolicy::new(ms(0), 2.0, ms(1000), 3, 0.0),
            Err(RetryPolicyError::InvalidDelayBounds)
        );
        assert_eq!(
            RetryPolicy::new(ms(2000), 2.0, ms(1000), 3, 0.0),
            Err(RetryPolicyError::InvalidDelayBounds)
        );
    }

    proptest! {
        #[test]
        fn unjittered_schedule_is_monotonic_until_cap(retry in 1u32..=64) {
            let policy = RetryPolicy::default();
            if let (Some(a), Some(b)) = (
                policy.delay_for_retry(retry, 0.0),
                policy.delay_for_retry(retry.saturating_add(1), 0.0),
            ) {
                prop_assert!(b >= a);
            }
        }

        #[test]
        fn jittered_delay_stays_within_bounds(retry in 1u32..=5, unit in 0.0f64..=1.0) {
            let policy = RetryPolicy::default();
            let base = policy.delay_for_retry(retry, 0.0).unwrap();
            let jittered = policy.delay_for_retry(retry, unit).unwrap();
            prop_assert!(jittered <= base);
            // jitter_fraction is 0.2, so the floor is 80% of base.
            let floor = base.mul_f64(0.8 - 1e-9);
            prop_assert!(jittered >= floor, "{jittered:?} < {floor:?}");
        }

        #[test]
        fn delays_never_exceed_max_or_panic(retry in 0u32..=1000, unit in proptest::num::f64::ANY) {
            let policy = RetryPolicy::default();
            if let Some(delay) = policy.delay_for_retry(retry, unit) {
                prop_assert!(delay <= Duration::from_secs(30));
            }
            if let Some(delay) = policy.delay_honoring_retry_after(retry, Duration::from_secs(u64::MAX / 4)) {
                prop_assert!(delay <= Duration::from_secs(30));
            }
        }
    }
}
