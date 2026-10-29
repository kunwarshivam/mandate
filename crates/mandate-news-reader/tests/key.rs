#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests: a wrong answer should stop the test"
)]
//! The news data key, read once (DEC-2277 items 3 and 4, DEC-2280 item 6): the vault's answer to
//! the one read decides `no_key`, `vault_unavailable`, or the key's own refusal, and no refusal,
//! error, or `Debug` output carries a byte of the key.

use std::cell::RefCell;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use mandate_connections::record::ConnectionId;
use mandate_news_reader::{KeyRefusal, read_key, read_then_end, stored_key};
use mandate_sources::fetch::NewsKeyRefusal;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::lease::Lease;
use mandate_vault::policy::{Identity, PlatformSecret, SecretPath};
use mandate_vault::source::{Grant, SecretSource};
use secrecy::{ExposeSecret, SecretSlice};

/// A paper key, as `mandate vault put news-data-key` stores it.
const PAPER_KEY: &[u8] = b"PKTESTKEYID0001:topsecret0001";

fn stored(bytes: &[u8]) -> Result<Option<SecretSlice<u8>>, SourceError> {
    Ok(Some(SecretSlice::from(bytes.to_vec())))
}

#[test]
fn the_vault_s_answer_decides_the_key_or_why_there_is_none() {
    assert_eq!(stored_key(Ok(None)).err(), Some(KeyRefusal::NoKey));
    assert_eq!(
        stored_key(Err(SourceError::Missing)).err(),
        Some(KeyRefusal::NoKey)
    );
    for outage in [
        SourceError::Unavailable,
        SourceError::Denied,
        SourceError::Sealed,
        SourceError::Malformed,
        SourceError::Integrity,
        SourceError::RoleMissing,
    ] {
        let named = outage.code();
        assert_eq!(
            stored_key(Err(outage)).err(),
            Some(KeyRefusal::VaultUnavailable),
            "{named} is an outage, not a refusal of the key"
        );
    }
    assert_eq!(
        stored_key(stored(b"AKLIVE0001:secret")).err(),
        Some(KeyRefusal::Refused(NewsKeyRefusal::NotPaper))
    );
    assert_eq!(
        stored_key(stored(b"PKKEY no colon")).err(),
        Some(KeyRefusal::Refused(NewsKeyRefusal::Malformed))
    );
    let key = stored_key(stored(PAPER_KEY)).unwrap();
    assert_eq!(key.key_id().expose_secret(), "PKTESTKEYID0001");
    assert_eq!(key.secret().expose_secret(), "topsecret0001");
}

#[test]
fn each_refusal_is_its_code_alone() {
    let codes = [
        (KeyRefusal::NoKey, "no_key"),
        (KeyRefusal::VaultUnavailable, "vault_unavailable"),
        (KeyRefusal::Refused(NewsKeyRefusal::NotPaper), "not_paper"),
        (
            KeyRefusal::Refused(NewsKeyRefusal::Malformed),
            "news_key_malformed",
        ),
    ];
    for (refusal, code) in codes {
        assert_eq!(refusal.code(), code);
    }
}

#[test]
fn no_byte_of_the_key_reaches_a_refusal_an_error_or_debug_output() {
    let live = b"AKLIVEKEYID9:livesecret9";
    let shown = [
        format!("{:?}", stored_key(stored(PAPER_KEY))),
        format!("{:?}", stored_key(stored(live))),
        format!("{:?}", stored_key(stored(b"PKSPLIT:a:b"))),
        stored_key(stored(live)).err().unwrap().code().to_owned(),
    ];
    for text in shown {
        for part in [
            "PKTESTKEYID0001",
            "topsecret0001",
            "AKLIVEKEYID9",
            "livesecret9",
            "PKSPLIT",
        ] {
            assert!(!text.contains(part), "{part} in {text}");
        }
    }
}

/// A vault session as the news reader that answers every read with `answer` and records each path
/// it is asked for; it refuses everything else the trait offers.
struct Recorder {
    identity: Identity,
    answer: fn() -> Result<Option<SecretSlice<u8>>, SourceError>,
    asked: RefCell<Vec<SecretPath>>,
    /// How many reads had been made at each end of the session, in order.
    ended: RefCell<Vec<usize>>,
    /// What ending the session answers.
    end_answer: fn() -> Result<(), SourceError>,
}

impl Recorder {
    fn answering(answer: fn() -> Result<Option<SecretSlice<u8>>, SourceError>) -> Self {
        Self {
            identity: Identity::NewsReader,
            answer,
            asked: RefCell::new(Vec::new()),
            ended: RefCell::new(Vec::new()),
            end_answer: || Ok(()),
        }
    }

    /// Ends the session, as `OpenBaoSource::end_session` revokes its token.
    async fn end(&self) -> Result<(), SourceError> {
        self.ended.borrow_mut().push(self.asked.borrow().len());
        (self.end_answer)()
    }
}

impl SecretSource for Recorder {
    fn identity(&self) -> &Identity {
        &self.identity
    }
    async fn put(&self, _: &SecretPath, _: &SecretSlice<u8>) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }
    async fn read(&self, path: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError> {
        self.asked.borrow_mut().push(path.clone());
        (self.answer)()
    }
    async fn delete(&self, _: &SecretPath) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }
    async fn lease(&self, _: &ConnectionId, _: UtcNanos) -> Result<Lease, SourceError> {
        Err(SourceError::Denied)
    }
    async fn renew(&self, _: &Lease, _: UtcNanos) -> Result<Lease, SourceError> {
        Err(SourceError::Denied)
    }
    async fn revoke(&self, _: Lease) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }
    async fn issue_grant(&self, _: &ConnectionId, _: UtcNanos) -> Result<Grant, SourceError> {
        Err(SourceError::Denied)
    }
    async fn revoke_grant(&self, _: &Grant) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }
}

/// Runs a future the recorder answers at once to its end.
fn now<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("the recorder answers at once"),
    }
}

/// The one read is of `platform/news-data-key` and nothing else, and its answer decides the key:
/// the stored paper key, `no_key` for a missing one, `vault_unavailable` for an outage.
#[test]
fn the_key_is_read_once_from_the_news_data_key_s_path() {
    let vault = Recorder::answering(|| stored(PAPER_KEY));
    let key = now(read_key(&vault)).unwrap();
    assert_eq!(key.key_id().expose_secret(), "PKTESTKEYID0001");
    assert_eq!(
        *vault.asked.borrow(),
        [SecretPath::Platform(PlatformSecret::NewsDataKey)],
        "exactly one read, of the news data key"
    );
    let missing = Recorder::answering(|| Err(SourceError::Missing));
    assert_eq!(now(read_key(&missing)).err(), Some(KeyRefusal::NoKey));
    assert_eq!(missing.asked.borrow().len(), 1);
    let outage = Recorder::answering(|| Err(SourceError::Unavailable));
    assert_eq!(
        now(read_key(&outage)).err(),
        Some(KeyRefusal::VaultUnavailable)
    );
    assert_eq!(outage.asked.borrow().len(), 1);
}

/// E2-22: the key is read once and the session is then ended once, after the read, whatever the
/// read answered, so the token outlives the read by no more than one request. A session the vault
/// does not end is `vault_unavailable`, and the key read through it is dropped.
#[test]
fn the_key_is_read_once_and_the_session_ended_once_after_whatever_the_read_answered() {
    let vault = Recorder::answering(|| stored(PAPER_KEY));
    let key = now(read_then_end(&vault, async |v: &Recorder| v.end().await)).unwrap();
    assert_eq!(key.key_id().expose_secret(), "PKTESTKEYID0001");
    assert_eq!(vault.asked.borrow().len(), 1, "one read");
    assert_eq!(*vault.ended.borrow(), [1], "one end, after the read");
    for (answer, refusal) in [
        (
            (|| Err(SourceError::Missing)) as fn() -> _,
            KeyRefusal::NoKey,
        ),
        (|| Ok(None), KeyRefusal::NoKey),
        (
            || Err(SourceError::Unavailable),
            KeyRefusal::VaultUnavailable,
        ),
        (
            || stored(b"AKLIVEKEY:secret"),
            KeyRefusal::Refused(NewsKeyRefusal::NotPaper),
        ),
    ] {
        let vault = Recorder::answering(answer);
        let got = now(read_then_end(&vault, async |v: &Recorder| v.end().await));
        assert_eq!(got.err(), Some(refusal));
        assert_eq!(vault.asked.borrow().len(), 1, "{refusal:?}: one read");
        assert_eq!(
            *vault.ended.borrow(),
            [1],
            "{refusal:?}: ended all the same"
        );
    }
    let mut unended = Recorder::answering(|| stored(PAPER_KEY));
    unended.end_answer = || Err(SourceError::Unavailable);
    let got = now(read_then_end(&unended, async |v: &Recorder| v.end().await));
    assert_eq!(
        got.err(),
        Some(KeyRefusal::VaultUnavailable),
        "a session the vault did not end drops the key read through it"
    );
    assert_eq!(*unended.ended.borrow(), [1]);
}

/// The crate's non-test source, file by file: each `src/` file up to its `#[cfg(test)] mod tests`,
/// so a pin never matches its own text.
fn non_test_sources() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            let code = text
                .split("#[cfg(test)]\nmod tests")
                .next()
                .unwrap_or_default()
                .to_owned();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                code,
            )
        })
        .collect()
}

/// E2-22, R2 on #2901: the one call of `read_then_end` outside the tests ends the session with
/// the real source's own `end_session`, so a no-op end, which every test of `read_then_end` would
/// pass, cannot ship: the vault token would then live out its whole lifetime.
#[test]
fn the_vault_s_own_end_session_is_the_one_end_the_reader_ever_passes() {
    let sources = non_test_sources();
    assert!(
        sources
            .iter()
            .any(|(name, code)| name == "key.rs" && code.contains("pub async fn vault_key")),
        "the pin reads the code it pins"
    );
    let calls: Vec<(&str, usize)> = sources
        .iter()
        .map(|(name, code)| (name.as_str(), code.matches("read_then_end(").count()))
        .filter(|(_, n)| *n > 0)
        .collect();
    assert_eq!(
        calls,
        [("key.rs", 1)],
        "one call of read_then_end, in key.rs"
    );
    let key = &sources.iter().find(|(name, _)| name == "key.rs").unwrap().1;
    assert_eq!(
        key.matches("read_then_end(&vault, OpenBaoSource::end_session)")
            .count(),
        1,
        "the session is ended by OpenBaoSource::end_session"
    );
}
