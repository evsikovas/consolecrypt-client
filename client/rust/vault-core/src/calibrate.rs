//! Argon2id parameter calibration at vault creation / passphrase change.
//!
//! Starts from the ADR-0002 default (64 MiB, t = 3, p = 1), measures one run
//! and scales towards `target`:
//!
//! * fast machine → more iterations (memory stays 64 MiB), capped at
//!   [`MAX_CALIBRATED_ITERATIONS`] because the *same* envelope must also be
//!   opened on the user's slowest device;
//! * slow machine → the default itself: new envelopes never go below
//!   [`Argon2Params::NEW_MINIMUM`] (64 MiB, t = 3), so unlocking simply takes
//!   longer there (about a second on old phones; 64 MiB is fine on every
//!   supported device). Before 0.8.7 a slow machine lowered iterations and
//!   then memory down to the protocol floor (19 MiB, t = 2); such envelopes
//!   keep opening and are re-wrapped on the next passphrase unlock.

use crate::error::Result;
use cc_crypto_core::{seal_password_envelope, Argon2Params, SecretString, Vrk};
use cc_protocol::VaultId;
use std::time::{Duration, Instant};

/// Default unlock-time target.
pub const DEFAULT_CALIBRATION_TARGET: Duration = Duration::from_millis(750);
/// Upper bound on calibrated iterations (≈ 2.7× the default cost).
pub const MAX_CALIBRATED_ITERATIONS: u32 = 8;

/// Measure Argon2id on this machine and return parameters for `target`.
/// Runs Argon2id once with the default parameters (~0.1–1 s, blocking).
pub fn calibrate_argon2(target: Duration) -> Result<Argon2Params> {
    let vrk = Vrk::generate()?;
    let pass = SecretString::from("consolecrypt-calibration");
    calibrate_argon2_with(target, |params| {
        let start = Instant::now();
        seal_password_envelope(&vrk, VaultId::NIL, &pass, params)?;
        Ok(start.elapsed())
    })
}

/// Calibration with an injectable measurement (`measure(params)` returns how
/// long one Argon2id run with `params` takes).
pub fn calibrate_argon2_with(
    target: Duration,
    mut measure: impl FnMut(Argon2Params) -> Result<Duration>,
) -> Result<Argon2Params> {
    let base = Argon2Params::NEW_MINIMUM;
    let elapsed = measure(base)?.as_secs_f64().max(1e-6);
    let target = target.as_secs_f64();
    // Cost is ~linear in iterations: a fast machine gets more of them, a
    // slow one keeps the minimum.
    let scaled = f64::from(base.iterations()) * target / elapsed;
    let iterations = (scaled.floor() as u32).clamp(base.iterations(), MAX_CALIBRATED_ITERATIONS);
    Ok(Argon2Params::new(
        base.memory_kib(),
        iterations,
        base.parallelism(),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(per_default_run_ms: u64) -> impl FnMut(Argon2Params) -> Result<Duration> {
        move |p| {
            assert_eq!(p, Argon2Params::DEFAULT);
            Ok(Duration::from_millis(per_default_run_ms))
        }
    }

    #[test]
    fn fast_machine_raises_iterations_with_cap() {
        let t = Duration::from_millis(750);
        let p = calibrate_argon2_with(t, fake(250)).unwrap();
        assert_eq!((p.memory_kib(), p.iterations()), (65536, 8));
        let p = calibrate_argon2_with(t, fake(500)).unwrap();
        assert_eq!((p.memory_kib(), p.iterations()), (65536, 4));
        let p = calibrate_argon2_with(t, fake(740)).unwrap();
        assert_eq!(p, Argon2Params::DEFAULT);
        let p = calibrate_argon2_with(t, fake(1)).unwrap();
        assert_eq!(p.iterations(), MAX_CALIBRATED_ITERATIONS);
    }

    #[test]
    fn slow_machine_never_below_the_new_minimum() {
        let t = Duration::from_millis(750);
        for ms in [1000, 3000, 60_000] {
            let p = calibrate_argon2_with(t, fake(ms)).unwrap();
            assert_eq!(p, Argon2Params::NEW_MINIMUM, "{ms} ms");
            assert!(p.meets_new_minimum());
        }
        assert_eq!(
            (
                Argon2Params::NEW_MINIMUM.memory_kib(),
                Argon2Params::NEW_MINIMUM.iterations()
            ),
            (64 * 1024, 3)
        );
    }

    #[test]
    fn real_measurement_returns_valid_params() {
        // Tiny target → slow path; one real Argon2 run at default params.
        let p = calibrate_argon2(Duration::from_millis(1)).unwrap();
        assert_eq!(p, Argon2Params::NEW_MINIMUM);
    }
}
