//! E8-10 slice D10a (DEC-2500 item 2): the operating system's random source behind both random
//! traits, and a source that cannot answer.

use super::{Entropy, EntropyUnavailable, OsRandom, System};
use mandate_notify::{NotifyError, SecureRandom as NoticeRandom};
use mandate_webpush::{SecureRandom as PushRandom, WebPushError};

/// A source that never answers, as a kernel without entropy or a sandbox without the syscall.
struct Dry;

impl Entropy for Dry {
    fn fill(&self, _out: &mut [u8]) -> Result<(), EntropyUnavailable> {
        Err(EntropyUnavailable)
    }
}

#[test]
#[ignore = "pending E8-10"]
fn the_system_source_fills_every_buffer_and_never_repeats_a_draw() {
    let mut random = OsRandom::new();
    let (mut first, mut second) = ([0u8; 16], [0u8; 16]);
    let drawn = (
        NoticeRandom::fill(&mut random, &mut first),
        NoticeRandom::fill(&mut random, &mut second),
    );
    assert_eq!(drawn, (Ok(()), Ok(())), "two notice-id draws");
    assert_ne!(first, second, "two 128-bit draws are distinct");
    let mut salt = [0u8; 65];
    assert_eq!(PushRandom::fill(&mut random, &mut salt), Ok(()));
    assert_ne!(salt, [0u8; 65], "a web-push draw fills its whole buffer");
    let mut direct = [0u8; 32];
    assert_eq!(System::new().fill(&mut direct), Ok(()));
    assert_ne!(direct, [0u8; 32], "the kernel's generator answers");
}

#[test]
#[ignore = "pending E8-10"]
fn a_source_that_cannot_answer_is_entropy_unavailable_on_both_traits() {
    let mut random = OsRandom::over(Dry);
    let mut id = [0u8; 16];
    assert_eq!(
        NoticeRandom::fill(&mut random, &mut id),
        Err(NotifyError::EntropyUnavailable),
        "no notice id is minted from a source that failed"
    );
    let mut salt = [0u8; 16];
    assert_eq!(
        PushRandom::fill(&mut random, &mut salt),
        Err(WebPushError::Random),
        "no web push is sealed from a source that failed"
    );
    assert_eq!(
        (id, salt),
        ([0u8; 16], [0u8; 16]),
        "and no bytes are made up"
    );
}

/// Live: the kernel's generator fills a whole buffer, and two draws differ.
#[test]
fn the_kernel_source_fills_a_whole_buffer_and_two_draws_differ() {
    let source = System::new();
    let (mut first, mut second) = ([0u8; 32], [0u8; 32]);
    assert_eq!(
        source.fill(&mut first),
        Ok(()),
        "the kernel's generator answers"
    );
    assert_eq!(source.fill(&mut second), Ok(()), "and answers again");
    assert_ne!(first, [0u8; 32], "a draw fills its buffer");
    assert_ne!(first, second, "two 256-bit draws are distinct");
}

/// Live at the stub and after it: a source that cannot answer never yields a draw, on either trait.
#[test]
fn a_source_that_cannot_answer_never_yields_a_draw() {
    let mut random = OsRandom::over(Dry);
    assert!(NoticeRandom::fill(&mut random, &mut [0u8; 16]).is_err());
    assert!(PushRandom::fill(&mut random, &mut [0u8; 16]).is_err());
    assert_eq!(
        EntropyUnavailable.to_string(),
        "the operating system's random source is unavailable",
        "the error holds nothing the source said"
    );
}
