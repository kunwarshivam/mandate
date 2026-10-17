//! The operating system's random source, behind both random traits the dispatcher draws from
//! (E8-10 slice D10a, DEC-2500 item 2): [`mandate_notify::SecureRandom`], for notice ids and event
//! ids, and [`mandate_webpush::SecureRandom`], for each web push's salt and ephemeral key. It reads
//! the kernel's generator through `ring`, already one of the process's dependencies (DEC-2503),
//! so the dispatcher adds no crate for it. A source that cannot answer is the closed
//! [`EntropyUnavailable`], never a panic and never a predictable fallback: the caller then mints
//! nothing and sends nothing.

use mandate_notify::{NotifyError, SecureRandom as NoticeRandom};
use mandate_webpush::{SecureRandom as PushRandom, WebPushError};
use ring::rand::SystemRandom;

/// The random source could not fill a buffer. Closed: it holds nothing the source said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the operating system's random source is unavailable")]
pub struct EntropyUnavailable;

/// Where [`OsRandom`]'s bytes come from: the kernel's generator in the process ([`System`]), a
/// source that fails in the tests.
pub trait Entropy {
    /// Fills `out` entirely, or fails.
    ///
    /// # Errors
    /// [`EntropyUnavailable`] when the source cannot fill `out`.
    fn fill(&self, out: &mut [u8]) -> Result<(), EntropyUnavailable>;
}

/// The kernel's generator, through `ring`.
#[derive(Debug)]
pub struct System(SystemRandom);

impl System {
    /// The kernel's generator. Nothing is read until the first fill.
    #[must_use]
    pub fn new() -> Self {
        Self(SystemRandom::new())
    }
}

impl Default for System {
    fn default() -> Self {
        Self::new()
    }
}

impl Entropy for System {
    /// Fills `out` from the kernel's generator through `ring`. This leaf is implemented in the
    /// tests PR, not stubbed: its only failure is the closed [`EntropyUnavailable`], which carries no
    /// `Unimplemented` report, so a stub here would leave a mutant `cargo xtask ci mutants` cannot
    /// exempt (DEC-137). `OsRandom`'s two traits over it stay stubs until the implementation PR.
    fn fill(&self, out: &mut [u8]) -> Result<(), EntropyUnavailable> {
        ring::rand::SecureRandom::fill(&self.0, out).map_err(|_| EntropyUnavailable)
    }
}

/// The process's one random source, over `entropy`.
#[derive(Debug, Default)]
pub struct OsRandom<E: Entropy = System> {
    #[allow(
        dead_code,
        reason = "the tests PR's stubs read no field; the implementation fills from it (DEC-77, E8-10 D10a)"
    )]
    entropy: E,
}

impl OsRandom<System> {
    /// The kernel's generator.
    #[must_use]
    pub fn new() -> Self {
        Self::over(System::new())
    }
}

impl<E: Entropy> OsRandom<E> {
    /// A source over `entropy`.
    pub fn over(entropy: E) -> Self {
        Self { entropy }
    }
}

impl<E: Entropy> NoticeRandom for OsRandom<E> {
    /// Fills `bytes` from the source; [`EntropyUnavailable`] is
    /// [`NotifyError::EntropyUnavailable`].
    fn fill(&mut self, _bytes: &mut [u8; 16]) -> Result<(), NotifyError> {
        Err(NotifyError::Unimplemented { story: "E8-10" })
    }
}

impl<E: Entropy> PushRandom for OsRandom<E> {
    /// Fills `out` from the source; [`EntropyUnavailable`] is [`WebPushError::Random`].
    fn fill(&mut self, _out: &mut [u8]) -> Result<(), WebPushError> {
        Err(WebPushError::Unimplemented { story: "E8-10" })
    }
}

#[cfg(test)]
mod tests;
