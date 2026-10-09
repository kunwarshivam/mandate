//! Fixtures shared by the notice tests: a recording random source and a hex oracle of their own.

#![allow(
    dead_code,
    reason = "each test binary uses a different part of the fixtures"
)]

use mandate_notify::{NotifyError, SecureRandom};

/// A deterministic stand-in for the operating system's source (splitmix64), which records every id
/// it fills so a test can compute the expected hex without the crate.
pub struct Recording {
    state: u64,
    pub issued: Vec<[u8; 16]>,
}

impl Recording {
    pub fn seeded(seed: u64) -> Self {
        Self {
            state: seed,
            issued: Vec::new(),
        }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl SecureRandom for Recording {
    fn fill(&mut self, bytes: &mut [u8; 16]) -> Result<(), NotifyError> {
        let (hi, lo) = (self.next(), self.next());
        bytes[..8].copy_from_slice(&hi.to_be_bytes());
        bytes[8..].copy_from_slice(&lo.to_be_bytes());
        self.issued.push(*bytes);
        Ok(())
    }
}

/// A source that always fails.
pub struct Broken;

impl SecureRandom for Broken {
    fn fill(&mut self, _: &mut [u8; 16]) -> Result<(), NotifyError> {
        Err(NotifyError::EntropyUnavailable)
    }
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| [DIGITS[usize::from(b >> 4)], DIGITS[usize::from(b & 0xf)]])
        .map(char::from)
        .collect()
}

/// The value of an entry point's answer, or a panic that names the error, so a stub's
/// `Unimplemented` is what a pending test fails on.
pub fn answer<T>(what: &str, result: Result<T, NotifyError>) -> T {
    result.unwrap_or_else(|e| panic!("{what}: {e:?}"))
}

pub const ORIGIN: &str = "https://app.owlhead.example";
