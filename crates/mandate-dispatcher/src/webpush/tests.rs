//! E8-10 slice D10b, E8-14 (DEC-2500 items 6 and 9, DEC-2502, DEC-2503, DEC-2504, DEC-2506): the
//! web-push provider over fake ports. Every expectation is written here from the spec and the
//! decisions, never read from the adapter: each status's outcome from DEC-729 as an independent
//! table, each text key's class pair from spec §3.2 and DEC-700 item 3, the 12-hour lifetime as
//! 43 200 s (DEC-790 item 4), and the body opened with the subscription's own private key on the
//! receiver's side of RFC 8291, written apart from `mandate_webpush`.

use super::{
    AddressBook, AddressRef, Clock, KeyId, Ports, PostFailure, PushAnswer, PushPost,
    RingVapidSigner, SEND_DEADLINE, StoredSubscription, VaultUnavailable, WebPushProvider,
    WebPushSwitch,
};
use crate::DispatchError;
use aes_gcm::Aes128Gcm;
use aes_gcm::aead::{Aead, KeyInit};
use base64ct::{Base64UrlUnpadded, Encoding};
use hkdf::Hkdf;
use mandate_notify::canary::{CANARIES, scan};
use mandate_notify::{
    AddressHandle, IdempotencyKey, NoticeId, Notification, NotifyError, Origin, Outcome, Provider,
    PushChannel, Reason, Recipient, SecureRandom as NoticeRandom, TextKey,
};
use mandate_time::UtcNanos;
use mandate_webpush::{
    DEFAULT_PUSH_ALLOWLIST, PushAllowlist, PushRequest, SecureRandom, Urgency, VapidSigner,
    VapidSubject, WebPushError,
};
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::elliptic_curve::sec1::ToSec1Point;
use p256::{PublicKey, SecretKey};
use proptest::prelude::*;
use proptest::test_runner::{Config as RunnerConfig, TestCaseError, TestRunner};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use secrecy::{SecretSlice, SecretString};
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::error::Error;
use std::rc::Rc;
use std::time::Duration;

type Checked = Result<(), Box<dyn Error>>;

/// A push endpoint on the default allowlist whose path is a canary (NT-2).
const ENDPOINT: &str = "https://fcm.googleapis.com/fcm/send/zqendpointcanary";
/// A well-formed `https` host on no allowlist.
const OFF_LIST: &str = "https://push.example.invalid/send/zqendpointcanary";
/// The subscription's `auth` secret, 16 octets that are themselves a canary.
const AUTH: [u8; 16] = *b"zqauthcanary0000";
/// The browser's private key; its public point is the subscription's `p256dh`.
const UA_PRIVATE: [u8; 32] = [0x11; 32];
/// The application server's VAPID private key, for the test signer only.
const VAPID_PRIVATE: [u8; 32] = [0x33; 32];
const KEY: &str = "0123456789abcdef";
const OTHER_KEY: &str = "fedcba9876543210";
const ROLE: &str = "mailto:push@example.invalid";
const PERSON: &str = "mailto:alice@example.invalid";
const RECIPIENT: &str = "p_01j8z3m4000000000000000001";
const ADDRESS: &str = "01J8Z3M4000000000000000002";
const NOW_S: i64 = 1_791_000_000;
const TWELVE_HOURS_S: u64 = 43_200;

/// What one step's POSTs were: each request and the deadline it was given.
#[derive(Default)]
struct Posted {
    requests: Vec<(PushRequest, Duration)>,
}

/// A push service that records each POST and gives one fixed answer.
struct Service {
    log: Rc<RefCell<Posted>>,
    answer: Result<PushAnswer, PostFailure>,
}

impl PushPost for Service {
    fn post(
        &mut self,
        request: &PushRequest,
        deadline: Duration,
    ) -> Result<PushAnswer, PostFailure> {
        self.log
            .borrow_mut()
            .requests
            .push((request.clone(), deadline));
        self.answer
    }
}

/// One vault entry as the fake vault holds it.
#[derive(Clone)]
struct Entry {
    endpoint: String,
    p256dh: Vec<u8>,
    auth: Vec<u8>,
    key_id: String,
}

/// A vault holding at most one entry, for [`RECIPIENT`] and [`ADDRESS`]. Each read records its
/// deadline and moves the clock on by `takes_s`.
struct Vault {
    entry: Option<Entry>,
    unavailable: bool,
    takes_s: i64,
    clock: Rc<Cell<UtcNanos>>,
    reads: Rc<RefCell<Vec<Duration>>>,
}

impl AddressBook for Vault {
    fn entry(
        &mut self,
        recipient: &Recipient,
        address: &AddressRef,
        deadline: Duration,
    ) -> Result<Option<StoredSubscription>, VaultUnavailable> {
        self.reads.borrow_mut().push(deadline);
        let at = self.clock.get();
        let later = UtcNanos::from_parts(at.secs().saturating_add(self.takes_s), at.nanos());
        self.clock.set(later.unwrap_or(at));
        if self.unavailable {
            return Err(VaultUnavailable);
        }
        let ours = Recipient::parse(RECIPIENT).is_ok_and(|r| r == *recipient)
            && *address == AddressRef::new(ADDRESS);
        Ok(self
            .entry
            .clone()
            .filter(|_| ours)
            .map(|e| StoredSubscription {
                endpoint: SecretString::from(e.endpoint),
                p256dh: SecretSlice::from(e.p256dh),
                auth: SecretSlice::from(e.auth),
                key_id: KeyId::new(&e.key_id),
            }))
    }
}

struct Ticking(Rc<Cell<UtcNanos>>);

impl Clock for Ticking {
    fn now(&self) -> UtcNanos {
        self.0.get()
    }
}

/// A deterministic random source, xorshift from a seed, that records every draw, or fails each.
struct Draws {
    state: u64,
    fails: bool,
    drawn: Rc<RefCell<Vec<Vec<u8>>>>,
}

impl SecureRandom for Draws {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), WebPushError> {
        if self.fails {
            return Err(WebPushError::Random);
        }
        for byte in out.iter_mut() {
            self.state ^= self.state.wrapping_shl(13);
            self.state ^= self.state.wrapping_shr(7);
            self.state ^= self.state.wrapping_shl(17);
            *byte = self.state.to_be_bytes().first().copied().unwrap_or(0);
        }
        self.drawn.borrow_mut().push(out.to_vec());
        Ok(())
    }
}

/// The notice-id source: each draw is 16 copies of the next byte.
struct Counter(u8);

impl NoticeRandom for Counter {
    fn fill(&mut self, bytes: &mut [u8; 16]) -> Result<(), NotifyError> {
        self.0 = self.0.wrapping_add(1);
        *bytes = [self.0; 16];
        Ok(())
    }
}

/// An ES256 signer on the RustCrypto side, independent of `ring`, that counts its signatures.
struct TestSigner {
    key: SigningKey,
    signed: Rc<Cell<u32>>,
}

impl VapidSigner for TestSigner {
    fn public_key(&self) -> [u8; 65] {
        let point = self.key.verifying_key().to_sec1_point(false);
        point.as_bytes().try_into().unwrap_or([0; 65])
    }
    fn sign_es256(&self, message: &[u8]) -> Result<[u8; 64], WebPushError> {
        self.signed.set(self.signed.get().saturating_add(1));
        let signature: Signature = self.key.sign(message);
        Ok(signature.to_bytes().into())
    }
}

/// The browser's key pair.
fn browser() -> Result<(SecretKey, Vec<u8>), Box<dyn Error>> {
    let private = SecretKey::from_slice(&UA_PRIVATE)?;
    let public = private
        .public_key()
        .to_sec1_point(false)
        .as_bytes()
        .to_vec();
    Ok((private, public))
}

fn vapid_key() -> Result<SigningKey, Box<dyn Error>> {
    Ok(SigningKey::from_slice(&VAPID_PRIVATE)?)
}

/// One provider's world: what each test sets.
struct Setup {
    entry: Option<Entry>,
    unavailable: bool,
    vault_takes_s: i64,
    answer: Result<PushAnswer, PostFailure>,
    random_fails: bool,
    seed: u64,
}

impl Setup {
    fn answering(answer: Result<PushAnswer, PostFailure>) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            entry: Some(Entry {
                endpoint: ENDPOINT.to_owned(),
                p256dh: browser()?.1,
                auth: AUTH.to_vec(),
                key_id: KEY.to_owned(),
            }),
            unavailable: false,
            vault_takes_s: 0,
            answer,
            random_fails: false,
            seed: 0x9e37_79b9_7f4a_7c15,
        })
    }

    fn accepting() -> Result<Self, Box<dyn Error>> {
        Self::answering(Ok(PushAnswer { status: 201 }))
    }
}

/// The provider and the fakes' shared records.
struct Rig {
    provider: WebPushProvider,
    posted: Rc<RefCell<Posted>>,
    reads: Rc<RefCell<Vec<Duration>>>,
    clock: Rc<Cell<UtcNanos>>,
    drawn: Rc<RefCell<Vec<Vec<u8>>>>,
    signed: Rc<Cell<u32>>,
    notices: Counter,
}

fn rigged(setup: Setup) -> Result<Rig, Box<dyn Error>> {
    let posted = Rc::new(RefCell::new(Posted::default()));
    let reads = Rc::new(RefCell::new(Vec::new()));
    let clock = Rc::new(Cell::new(UtcNanos::from_parts(NOW_S, 0)?));
    let drawn = Rc::new(RefCell::new(Vec::new()));
    let signed = Rc::new(Cell::new(0));
    let ports = Ports {
        post: Box::new(Service {
            log: Rc::clone(&posted),
            answer: setup.answer,
        }),
        book: Box::new(Vault {
            entry: setup.entry,
            unavailable: setup.unavailable,
            takes_s: setup.vault_takes_s,
            clock: Rc::clone(&clock),
            reads: Rc::clone(&reads),
        }),
        clock: Box::new(Ticking(Rc::clone(&clock))),
        random: Box::new(Draws {
            state: setup.seed,
            fails: setup.random_fails,
            drawn: Rc::clone(&drawn),
        }),
    };
    let signer = TestSigner {
        key: vapid_key()?,
        signed: Rc::clone(&signed),
    };
    let keys: Vec<(KeyId, Box<dyn VapidSigner>)> = vec![(KeyId::new(KEY), Box::new(signer))];
    let provider = WebPushProvider::new(
        ports,
        PushAllowlist::parse(&DEFAULT_PUSH_ALLOWLIST)?,
        VapidSubject::parse(ROLE)?,
        keys,
    )?;
    Ok(Rig {
        provider,
        posted,
        reads,
        clock,
        drawn,
        signed,
        notices: Counter(0),
    })
}

/// One send of a fresh notice carrying `text`, on `channel`, to [`ADDRESS`], through the
/// [`Provider`] the dispatcher's fan-out calls; the notice and the provider's own answer.
fn send_on(
    rig: &mut Rig,
    text: TextKey,
    channel: PushChannel,
) -> Result<(Notification, Result<Outcome, NotifyError>), Box<dyn Error>> {
    let notification = Notification {
        notice: NoticeId::mint(&mut rig.notices)?,
        text,
    };
    let handle = AddressHandle {
        recipient: Recipient::parse(RECIPIENT)?,
        channel,
    };
    let key = IdempotencyKey::of(&notification.notice, &handle)?;
    let origin = Origin::parse("https://app.example.invalid")?;
    let address = AddressRef::new(ADDRESS);
    let answer = rig
        .provider
        .to(&address)?
        .send(&origin, &notification, &handle, &key);
    Ok((notification, answer))
}

fn send(
    rig: &mut Rig,
    text: TextKey,
) -> Result<(Notification, Result<Outcome, NotifyError>), Box<dyn Error>> {
    send_on(rig, text, PushChannel::WebPush)
}

fn posts(rig: &Rig) -> usize {
    rig.posted.borrow().requests.len()
}

/// DEC-729, typed here from the decision: a 2xx is accepted, a 3xx, 404 and 410 mark the address,
/// 400, 401 and 403 are the deployment's fault, 413 is too large, 429 is rate limited, and every
/// other status, every 5xx included, is retried as a provider error.
fn dec_729(status: u16) -> Option<Outcome> {
    let permanent = |reason| Some(Outcome::Permanent { reason });
    let retryable = |reason| Some(Outcome::Retryable { reason });
    if (200..300).contains(&status) {
        return None;
    }
    if (300..400).contains(&status) || status == 404 || status == 410 {
        return permanent(Reason::AddressRejected);
    }
    match status {
        400 | 401 | 403 => permanent(Reason::ProviderError),
        413 => permanent(Reason::TooLarge),
        429 => retryable(Reason::RateLimited),
        _ => retryable(Reason::ProviderError),
    }
}

/// Spec §3.2's class for each text key, and DEC-700 item 3's pair for that class.
fn envelope(text: TextKey) -> (Urgency, u32) {
    match text {
        TextKey::ApprovalNeeded => (Urgency::High, 3_600),
        TextKey::AttentionNeeded | TextKey::AccountChanged => (Urgency::High, 86_400),
        TextKey::BriefReady => (Urgency::Normal, 21_600),
    }
}

/// The receiver's side of RFC 8291 §3 and RFC 8188 §2, with the browser's private key.
fn open(body: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let salt = body.get(..16).ok_or("salt")?;
    let as_public = body.get(21..86).ok_or("keyid")?;
    let sealed = body.get(86..).ok_or("record")?;
    let (ua, ua_public) = browser()?;
    let sender = PublicKey::from_sec1_bytes(as_public)?;
    let shared = p256::ecdh::diffie_hellman(ua.to_nonzero_scalar(), sender.as_affine());
    let info = [b"WebPush: info\0".as_slice(), &ua_public, as_public].concat();
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&AUTH), shared.raw_secret_bytes())
        .expand(&info, &mut ikm)
        .map_err(|_| "ikm")?;
    let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
        .map_err(|_| "cek")?;
    hk.expand(b"Content-Encoding: nonce\0", &mut nonce)
        .map_err(|_| "nonce")?;
    Ok(Aes128Gcm::new_from_slice(&cek)?
        .decrypt(&nonce.into(), sealed)
        .map_err(|_| "the body does not open with the subscription's key")?)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn b64(text: &str) -> Vec<u8> {
    Base64UrlUnpadded::decode_vec(text).unwrap_or_default()
}

/// The `t=` token and `k=` key of a VAPID header, and the token's header, claims, and signature.
fn vapid(authorization: &str) -> (String, String, String, String, String) {
    let (token, key) = authorization
        .strip_prefix("vapid t=")
        .and_then(|rest| rest.split_once(", k="))
        .unwrap_or_default();
    let (signed, signature) = token.rsplit_once('.').unwrap_or_default();
    let (head, claims) = signed.split_once('.').unwrap_or_default();
    let claims = String::from_utf8(b64(claims)).unwrap_or_default();
    (
        signed.to_owned(),
        key.to_owned(),
        String::from_utf8(b64(head)).unwrap_or_default(),
        claims,
        signature.to_owned(),
    )
}

/// A live test, true before and after the adapter is built, so the mutation gate has a test to
/// judge this module by (DEC-139): web push is off unless the configuration turns it on (DEC-2504
/// item 4).
#[test]
fn web_push_is_off_unless_turned_on() {
    assert_eq!(WebPushSwitch::default(), WebPushSwitch::Off);
    assert_eq!(SEND_DEADLINE, Duration::from_secs(10), "DEC-2500 item 6");
}

/// A live test: a push service's answer is its status and nothing else, so no header, the
/// `Location` that names a resource under the endpoint included, can reach a record (DEC-2500
/// item 9). The literal names every field, so one added fails to compile here.
#[test]
fn a_push_answer_is_its_status_alone() {
    let PushAnswer { status } = PushAnswer { status: 201 };
    assert_eq!(status, 201);
    assert_eq!(
        format!("{:?}", PushAnswer { status }),
        "PushAnswer { status: 201 }"
    );
}

/// A live test (NT-2, DEC-2502 item 8, DEC-2503 item 4): no type that holds an endpoint, a
/// subscription key, a VAPID key, or a signed request derives or implements `Debug` or `Display`.
#[test]
fn no_type_holding_an_address_or_a_key_prints() -> std::io::Result<()> {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/webpush.rs"))?;
    for (declared, name) in [
        ("pub struct StoredSubscription {", "StoredSubscription"),
        ("pub struct Ports {", "Ports"),
        ("pub struct RingVapidSigner {", "RingVapidSigner"),
        ("pub struct WebPushProvider {", "WebPushProvider"),
        ("pub struct Addressed<'a> {", "Addressed"),
    ] {
        assert!(src.contains(declared), "{declared} is declared");
        let above = src.split(declared).next().unwrap_or_default().lines().rev();
        let attributes: String = above
            .take_while(|l| ["///", "#[", "    ", ")]"].iter().any(|p| l.starts_with(p)))
            .filter(|l| l.starts_with("#[derive"))
            .collect();
        assert!(!attributes.contains("Debug"), "{name} derives no Debug");
        for printer in ["Debug", "Display"] {
            assert!(
                !src.contains(&format!("{printer} for {name}")),
                "{name} implements no {printer}"
            );
        }
    }
    Ok(())
}

#[test]
fn off_withholds_every_web_push_pair_and_on_keeps_the_audience() -> Checked {
    let audience = [
        ("p_a", PushChannel::WebPush),
        ("p_a", PushChannel::Email),
        ("p_b", PushChannel::WebPush),
        ("p_b", PushChannel::Sms),
    ];
    assert_eq!(
        WebPushSwitch::Off.sends(&audience)?,
        [("p_a", PushChannel::Email), ("p_b", PushChannel::Sms)],
        "off: every pair but web push, in the audience's order"
    );
    assert_eq!(WebPushSwitch::On.sends(&audience)?, audience);
    assert_eq!(
        WebPushSwitch::Off.sends(&[("p_a", PushChannel::WebPush)])?,
        []
    );
    Ok(())
}

/// NT-1 (spec §4.2, §4.6, DEC-790 item 7): the body opens with the subscription's own key to
/// exactly `{"notice","text"}`, the closed pair `web/public/push-sw.js` reads, then its delimiter
/// and zeros to 128 octets; the request goes once, to the stored endpoint unchanged, in its class's
/// fixed envelope (DEC-700 item 3), and is accepted.
#[test]
fn an_accepted_push_carries_exactly_the_closed_pair_to_the_subscription() -> Checked {
    for text in [
        TextKey::ApprovalNeeded,
        TextKey::AttentionNeeded,
        TextKey::AccountChanged,
        TextKey::BriefReady,
    ] {
        let mut rig = rigged(Setup::accepting()?)?;
        let (notification, answer) = send(&mut rig, text)?;
        assert!(matches!(answer?, Outcome::Accepted { .. }), "{text:?}");
        let posted = rig.posted.borrow();
        let [(request, _)] = posted.requests.as_slice() else {
            return Err(format!("{text:?}: one POST, not {}", posted.requests.len()).into());
        };
        assert_eq!(request.endpoint, ENDPOINT, "the address exactly as stored");
        assert_eq!((request.urgency, request.ttl_s), envelope(text), "{text:?}");
        let record = open(&request.body)?;
        let json = format!(
            r#"{{"notice":"{}","text":"{}"}}"#,
            notification.notice.hex()?,
            text.key()?
        );
        let mut expected = [json.as_bytes(), &[2]].concat();
        expected.resize(128, 0);
        assert_eq!(record, expected, "{text:?}: the closed pair, padded");
        assert_eq!(scan(&record)?, Vec::<&str>::new());
    }
    Ok(())
}

/// Spec §4.6, DEC-726 item 3, DEC-790 item 4, DEC-2503 items 4 and 6: every attempt signs its own
/// header with that attempt's time, `exp` exactly 12 hours on, `aud` the push service's origin, and
/// `sub` the deployment's role mailbox, and the signature verifies against the key the subscription
/// was made under.
#[test]
fn each_attempt_signs_a_fresh_header_with_the_role_mailbox_and_twelve_hours() -> Checked {
    let mut rig = rigged(Setup::accepting()?)?;
    let mut headers = Vec::new();
    for later_s in [0i64, 90] {
        let at = UtcNanos::from_parts(NOW_S.saturating_add(later_s), 0)?;
        rig.clock.set(at);
        let (_, answer) = send(&mut rig, TextKey::AttentionNeeded)?;
        assert!(matches!(answer?, Outcome::Accepted { .. }));
        let posted = rig.posted.borrow();
        let (request, _) = posted.requests.last().ok_or("a POST")?;
        let (signed, key, head, claims, signature) = vapid(&request.authorization);
        let exp = u64::try_from(at.secs())?.saturating_add(TWELVE_HOURS_S);
        assert_eq!(head, r#"{"typ":"JWT","alg":"ES256"}"#);
        assert_eq!(
            claims,
            format!(r#"{{"aud":"https://fcm.googleapis.com","exp":{exp},"sub":"{ROLE}"}}"#)
        );
        let held = vapid_key()?.verifying_key().to_sec1_point(false);
        assert_eq!(
            b64(&key),
            held.as_bytes(),
            "k= is the key the address names"
        );
        let verifying = VerifyingKey::from_sec1_bytes(&b64(&key))?;
        let signature = Signature::from_slice(&b64(&signature))?;
        assert!(verifying.verify(signed.as_bytes(), &signature).is_ok());
        headers.push(request.authorization.clone());
    }
    assert_eq!(rig.signed.get(), 2, "one signature per attempt");
    assert_ne!(headers.first(), headers.last(), "no header is reused");
    Ok(())
}

/// DEC-2503 item 6, DEC-727: a direct send is held to the role-mailbox rule too, so a provider
/// with a person's address, or an `https:` subject, is never built.
#[test]
fn a_subject_that_is_not_a_role_mailbox_builds_no_provider() -> Checked {
    for subject in [PERSON, "https://example.invalid/contact", ROLE] {
        let ports = Ports {
            post: Box::new(Service {
                log: Rc::default(),
                answer: Ok(PushAnswer { status: 201 }),
            }),
            book: Box::new(Vault {
                entry: None,
                unavailable: false,
                takes_s: 0,
                clock: Rc::new(Cell::new(UtcNanos::from_parts(NOW_S, 0)?)),
                reads: Rc::default(),
            }),
            clock: Box::new(Ticking(Rc::new(Cell::new(UtcNanos::from_parts(NOW_S, 0)?)))),
            random: Box::new(Draws {
                state: 7,
                fails: false,
                drawn: Rc::default(),
            }),
        };
        let built = WebPushProvider::new(
            ports,
            PushAllowlist::parse(&DEFAULT_PUSH_ALLOWLIST)?,
            VapidSubject::parse(subject)?,
            Vec::new(),
        );
        match (subject, built) {
            (ROLE, built) => assert!(built.is_ok(), "the role mailbox builds"),
            (_, Err(refused)) => assert_eq!(
                refused,
                DispatchError::WebPush(WebPushError::InvalidSubject),
                "{subject}"
            ),
            (_, Ok(_)) => return Err(format!("{subject} built a provider").into()),
        }
    }
    Ok(())
}

/// Spec §4.6, DEC-792 items 1 and 4: an address on a host off the allowlist is never posted to,
/// and its attempt is `permanent { address_rejected }`, which marks it.
#[test]
fn an_endpoint_off_the_allowlist_is_address_rejected_with_no_post() -> Checked {
    for endpoint in [
        OFF_LIST,
        "https://fcm.googleapis.com:8443/fcm/send/x",
        "http://fcm.googleapis.com/fcm/send/x",
    ] {
        let mut setup = Setup::accepting()?;
        if let Some(entry) = setup.entry.as_mut() {
            entry.endpoint = endpoint.to_owned();
        }
        let mut rig = rigged(setup)?;
        let (_, answer) = send(&mut rig, TextKey::AttentionNeeded)?;
        assert_eq!(
            answer?,
            Outcome::Permanent {
                reason: Reason::AddressRejected
            },
            "{endpoint}"
        );
        assert_eq!(posts(&rig), 0, "{endpoint}: nothing posted");
    }
    Ok(())
}

/// DEC-729 through `push_status`, against the independent table: each status a push service can
/// answer is the outcome the decision gives it, a `2xx` carrying a message id.
#[test]
fn each_status_is_dec_729s_outcome() -> Checked {
    for status in [
        0u16, 100, 199, 200, 201, 202, 204, 299, 300, 301, 302, 304, 307, 308, 399, 400, 401, 402,
        403, 404, 405, 408, 409, 410, 411, 412, 413, 414, 418, 428, 429, 430, 451, 499, 500, 502,
        503, 504, 599, 600, 999, 65_535,
    ] {
        let mut rig = rigged(Setup::answering(Ok(PushAnswer { status }))?)?;
        let (_, answer) = send(&mut rig, TextKey::ApprovalNeeded)?;
        let answer = answer?;
        match dec_729(status) {
            None => assert!(
                matches!(&answer, Outcome::Accepted { provider_message_id } if provider_message_id.len() == 32),
                "{status}: {answer:?}"
            ),
            Some(expected) => assert_eq!(answer, expected, "{status}"),
        }
        assert_eq!(posts(&rig), 1, "{status}: one POST, no redirect followed");
    }
    Ok(())
}

/// Spec §5.2, §5.9: a POST past its deadline is `retryable { timeout }`, and a push service that
/// could not be reached is the relay's `unreachable`, `retryable { timeout }` too (DEC-728,
/// DEC-2506 item 3). Neither marks the address.
#[test]
fn a_push_service_that_does_not_answer_is_retried_as_a_timeout() -> Checked {
    for failure in [PostFailure::Timeout, PostFailure::Unreachable] {
        let mut rig = rigged(Setup::answering(Err(failure))?)?;
        let (_, answer) = send(&mut rig, TextKey::AttentionNeeded)?;
        assert_eq!(
            answer?,
            Outcome::Retryable {
                reason: Reason::Timeout
            },
            "{failure:?}"
        );
        assert_eq!(posts(&rig), 1);
    }
    Ok(())
}

/// DEC-2500 item 6, DEC-2502 items 4 and 7: the vault read and the POST share one 10-second
/// deadline. The vault is given all of it; the POST what the vault left. A vault that spends it
/// all is `retryable { provider_error }`, with no POST.
#[test]
fn the_vault_read_and_the_post_share_one_ten_second_deadline() -> Checked {
    let mut setup = Setup::accepting()?;
    setup.vault_takes_s = 3;
    let mut rig = rigged(setup)?;
    let (_, answer) = send(&mut rig, TextKey::AttentionNeeded)?;
    assert!(matches!(answer?, Outcome::Accepted { .. }));
    assert_eq!(*rig.reads.borrow(), [Duration::from_secs(10)]);
    let posted = rig.posted.borrow();
    let deadlines: Vec<Duration> = posted.requests.iter().map(|(_, d)| *d).collect();
    assert_eq!(deadlines, [Duration::from_secs(7)], "what the vault left");
    drop(posted);
    for spent_s in [10, 11] {
        let mut setup = Setup::accepting()?;
        setup.vault_takes_s = spent_s;
        let mut rig = rigged(setup)?;
        let (_, answer) = send(&mut rig, TextKey::AttentionNeeded)?;
        assert_eq!(
            answer?,
            Outcome::Retryable {
                reason: Reason::ProviderError
            },
            "a vault read of {spent_s} s"
        );
        assert_eq!(posts(&rig), 0, "nothing posted after {spent_s} s");
    }
    Ok(())
}

/// Spec §5.2, DEC-2502 item 7, DEC-712: a vault that does not answer is
/// `retryable { provider_error }` and marks nothing; only its definite answer that no entry exists
/// is `permanent { address_missing }`. Neither posts anything.
#[test]
fn a_vault_that_does_not_answer_is_retried_and_a_missing_entry_is_address_missing() -> Checked {
    let mut down = Setup::accepting()?;
    down.unavailable = true;
    let mut rig = rigged(down)?;
    let (_, answer) = send(&mut rig, TextKey::AttentionNeeded)?;
    assert_eq!(
        answer?,
        Outcome::Retryable {
            reason: Reason::ProviderError
        }
    );
    assert_eq!(posts(&rig), 0);
    let mut gone = Setup::accepting()?;
    gone.entry = None;
    let mut rig = rigged(gone)?;
    let (_, answer) = send(&mut rig, TextKey::AttentionNeeded)?;
    assert_eq!(
        answer?,
        Outcome::Permanent {
            reason: Reason::AddressMissing
        }
    );
    assert_eq!(posts(&rig), 0);
    Ok(())
}

/// DEC-2503 item 5: an address whose key this process does not hold is the deployment's fault,
/// `permanent { provider_error }`, which marks nothing, and nothing is signed or posted.
#[test]
fn a_key_the_process_does_not_hold_is_provider_error_with_no_post() -> Checked {
    let mut setup = Setup::accepting()?;
    if let Some(entry) = setup.entry.as_mut() {
        entry.key_id = OTHER_KEY.to_owned();
    }
    let mut rig = rigged(setup)?;
    let (_, answer) = send(&mut rig, TextKey::ApprovalNeeded)?;
    assert_eq!(
        answer?,
        Outcome::Permanent {
            reason: Reason::ProviderError
        }
    );
    assert_eq!((posts(&rig), rig.signed.get()), (0, 0));
    Ok(())
}

/// DEC-2506 item 3: subscription keys the vault holds that do not parse (a `p256dh` that is not
/// an uncompressed point, an `auth` that is not 16 octets) are `permanent { provider_error }`,
/// marking nothing, and nothing is posted.
#[test]
fn subscription_keys_that_do_not_parse_are_provider_error_with_no_post() -> Checked {
    let (_, good) = browser()?;
    let short_auth = AUTH.get(..15).ok_or("auth")?.to_vec();
    for (p256dh, auth) in [
        (vec![4u8; 65], AUTH.to_vec()),
        (good.get(..33).ok_or("point")?.to_vec(), AUTH.to_vec()),
        (good, short_auth),
    ] {
        let mut setup = Setup::accepting()?;
        if let Some(entry) = setup.entry.as_mut() {
            entry.p256dh = p256dh;
            entry.auth = auth;
        }
        let mut rig = rigged(setup)?;
        let (_, answer) = send(&mut rig, TextKey::AttentionNeeded)?;
        assert_eq!(
            answer?,
            Outcome::Permanent {
                reason: Reason::ProviderError
            }
        );
        assert_eq!(posts(&rig), 0);
    }
    Ok(())
}

/// DEC-2506 item 2: a handle on any channel but web push reaching this provider is a fault of the
/// deployment's wiring, `permanent { provider_error }`, with no vault read and no POST.
#[test]
fn a_handle_on_another_channel_is_provider_error_with_no_vault_read() -> Checked {
    for channel in PushChannel::ALL
        .into_iter()
        .filter(|c| *c != PushChannel::WebPush)
    {
        let mut rig = rigged(Setup::accepting()?)?;
        let (_, answer) = send_on(&mut rig, TextKey::AttentionNeeded, channel)?;
        assert_eq!(
            answer?,
            Outcome::Permanent {
                reason: Reason::ProviderError
            },
            "{channel:?}"
        );
        assert_eq!((rig.reads.borrow().len(), posts(&rig)), (0, 0));
    }
    Ok(())
}

/// DEC-2500 item 9, spec §5.2: each accepted attempt's message id is 32 lowercase hex digits of a
/// fresh 128-bit draw from the provider's secure random source, never anything the push service
/// said and nothing derived from the address.
#[test]
fn the_message_id_is_a_fresh_random_draw() -> Checked {
    let mut rig = rigged(Setup::accepting()?)?;
    let mut ids = Vec::new();
    for _ in 0..3 {
        let (_, answer) = send(&mut rig, TextKey::BriefReady)?;
        let Outcome::Accepted {
            provider_message_id,
        } = answer?
        else {
            return Err("accepted".into());
        };
        ids.push(provider_message_id);
    }
    let draws: Vec<String> = rig
        .drawn
        .borrow()
        .iter()
        .filter(|d| d.len() == 16)
        .map(|d| hex(d))
        .collect();
    for id in &ids {
        assert!(
            id.len() == 32
                && id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{id}"
        );
        assert!(
            draws.contains(id),
            "{id} is one of the source's 16-octet draws"
        );
        assert!(!ENDPOINT.contains(id.as_str()));
    }
    let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "one id per attempt");
    Ok(())
}

/// DEC-2500 item 2, spec §5.9: a random source that fails sends nothing and journals nothing: the
/// send is not formed, so the step ends and the next one tries again.
#[test]
fn a_random_source_that_fails_sends_nothing() -> Checked {
    let mut setup = Setup::accepting()?;
    setup.random_fails = true;
    let mut rig = rigged(setup)?;
    let (_, answer) = send(&mut rig, TextKey::AttentionNeeded)?;
    assert_eq!(answer, Err(NotifyError::EntropyUnavailable));
    assert_eq!(posts(&rig), 0);
    Ok(())
}

/// Every secret one send handles, in each form it could be printed in: the endpoint, the
/// subscription's `p256dh` and `auth`, the VAPID private key, and the signed header.
fn secrets(authorization: &[String]) -> Result<Vec<String>, Box<dyn Error>> {
    let (_, p256dh) = browser()?;
    let mut held = vec![
        "zqendpointcanary".to_owned(),
        "zqauthcanary".to_owned(),
        hex(&p256dh),
        Base64UrlUnpadded::encode_string(&p256dh),
        hex(&AUTH),
        Base64UrlUnpadded::encode_string(&AUTH),
        hex(&VAPID_PRIVATE),
        Base64UrlUnpadded::encode_string(&VAPID_PRIVATE),
        format!("{VAPID_PRIVATE:?}"),
        format!("{AUTH:?}"),
    ];
    for header in authorization {
        let (signed, _, _, _, signature) = vapid(header);
        held.extend([header.clone(), signed, signature]);
    }
    Ok(held)
}

fn leaks(captured: &str, secrets: &[String]) -> Vec<String> {
    secrets
        .iter()
        .filter(|s| !s.is_empty() && captured.contains(s.as_str()))
        .cloned()
        .collect()
}

/// NT-2 (DEC-2502 item 8, DEC-2503 item 4): a canary endpoint, canary keys, and a canary VAPID key
/// driven through every failure of the adapter (a refused connection, a timeout, a redirect, an
/// oversized answer, a vault outage, a missing entry, an endpoint off the list, a key not held, a
/// random source that fails) and a refused start; every outcome and error, `Debug` and `Display`,
/// is scanned for them, and for NT-1's canaries.
#[test]
fn no_address_key_or_header_reaches_any_outcome_or_error() -> Checked {
    let mut captured = String::new();
    let mut headers = Vec::new();
    let mut setups = Vec::new();
    for answer in [
        Err(PostFailure::Unreachable),
        Err(PostFailure::Timeout),
        Ok(PushAnswer { status: 302 }),
        Ok(PushAnswer { status: 413 }),
        Ok(PushAnswer { status: 201 }),
    ] {
        setups.push(Setup::answering(answer)?);
    }
    let mut down = Setup::accepting()?;
    down.unavailable = true;
    let mut gone = Setup::accepting()?;
    gone.entry = None;
    let mut off_list = Setup::accepting()?;
    let mut unknown_key = Setup::accepting()?;
    if let (Some(off), Some(unknown)) = (off_list.entry.as_mut(), unknown_key.entry.as_mut()) {
        off.endpoint = OFF_LIST.to_owned();
        unknown.key_id = OTHER_KEY.to_owned();
    }
    let mut dry = Setup::accepting()?;
    dry.random_fails = true;
    setups.extend([down, gone, off_list, unknown_key, dry]);
    for setup in setups {
        let mut rig = rigged(setup)?;
        let (_, answer) = send(&mut rig, TextKey::ApprovalNeeded)?;
        match &answer {
            Ok(outcome) => captured.push_str(&format!("{outcome:?}")),
            Err(error) => captured.push_str(&format!("{error} {error:?}")),
        }
        let posted = rig.posted.borrow();
        headers.extend(posted.requests.iter().map(|(r, _)| r.authorization.clone()));
    }
    let refused = WebPushProvider::new(
        Ports {
            post: Box::new(Service {
                log: Rc::default(),
                answer: Ok(PushAnswer { status: 201 }),
            }),
            book: Box::new(Vault {
                entry: None,
                unavailable: true,
                takes_s: 0,
                clock: Rc::new(Cell::new(UtcNanos::from_parts(NOW_S, 0)?)),
                reads: Rc::default(),
            }),
            clock: Box::new(Ticking(Rc::new(Cell::new(UtcNanos::from_parts(NOW_S, 0)?)))),
            random: Box::new(Draws {
                state: 3,
                fails: false,
                drawn: Rc::default(),
            }),
        },
        PushAllowlist::parse(&DEFAULT_PUSH_ALLOWLIST)?,
        VapidSubject::parse(PERSON)?,
        Vec::new(),
    );
    if let Err(error) = refused {
        captured.push_str(&format!("{error} {error:?}"));
    }
    for printed in [
        format!("{:?}", AddressRef::new(ADDRESS)),
        format!("{:?}", KeyId::new(KEY)),
        format!("{VaultUnavailable} {VaultUnavailable:?}"),
        format!("{} {:?}", PostFailure::Timeout, PostFailure::Unreachable),
    ] {
        captured.push_str(&printed);
    }
    assert!(!headers.is_empty(), "the scan saw signed headers");
    assert_eq!(leaks(&captured, &secrets(&headers)?), Vec::<String>::new());
    assert_eq!(scan(captured.as_bytes())?, Vec::<&str>::new());
    assert!(CANARIES.iter().all(|c| !captured.contains(c)));
    Ok(())
}

/// One generated case: a status, a text key, a vault answer, and the random seed.
fn case() -> impl Strategy<Value = (u16, usize, u8, u64)> {
    let status = prop_oneof![
        any::<u16>(),
        prop::sample::select(vec![
            200u16, 201, 202, 301, 302, 400, 401, 403, 404, 410, 413, 429, 500, 503
        ]),
    ];
    (status, 0usize..4, 0u8..4, 1u64..u64::MAX)
}

/// The property behind NT-2 and DEC-729 over random statuses, text keys, vault answers, and
/// subscriptions' random draws: each attempt's outcome is the independent table's, no POST follows
/// a vault that did not answer or an entry that is gone, and no outcome or error prints a secret.
#[test]
fn any_status_and_vault_answer_maps_by_the_decisions_and_prints_no_secret() -> Checked {
    let texts = [
        TextKey::ApprovalNeeded,
        TextKey::AttentionNeeded,
        TextKey::AccountChanged,
        TextKey::BriefReady,
    ];
    let mut runner = TestRunner::new(RunnerConfig {
        cases: 96,
        failure_persistence: None,
        ..RunnerConfig::default()
    });
    let fail = |e: Box<dyn Error>| TestCaseError::fail(e.to_string());
    let checked = runner.run(&case(), |(status, text, vault, seed)| {
        let text = *texts.get(text).ok_or_else(|| TestCaseError::fail("text"))?;
        let mut setup = Setup::answering(Ok(PushAnswer { status })).map_err(fail)?;
        setup.seed = seed;
        match vault {
            1 => setup.unavailable = true,
            2 => setup.entry = None,
            _ => {}
        }
        let mut rig = rigged(setup).map_err(fail)?;
        let (_, answer) = send(&mut rig, text).map_err(fail)?;
        let answer = answer.map_err(|e| TestCaseError::fail(e.to_string()))?;
        let expected = match vault {
            1 => Some(Outcome::Retryable {
                reason: Reason::ProviderError,
            }),
            2 => Some(Outcome::Permanent {
                reason: Reason::AddressMissing,
            }),
            _ => dec_729(status),
        };
        match &expected {
            None => prop_assert!(
                matches!(&answer, Outcome::Accepted { provider_message_id } if provider_message_id.len() == 32),
                "{status}: {answer:?}"
            ),
            Some(expected) => prop_assert_eq!(&answer, expected),
        }
        let posted = rig.posted.borrow();
        let reached = usize::from(vault == 0 || vault == 3);
        prop_assert_eq!(posted.requests.len(), reached);
        for (request, _) in &posted.requests {
            prop_assert_eq!((request.urgency, request.ttl_s), envelope(text));
        }
        let headers: Vec<String> = posted
            .requests
            .iter()
            .map(|(r, _)| r.authorization.clone())
            .collect();
        let printed = format!("{answer:?}");
        let secrets = secrets(&headers).map_err(fail)?;
        prop_assert_eq!(leaks(&printed, &secrets), Vec::<String>::new());
        Ok(())
    });
    checked.map_err(|e| e.to_string())?;
    Ok(())
}

/// DEC-2503 items 1, 3 and 4: the signer reads a PKCS #8 P-256 document as the keygen command
/// writes it (`ring`'s `generate_pkcs8`), names its key by the first 16 lowercase hex digits of the
/// SHA-256 of its public point, and signs ES256 `r || s` that verifies on the RustCrypto side; a
/// document that is not such a key is refused with the closed `Signer` code, holding none of it.
#[test]
fn the_ring_signer_signs_es256_and_names_its_key_by_its_public_point() -> Checked {
    let document =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .map_err(|_| "keygen")?;
    let pair = EcdsaKeyPair::from_pkcs8(
        &ECDSA_P256_SHA256_FIXED_SIGNING,
        document.as_ref(),
        &SystemRandom::new(),
    )
    .map_err(|_| "parse")?;
    let public = pair.public_key().as_ref().to_vec();
    let signer = RingVapidSigner::from_pkcs8(SecretSlice::from(document.as_ref().to_vec()))?;
    assert_eq!(signer.public_key().as_slice(), public.as_slice());
    let digest = hex(&Sha256::digest(&public));
    let want = KeyId::new(digest.get(..16).ok_or("16 digits")?);
    assert_eq!(signer.key_id()?, &want);
    let message = b"eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiJ9.e30";
    let signature = Signature::from_slice(&signer.sign_es256(message)?)?;
    let verifying = VerifyingKey::from_sec1_bytes(&public)?;
    assert!(
        verifying.verify(message, &signature).is_ok(),
        "ES256 r || s"
    );
    let foreign = [document.as_ref().get(..40).ok_or("prefix")?, &[0u8; 8]].concat();
    for refused in [Vec::new(), foreign, vec![0x30; 138]] {
        let held = hex(&refused);
        let answer = RingVapidSigner::from_pkcs8(SecretSlice::from(refused)).map(|_| ());
        assert_eq!(answer, Err(DispatchError::WebPush(WebPushError::Signer)));
        assert!(held.is_empty() || !format!("{answer:?}").contains(&held));
    }
    Ok(())
}
