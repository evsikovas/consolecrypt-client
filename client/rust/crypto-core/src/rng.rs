//! OS CSPRNG access (`getrandom`) — the only randomness source (ADR-0002).

use crate::error::{CryptoError, Result};

/// Fill `buf` with bytes from the operating system CSPRNG.
pub fn fill_random(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|_| CryptoError::Rng)
}

/// Return `N` random bytes. Use only for non-secret values (nonces, salts,
/// ids); secret keys are generated directly into their zeroizing boxes.
pub fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    fill_random(&mut out)?;
    Ok(out)
}

/// Uniformly random integer in `0..bound` (rejection sampling, no modulo
/// bias). `bound` must be non-zero.
pub fn random_below(bound: u32) -> Result<u32> {
    if bound == 0 {
        return Err(CryptoError::Rng);
    }
    // Largest multiple of `bound` that fits in u32 space; values at or above
    // it are rejected so every residue is equally likely.
    let zone = u32::MAX - (u32::MAX % bound);
    loop {
        let v = getrandom::u32().map_err(|_| CryptoError::Rng)?;
        if v < zone {
            return Ok(v % bound);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_below_stays_in_range() {
        for bound in [1u32, 2, 3, 24, 1000] {
            for _ in 0..200 {
                assert!(random_below(bound).unwrap() < bound);
            }
        }
        assert!(random_below(0).is_err());
    }

    #[test]
    fn random_arrays_differ() {
        let a: [u8; 32] = random_array().unwrap();
        let b: [u8; 32] = random_array().unwrap();
        assert_ne!(a, b);
    }
}
