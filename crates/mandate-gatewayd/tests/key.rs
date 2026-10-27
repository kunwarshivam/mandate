//! The model provider's key, read once at start (DEC-2020 items 3 and 5, E21-62): only the model
//! gateway's vault session reads it, it is read once however many calls follow, it is served for
//! its upstream alone, a vault that cannot give it stops the start with its own code, a value a
//! header cannot carry is refused, and no `Debug`, error or code carries a byte of it.

use std::cell::Cell;

use mandate_connections::record::ConnectionId;
use mandate_gateway::provider::{SecretError, SecretSource};
use mandate_gatewayd::key::{HeldKey, KeyError};
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::lease::Lease;
use mandate_vault::policy::{Identity, PlatformSecret, SecretPath};
use mandate_vault::source::{Grant, SecretSource as Vault};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use secrecy::SecretSlice;

const UPSTREAM: &str = "anthropic";
const CANARY: &str = "sk-or-v1-canary-not-a-real-key-7f3a";

/// A vault session as `identity` whose one read answers `answer`, counting reads.
struct FakeVault {
    identity: Identity,
    answer: Result<Option<Vec<u8>>, SourceError>,
    reads: Cell<u32>,
}

impl FakeVault {
    fn holding(identity: Identity, answer: Result<Option<Vec<u8>>, SourceError>) -> Self {
        Self {
            identity,
            answer,
            reads: Cell::new(0),
        }
    }

    fn with_key(key: &[u8]) -> Self {
        Self::holding(Identity::ModelGateway, Ok(Some(key.to_vec())))
    }
}

#[allow(
    clippy::manual_async_fn,
    reason = "the vault's trait declares async methods; the fake answers each without awaiting"
)]
impl Vault for FakeVault {
    fn identity(&self) -> &Identity {
        &self.identity
    }

    async fn put(&self, _path: &SecretPath, _secret: &SecretSlice<u8>) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }

    async fn read(&self, path: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError> {
        assert_eq!(
            *path,
            SecretPath::Platform(PlatformSecret::ModelProviderKey),
            "the gateway reads its own secret and no other"
        );
        self.reads.set(self.reads.get() + 1);
        self.answer
            .clone()
            .map(|stored| stored.map(SecretSlice::from))
    }

    async fn delete(&self, _path: &SecretPath) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }

    async fn lease(
        &self,
        _connection: &ConnectionId,
        _now: UtcNanos,
    ) -> Result<Lease, SourceError> {
        Err(SourceError::Denied)
    }

    async fn renew(&self, _lease: &Lease, _now: UtcNanos) -> Result<Lease, SourceError> {
        Err(SourceError::Denied)
    }

    async fn revoke(&self, _lease: Lease) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }

    async fn issue_grant(
        &self,
        _connection: &ConnectionId,
        _now: UtcNanos,
    ) -> Result<Grant, SourceError> {
        Err(SourceError::Denied)
    }

    async fn revoke_grant(&self, _grant: &Grant) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }
}

fn held(vault: &FakeVault) -> Result<HeldKey, KeyError> {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(HeldKey::read(vault, UPSTREAM))
}

const ERRORS: [KeyError; 5] = [
    KeyError::Identity,
    KeyError::Missing,
    KeyError::Denied,
    KeyError::Unavailable,
    KeyError::Malformed,
];

/// DEC-2020 item 3: the key is read once, then served for every call to its upstream from memory,
/// and for no other upstream.
#[test]
fn the_key_is_read_once_and_served_for_its_upstream_alone() {
    let vault = FakeVault::with_key(CANARY.as_bytes());
    let key = held(&vault).unwrap();
    for _ in 0..5 {
        assert_eq!(key.provider_key(UPSTREAM).unwrap().expose_secret(), CANARY);
    }
    assert_eq!(vault.reads.get(), 1, "one read at start, none per call");
    for other in ["openai", "", "anthropic ", "Anthropic"] {
        assert_eq!(
            key.provider_key(other),
            Err(SecretError::Missing),
            "{other:?}"
        );
    }
}

/// DEC-2020 item 5: only a session as the model gateway reads the key; any other identity is
/// refused before the vault is asked.
#[test]
fn only_the_model_gateway_s_session_reads_the_key() {
    for identity in [
        Identity::Runtime,
        Identity::SecretImport,
        Identity::EmailSender,
        Identity::Api,
    ] {
        let vault = FakeVault::holding(identity.clone(), Ok(Some(CANARY.as_bytes().to_vec())));
        assert_eq!(
            held(&vault).map(|_| ()),
            Err(KeyError::Identity),
            "{identity:?}"
        );
        assert_eq!(vault.reads.get(), 0, "{identity:?} never reads");
    }
}

/// DEC-2020 item 3: a vault that cannot give the key stops the start, each case with its own code:
/// no key, the policy's refusal, and an outage of any kind, never a refusal mistaken for one.
#[test]
fn a_vault_that_cannot_give_the_key_stops_the_start_with_its_code() {
    let cases = [
        (Ok(None), KeyError::Missing),
        (Err(SourceError::Missing), KeyError::Missing),
        (Err(SourceError::Denied), KeyError::Denied),
        (Err(SourceError::Unavailable), KeyError::Unavailable),
        (Err(SourceError::Sealed), KeyError::Unavailable),
        (Err(SourceError::Malformed), KeyError::Unavailable),
        (Err(SourceError::Integrity), KeyError::Unavailable),
    ];
    for (answer, expected) in cases {
        let vault = FakeVault::holding(Identity::ModelGateway, answer.clone());
        assert_eq!(held(&vault).map(|_| ()), Err(expected), "{answer:?}");
    }
    let codes: Vec<&str> = ERRORS.iter().map(|error| error.code()).collect();
    assert_eq!(
        codes,
        [
            "model_key_identity",
            "model_key_missing",
            "model_key_denied",
            "model_key_unavailable",
            "model_key_malformed",
        ]
    );
}

/// The key goes into a request header: a stored value is held only when it is non-empty printable
/// ASCII, so a newline can never smuggle a second header and a stray byte never reaches the wire.
#[test]
fn a_stored_value_a_header_cannot_carry_is_refused() {
    let values = proptest::collection::vec(any::<u8>(), 0..48);
    TestRunner::new(Config {
        cases: 256,
        failure_persistence: None,
        ..Config::default()
    })
    .run(&values, |stored| {
        let printable =
            !stored.is_empty() && stored.iter().all(|byte| (0x21..=0x7e).contains(byte));
        let read = held(&FakeVault::with_key(&stored));
        if printable {
            let key = read.unwrap().provider_key(UPSTREAM).unwrap();
            prop_assert_eq!(key.expose_secret().as_bytes(), stored.as_slice());
        } else {
            prop_assert_eq!(read.map(|_| ()), Err(KeyError::Malformed));
        }
        Ok(())
    })
    .unwrap();
    for refused in [
        format!("{CANARY}\r\nX-Injected: 1"),
        format!(" {CANARY}"),
        format!("{CANARY}\n"),
        String::new(),
    ] {
        assert_eq!(
            held(&FakeVault::with_key(refused.as_bytes())).map(|_| ()),
            Err(KeyError::Malformed),
            "{refused:?}"
        );
    }
}

/// E21-62: the key's bytes appear in no `Debug` of the held key or of what it hands out, and in no
/// error's text, `Debug` or code, whichever way the read went.
#[test]
fn the_key_appears_in_no_debug_error_or_code() {
    let key = held(&FakeVault::with_key(CANARY.as_bytes())).unwrap();
    assert_eq!(
        format!("{key:?}"),
        r#"HeldKey { upstreams: {"anthropic"}, .. }"#,
        "the held key shows its upstreams and nothing of the key"
    );
    let mut shown = vec![
        format!("{key:?}"),
        format!("{:?}", key.provider_key(UPSTREAM).unwrap()),
        format!("{:?}", key.provider_key("other")),
    ];
    for error in ERRORS {
        shown.extend([
            error.to_string(),
            format!("{error:?}"),
            error.code().to_owned(),
        ]);
    }
    let malformed = held(&FakeVault::with_key(format!("{CANARY}\n").as_bytes())).unwrap_err();
    shown.extend([malformed.to_string(), format!("{malformed:?}")]);
    for text in shown {
        assert!(!text.contains(CANARY), "the key shows in {text:?}");
        assert!(
            !text.contains("canary"),
            "part of the key shows in {text:?}"
        );
    }
}

/// DEC-2216: the live registry's entries may route through more than one upstream, so the key is
/// served for each upstream the start names and still for no other, from the one read.
#[test]
fn the_key_is_served_for_each_upstream_named_and_no_other() {
    let vault = FakeVault::with_key(CANARY.as_bytes());
    let key = held(&vault).unwrap().with_upstream("google-vertex/global");
    for upstream in [UPSTREAM, "google-vertex/global"] {
        assert_eq!(key.provider_key(upstream).unwrap().expose_secret(), CANARY);
    }
    assert_eq!(key.provider_key("azure"), Err(SecretError::Missing));
    assert_eq!(vault.reads.get(), 1);
}
