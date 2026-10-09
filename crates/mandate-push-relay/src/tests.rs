//! E8-14 slice S8b. Every verdict is written here from spec §4.6 and DEC-724, never read from the
//! relay: the class pairs from DEC-700 item 3, the cap as the literal 512, and endpoint tables.

use super::{
    Forward, LogEntry, PushAnswer, PushService, RelayError, RelayId, RelayLog, RelayRequest,
    check_authorization, relay,
};
use mandate_notify::canary::{CANARIES, scan};
use mandate_webpush::{
    DEFAULT_PUSH_ALLOWLIST, PushAllowlist, PushEndpoint, Urgency, VapidSigner, VapidSubject,
    WebPushError, vapid_authorization,
};
use proptest::prelude::*;

const ID: &str = "0123456789abcdef0123456789abcdef";
const FCM: &str = "https://fcm.googleapis.com/fcm/send/dGVzdA";
const EVIL: &str = "https://evil.example/x";
const ACTION: (&str, u32) = ("high", 3_600);
const SAFETY: (&str, u32) = ("high", 86_400);
const INFO: (&str, u32) = ("normal", 21_600);
const PAIRS: [(&str, u32); 3] = [ACTION, SAFETY, INFO];
const OK: Answer = Ok(201);
#[rustfmt::skip]
const ALLOWED: [&str; 5] = [
    FCM, "https://fcm.googleapis.com:443/fcm/send/x", "https://web.push.apple.com/QGuQ",
    "https://updates.push.services.mozilla.com/wpush/v2/x", "https://wns2-bl2p.notify.windows.com/w/?token=x",
];
#[rustfmt::skip]
const REFUSED: [&str; 9] = [
    "http://fcm.googleapis.com/x", "https://evil.example/x", "https://push.apple.com/x",
    "https://fcm.googleapis.com:8443/x", "https://u@fcm.googleapis.com/x", "https://127.0.0.1/x",
    "https://FCM.googleapis.com/x", "https://fcm.googleapis.com./x", "",
];
#[rustfmt::skip]
const BAD_IDS: [&str; 6] = [
    "123456789abcdef0123456789abcdef", "0123456789abcdef0123456789abcdef0", "zqinstrument", "",
    "0123456789ABCDEF0123456789ABCDEF", "01J8ZNB0M000000000000000K1",
];

/// One captured `POST`: endpoint, urgency, TTL, body.
type Post = (String, &'static str, u32, Vec<u8>);
type Answer = Result<u16, RelayError>;

struct Pushes(Vec<Post>, PushAnswer);
impl PushService for Pushes {
    fn post(&mut self, f: &Forward<'_>) -> PushAnswer {
        #[rustfmt::skip]
        self.0.push((f.endpoint.into(), f.urgency.as_str(), f.ttl_s, f.body.into()));
        self.1
    }
}

impl RelayLog for Vec<LogEntry> {
    fn record(&mut self, entry: LogEntry) {
        self.push(entry);
    }
}

#[derive(Debug, Clone)]
struct Wire(String, String, String, u32, Vec<u8>);

fn body(len: usize) -> Vec<u8> {
    (0..=255u8).cycle().take(len).collect()
}

fn wire(endpoint: &str, (urgency, ttl): (&str, u32), len: usize) -> Wire {
    Wire(ID.into(), endpoint.into(), urgency.into(), ttl, body(len))
}

/// The one log entry a request leaves: its id when it parses, and its answer.
fn entry(relay_id: &str, answer: Answer) -> LogEntry {
    let relay_id = RelayId::parse(relay_id).ok();
    LogEntry { relay_id, answer }
}

/// Sends `w` through `pushes` and `log`; a stub's report ends the test (DEC-77). The default list
/// always parses; if it did not, every endpoint would be refused, so that reads `AddressRejected`.
fn send(w: &Wire, pushes: &mut Pushes, log: &mut Vec<LogEntry>) -> Result<Answer, RelayError> {
    send_with(w, &header(B, SIG, KEY), pushes, log)
}

/// [`send`] with `authorization` as the header; [`send`] adds a valid one (DEC-726 item 6).
fn send_with(
    w: &Wire,
    authorization: &str,
    push: &mut dyn PushService,
    log: &mut Vec<LogEntry>,
) -> Result<Answer, RelayError> {
    let allowlist =
        PushAllowlist::parse(&DEFAULT_PUSH_ALLOWLIST).map_err(|_| RelayError::AddressRejected)?;
    #[rustfmt::skip]
    let request = RelayRequest { relay_id: &w.0, endpoint: &w.1, urgency: &w.2, ttl_s: w.3, authorization, ciphertext: &w.4 };
    match relay(&request, &allowlist, push, log) {
        Err(stub @ RelayError::Unimplemented { .. }) => Err(stub),
        answer => Ok(answer),
    }
}

fn run(w: &Wire, answer: PushAnswer) -> Result<(Answer, Vec<Post>, Vec<LogEntry>), RelayError> {
    let (mut pushes, mut log) = (Pushes(Vec::new(), answer), Vec::new());
    let answer = send(w, &mut pushes, &mut log)?;
    Ok((answer, pushes.0, log))
}

/// A refused request: the reason, no `POST`, and one log entry with the id when it parsed.
fn refused(w: &Wire, reason: RelayError) -> Result<(), RelayError> {
    let (answer, posts, log) = run(w, PushAnswer::Status(201))?;
    let expected = (Err(reason), 0, vec![entry(&w.0, Err(reason))]);
    assert_eq!((answer, posts.len(), log), expected, "{:?}", (&w.2, w.3));
    Ok(())
}

/// A valid request is posted once, exactly as given, and answers the push service's status, a
/// `3xx` included; a push service that does not answer is `unreachable`, never retried (NT-9).
#[test]
fn a_valid_request_is_posted_once_and_answers_the_status_or_unreachable() -> Result<(), RelayError>
{
    let statuses = [201, 308, 410, 429].map(|s| (PushAnswer::Status(s), Ok(s)));
    for (push, expected) in statuses
        .into_iter()
        .chain([(PushAnswer::Unreachable, Err(RelayError::Unreachable))])
    {
        for (pair, endpoint) in PAIRS.into_iter().flat_map(|p| ALLOWED.map(|e| (p, e))) {
            let (answer, posts, log) = run(&wire(endpoint, pair, 230), push)?;
            assert_eq!(answer, expected);
            assert_eq!(posts, [(endpoint.to_owned(), pair.0, pair.1, body(230))]);
            assert_eq!(log, [entry(ID, answer)]);
        }
    }
    Ok(())
}

#[test]
fn only_an_allowlisted_endpoint_of_at_most_2048_octets_is_forwarded() -> Result<(), RelayError> {
    let fill = |total: usize| format!("{FCM}{}", "a".repeat(total.saturating_sub(FCM.len())));
    for endpoint in ALLOWED.map(str::to_owned).into_iter().chain([fill(2_048)]) {
        let (answer, posts, _) = run(&wire(&endpoint, INFO, 230), PushAnswer::Status(201))?;
        assert_eq!((answer, posts.len()), (OK, 1), "{endpoint:.60}");
    }
    for endpoint in REFUSED.map(str::to_owned).into_iter().chain([fill(2_049)]) {
        refused(&wire(&endpoint, INFO, 230), RelayError::AddressRejected)?;
    }
    Ok(())
}

#[test]
fn a_relay_id_is_exactly_32_lowercase_hex_digits() -> Result<(), RelayError> {
    RelayId::parse(ID)?;
    RelayId::parse(&"f".repeat(32))?;
    for bad in BAD_IDS {
        assert_eq!(RelayId::parse(bad), Err(RelayError::InvalidRelayId));
        let w = Wire(bad.into(), FCM.into(), "high".into(), 3_600, body(230));
        refused(&w, RelayError::InvalidRelayId)?;
    }
    Ok(())
}

/// DEC-724 item 8, written by hand: faults `i` (relay id), `e` (endpoint), `v` (envelope) and `s`
/// (size) in one request, and the refusal that must win, the first in that order.
#[rustfmt::skip]
const FIRST_FAULT: [(&str, RelayError); 9] = [
    ("ie", RelayError::InvalidRelayId), ("ev", RelayError::AddressRejected),
    ("vs", RelayError::InvalidEnvelope), ("is", RelayError::InvalidRelayId),
    ("es", RelayError::AddressRejected), ("iv", RelayError::InvalidRelayId),
    ("evs", RelayError::AddressRejected), ("ivs", RelayError::InvalidRelayId),
    ("ievs", RelayError::InvalidRelayId),
];

/// Every refusal posts nothing and leaves one log entry, with the id only when it parsed.
#[test]
fn with_several_faults_the_first_in_the_fixed_order_is_the_refusal() -> Result<(), RelayError> {
    for (faults, first) in FIRST_FAULT {
        let has = |fault| faults.contains(fault);
        let id = if has('i') { "zqrule" } else { ID };
        let endpoint = if has('e') { EVIL } else { FCM };
        let (urgency, ttl) = if has('v') { ("low", 1) } else { SAFETY };
        let len = if has('s') { 513 } else { 230 };
        let w = Wire(id.into(), endpoint.into(), urgency.into(), ttl, body(len));
        refused(&w, first)?;
    }
    Ok(())
}

/// The two octet counts either side of the cap, forced rather than only sampled.
#[test]
fn exactly_512_bytes_is_forwarded_once_and_513_is_refused_too_large() -> Result<(), RelayError> {
    let (answer, posts, log) = run(&wire(FCM, SAFETY, 512), PushAnswer::Status(201))?;
    assert_eq!(answer, OK);
    assert_eq!(posts, [(FCM.to_owned(), SAFETY.0, SAFETY.1, body(512))]);
    assert_eq!(log, [entry(ID, OK)]);
    refused(&wire(FCM, SAFETY, 513), RelayError::TooLarge)
}

fn urgencies() -> impl Strategy<Value = &'static str> {
    prop::sample::select(["high", "normal", "low", "very-low", "High", "", " high"].to_vec())
}

fn ttls() -> impl Strategy<Value = u32> {
    let edges = prop::sample::select(vec![0, 1, 3_599, 3_600, 21_600, 86_400, 86_401]);
    prop_oneof![edges, any::<u32>()]
}

fn wires() -> impl Strategy<Value = Wire> {
    let endpoints = prop::sample::select(ALLOWED.into_iter().chain(REFUSED).collect::<Vec<_>>());
    let ids = prop::sample::select(vec![ID, "zqrule", "0123456789ABCDEF0123456789ABCDEF"]);
    (ids, endpoints, urgencies(), ttls(), 0usize..=600)
        .prop_map(|(id, e, u, t, len)| Wire(id.into(), e.into(), u.into(), t, body(len)))
}

proptest! {
    #[test]
    fn the_cap_admits_512_bytes_and_refuses_513_and_over(
        len in prop_oneof![Just(512usize), Just(513), 0usize..=2_048]
    ) {
        let (answer, posts, _) = run(&wire(FCM, SAFETY, len), PushAnswer::Status(201))?;
        let expected = if len <= 512 { OK } else { Err(RelayError::TooLarge) };
        prop_assert_eq!((answer, posts.len()), (expected, usize::from(len <= 512)));
    }

    #[test]
    fn only_one_class_pair_of_urgency_and_ttl_is_forwarded(urgency in urgencies(), ttl in ttls()) {
        let w = wire(FCM, (urgency, ttl), 230);
        if PAIRS.contains(&(urgency, ttl)) {
            let (answer, posts, _) = run(&w, PushAnswer::Status(201))?;
            prop_assert_eq!((answer, posts.len()), (OK, 1));
        } else {
            refused(&w, RelayError::InvalidEnvelope)?;
        }
    }

    /// Nothing is retained: a run of requests through one log and one push service leaves exactly
    /// what each request leaves alone, one log entry each, and no entry holds an address.
    #[test]
    fn nothing_carries_over_from_one_request_to_the_next(ws in prop::collection::vec(wires(), 1..8)) {
        let (mut pushes, mut log) = (Pushes(Vec::new(), PushAnswer::Status(201)), Vec::new());
        let (mut posts, mut entries, mut answers) = (Vec::new(), Vec::new(), Vec::new());
        for w in &ws {
            answers.push(send(w, &mut pushes, &mut log)?);
            let (answer, p, l) = run(w, PushAnswer::Status(201))?;
            prop_assert_eq!(answers.last(), Some(&answer));
            posts.extend(p);
            entries.extend(l);
        }
        prop_assert_eq!(&pushes.0, &posts);
        prop_assert_eq!(&log, &entries);
        prop_assert_eq!(log.len(), ws.len());
        let logged = format!("{log:?}{answers:?}");
        prop_assert!(ws.iter().all(|w| w.1.is_empty() || !logged.contains(&w.1)));
    }
}

/// Rule 6 and NT-1: everything the relay passes on (each `POST`, its log, its answers) holds only
/// the ciphertext as given, an allowlisted endpoint, a class's fixed pair, and opaque ids; with
/// those two stated exceptions (CP-1) set aside, the canary scan finds nothing.
#[test]
fn rule_6_the_relay_passes_on_only_the_body_the_endpoint_the_class_pair_and_opaque_ids()
-> Result<(), RelayError> {
    let mut ws: Vec<Wire> = PAIRS.into_iter().map(|p| wire(FCM, p, 230)).collect();
    let fields = [0, 1, 2].into_iter().cycle().zip(PAIRS.into_iter().cycle());
    for (canary, (field, pair)) in CANARIES.into_iter().zip(fields) {
        let mut w = wire(FCM, pair, 230);
        match field {
            0 => w.0 = format!("{canary}{}", ID.get(canary.len()..).unwrap_or_default()),
            1 => w.2 = canary.to_owned(),
            _ => w.1 = format!("https://{canary}.push.example/x"),
        }
        ws.push(w);
    }
    let (mut pushes, mut log) = (Pushes(Vec::new(), PushAnswer::Status(201)), Vec::new());
    let mut captured = String::new();
    for w in &ws {
        captured.push_str(&format!("{:?}", send(w, &mut pushes, &mut log)?));
    }
    assert_eq!(pushes.0.len(), 3, "only the three clean requests go out");
    for (endpoint, urgency, ttl, sent) in &pushes.0 {
        assert!(endpoint == FCM && *sent == body(230));
        assert!(PAIRS.contains(&(*urgency, *ttl)));
        captured.push_str(&format!("{urgency}{ttl}"));
    }
    captured.push_str(&format!("{log:?}"));
    assert!(!captured.contains("https://"), "no endpoint is logged");
    assert_eq!(scan(captured.as_bytes()), Ok(Vec::new()));
    Ok(())
}

#[rustfmt::skip]
const CODES: [(RelayError, &str); 7] = [
    (RelayError::Unimplemented { story: "E8-14" }, "unimplemented"), (RelayError::InvalidRelayId, "invalid_relay_id"),
    (RelayError::AddressRejected, "address_rejected"), (RelayError::InvalidEnvelope, "invalid_envelope"),
    (RelayError::TooLarge, "too_large"), (RelayError::Unreachable, "unreachable"),
    (RelayError::InvalidAuthorization, "invalid_authorization"),
];

#[test]
fn each_refusal_has_its_fixed_code() {
    let codes = CODES.map(|(error, _)| error.code());
    assert_eq!(codes, CODES.map(|(_, code)| code));
}

/// NT-3 and NT-9 at rung 1: the relay's only normal dependencies are the push types and the
/// error derive, so it holds no handle to the journal, the runtime, or any trading state.
#[test]
fn the_relay_depends_on_nothing_but_the_push_types() -> std::io::Result<()> {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))?;
    let deps = "\n[dependencies]\nmandate-webpush.workspace = true\nthiserror.workspace = true\n\n";
    assert!(manifest.contains(deps), "push types only");
    Ok(())
}

/// DEC-726 item 4's form, written by hand: the JWT header `vapid_authorization` writes, a claims
/// segment, and the signature's and key's lengths in unpadded base64url.
const A: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiJ9";
const B: &str = "eyJleHAiOjE0NTM1MjM3NjgsInN1YiI6Im1haWx0bzpwdXNoQGV4YW1wbGUuaW52YWxpZCJ9";
const SIG: usize = 86;
const KEY: usize = 87;
const FOUNDER: &str = "mailto:push@notify.owlhead.ai";
const NOW: u64 = 1_453_480_568;
const REFUSED_HEADER: Answer = Err(RelayError::InvalidAuthorization);

/// `len` base64url characters, every edge of the alphabet among them.
fn seg(len: usize) -> String {
    "-_09AZaz".chars().cycle().take(len).collect()
}

fn header(claims: &str, sig: usize, key: usize) -> String {
    format!("vapid t={A}.{claims}.{}, k={}", seg(sig), seg(key))
}

fn shortest() -> String {
    format!("vapid t=a.b.{}, k={}", seg(SIG), seg(KEY))
}

/// The shape check alone: `Ok(true)` passes and `Ok(false)` is `invalid_authorization`; a stub's
/// report, or any other answer, ends the test (DEC-77).
fn shaped(field: &str) -> Result<bool, RelayError> {
    match check_authorization(field) {
        Ok(()) => Ok(true),
        Err(RelayError::InvalidAuthorization) => Ok(false),
        Err(other) => Err(other),
    }
}

/// A push service that records the header of each `POST` and answers `201`.
struct Headers(Vec<String>);
impl PushService for Headers {
    fn post(&mut self, f: &Forward<'_>) -> PushAnswer {
        self.0.push(f.authorization.into());
        PushAnswer::Status(201)
    }
}

/// Sends `w` with `authorization` and expects `answer`: the header posted once exactly as given
/// when it is `201`, nothing posted otherwise, and one log entry that holds no part of it.
fn expect(w: &Wire, authorization: &str, answer: Answer) -> Result<(), RelayError> {
    let (mut seen, mut log) = (Headers(Vec::new()), Vec::new());
    let got = send_with(w, authorization, &mut seen, &mut log)?;
    let posted = answer.iter().map(|_| authorization.to_owned()).collect();
    let expected = (answer, posted, vec![entry(&w.0, answer)]);
    assert_eq!((got, seen.0, log), expected, "{authorization:.60}");
    Ok(())
}

/// DEC-726 item 4's refusals, each one bend of a well-formed header.
#[rustfmt::skip]
fn malformed() -> Vec<String> {
    let (t, k, good) = (format!("{A}.{B}.{}", seg(SIG)), seg(KEY), header(B, SIG, KEY));
    let (sig_less, key_less) = (seg(SIG.saturating_sub(1)), seg(KEY.saturating_sub(1)));
    let schemes = ["Vapid", "VAPID", "WebPush", "Bearer", "vapid ", "", " vapid"];
    let mut bad: Vec<String> = schemes.map(|scheme| format!("{scheme} t={t}, k={k}")).into();
    bad.extend([
        format!("vapidt={t}, k={k}"), format!("vapid T={t}, k={k}"), format!("vapid t={t},k={k}"),
        format!("vapid t={t},  k={k}"), format!("vapid t={t} k={k}"), format!("vapid k={k}, t={t}"),
        format!("vapid t={t}"), "vapid".into(), format!("vapid t={A}.{}, k={k}", seg(SIG)),
        format!("vapid t={A}.{B}.{B}.{}, k={k}", seg(SIG)), header("", SIG, KEY),
        format!("vapid t=.{B}.{}, k={k}", seg(SIG)), header(&format!("{B}="), SIG, KEY),
        header(&format!("{B}+"), SIG, KEY), header(&format!("{B}/"), SIG, KEY),
        header(&format!("{B}é"), SIG, KEY), format!("vapid t={A}.{B}.{sig_less}+, k={k}"),
        format!("vapid t={t}, k={key_less}/"), format!("vapid t={t}, k={key_less}="),
        header(B, 85, KEY), header(B, 87, KEY), header(B, SIG, 86), header(B, SIG, 88),
        format!(" {good}"), format!("{good} "), format!("{good}\r\n"), format!("x{good}"), format!("{good},"),
    ]);
    bad
}

/// DEC-726 item 5: the core takes an absent field as the empty one and refuses it, posting nothing.
#[test]
fn a_request_without_authorization_is_refused_and_nothing_is_posted() -> Result<(), RelayError> {
    assert!(!shaped("")?, "an empty field is refused");
    for pair in PAIRS {
        expect(&wire(FCM, pair, 230), "", REFUSED_HEADER)?;
    }
    Ok(())
}

#[test]
fn a_malformed_authorization_is_refused() -> Result<(), RelayError> {
    assert!(shaped(&header(B, SIG, KEY))?, "the table bends a good one");
    for bad in malformed() {
        assert!(!shaped(&bad)?, "{bad:.60}");
        expect(&wire(FCM, ACTION, 230), &bad, REFUSED_HEADER)?;
    }
    Ok(())
}

/// The parts DEC-726 item 4 fixes are 223 octets, so 801 octets of claims make exactly 1 024.
#[test]
fn an_authorization_over_1024_octets_is_refused() -> Result<(), RelayError> {
    let (at_cap, over) = (header(&seg(801), SIG, KEY), header(&seg(802), SIG, KEY));
    assert_eq!((at_cap.len(), over.len()), (1_024, 1_025));
    assert_eq!((shaped(&at_cap)?, shaped(&over)?), (true, false));
    expect(&wire(FCM, INFO, 230), &at_cap, OK)?;
    expect(&wire(FCM, INFO, 230), &over, REFUSED_HEADER)
}

#[test]
fn a_valid_authorization_is_forwarded_byte_for_byte() -> Result<(), RelayError> {
    for valid in [shortest(), header(B, SIG, KEY), header(&seg(801), SIG, KEY)] {
        assert!(shaped(&valid)?, "{valid:.60}");
        for pair in PAIRS {
            expect(&wire(FCM, pair, 512), &valid, OK)?;
        }
    }
    Ok(())
}

/// A signer with fixed bytes: the relay never verifies a signature (DEC-726 item 2).
struct FixedSigner;
impl VapidSigner for FixedSigner {
    fn public_key(&self) -> [u8; 65] {
        [4; 65]
    }
    fn sign_es256(&self, _: &[u8]) -> Result<[u8; 64], WebPushError> {
        Ok([0xfb; 64])
    }
}

/// An independent fixture: the writer's own header, for the founder's subject (DEC-790 item 4)
/// on a short and on a 253-octet allowlisted host, and for a 287-octet subject with the 253-octet
/// host and a 20-digit `exp`, is never refused by the cap.
#[test]
fn a_header_from_vapid_authorization_passes() -> Result<(), Box<dyn std::error::Error>> {
    let allowlist = PushAllowlist::parse(&DEFAULT_PUSH_ALLOWLIST)?;
    let [a, b, c] = ["a", "b", "c"].map(|l| l.repeat(63));
    let host = format!("{a}.{b}.{c}.{}.notify.windows.com", "d".repeat(42));
    let (longest, widest) = (
        format!("https://{host}/w"),
        format!("mailto:push@{}.invalid", "e".repeat(267)),
    );
    assert_eq!((host.len(), widest.len()), (253, 287));
    #[rustfmt::skip]
    let cases = [
        (FCM, FOUNDER, NOW), ("https://a.push.apple.com/x", FOUNDER, NOW), (&longest, FOUNDER, NOW),
        (&longest, &widest, 18_446_744_073_709_508_415),
    ];
    for (endpoint, subject, now) in cases {
        let endpoint = PushEndpoint::parse_allowed(endpoint, &allowlist)?;
        let made =
            vapid_authorization(&endpoint, &VapidSubject::parse(subject)?, &FixedSigner, now)?;
        assert!(made.len() <= 1_024, "{} octets", made.len());
        assert!(shaped(&made)?, "{made:.60}");
        expect(&wire(FCM, SAFETY, 230), &made, OK)?;
    }
    Ok(())
}

/// DEC-726 item 3, NT-1, CP-1: a header whose claims (`aud` and `sub`, so the decoded segment),
/// signature and key carry canaries reaches no log entry and no answer, forwarded or refused.
#[test]
fn the_authorization_never_reaches_the_log() -> Result<(), Box<dyn std::error::Error>> {
    let allowlist = PushAllowlist::parse(&DEFAULT_PUSH_ALLOWLIST)?;
    let endpoint =
        PushEndpoint::parse_allowed("https://zqinstrument.notify.windows.com/x", &allowlist)?;
    let subject = VapidSubject::parse(&format!("mailto:{}@example.invalid", CANARIES.join(".")))?;
    let made = vapid_authorization(&endpoint, &subject, &FixedSigner, NOW)?;
    let claims = made.split('.').nth(1).unwrap_or_default().to_owned();
    #[rustfmt::skip]
    let fill = |canary: &str, len| format!("{canary}{}", seg(len).get(canary.len()..).unwrap_or_default());
    let (sig, key) = (fill("zqthesis", SIG), fill("zqpersonal", KEY));
    let carried = format!("vapid t={A}.{claims}.{sig}, k={key}");
    assert!(shaped(&carried)?, "the canary header is well formed");
    let bent = format!("{carried} ");
    #[rustfmt::skip]
    let requests = [
        (wire(FCM, SAFETY, 230), &carried), (wire(FCM, SAFETY, 513), &carried),
        (wire(FCM, SAFETY, 230), &bent), (wire(EVIL, SAFETY, 230), &carried),
    ];
    let (mut seen, mut log, mut captured) = (Headers(Vec::new()), Vec::new(), String::new());
    for (w, authorization) in requests {
        let answer = send_with(&w, authorization, &mut seen, &mut log)?;
        captured.push_str(&format!("{answer:?}"));
        if let Err(e) = answer {
            captured.push_str(&format!("{e} {e:?} {}", e.code()));
        }
    }
    assert_eq!(seen.0, [carried.as_str()], "only the clean one goes out");
    captured.push_str(&format!("{log:?}"));
    assert!(
        !captured.contains(&claims) && !captured.contains("vapid"),
        "no header is kept"
    );
    assert_eq!(scan(captured.as_bytes())?, Vec::<&str>::new());
    Ok(())
}

/// DEC-726 item 6: the header is checked after every DEC-724 check, so each earlier fault keeps
/// its refusal beside a bad header, and alone the bad header is the refusal.
#[test]
fn authorization_is_checked_last() -> Result<(), RelayError> {
    #[rustfmt::skip]
    let faults = [
        (Wire(ID.into(), FCM.into(), "high".into(), 3_600, body(513)), Err(RelayError::TooLarge)),
        (Wire(ID.into(), FCM.into(), "low".into(), 1, body(230)), Err(RelayError::InvalidEnvelope)),
        (Wire(ID.into(), EVIL.into(), "high".into(), 3_600, body(230)), Err(RelayError::AddressRejected)),
        (Wire("zqrule".into(), FCM.into(), "high".into(), 3_600, body(230)), Err(RelayError::InvalidRelayId)),
        (wire(FCM, ACTION, 230), REFUSED_HEADER),
    ];
    for bad in [String::new(), "vapid".into(), header(B, 85, KEY)] {
        assert!(!shaped(&bad)?, "{bad:.60}");
        for (w, first) in &faults {
            expect(w, &bad, *first)?;
        }
    }
    Ok(())
}

/// DEC-726 item 7: the ciphertext's 512 and the endpoint's 2 048 hold beside a header at its cap,
/// which lowers neither, and beside the shortest header, which raises neither.
#[test]
fn the_caps_do_not_share_room() -> Result<(), RelayError> {
    let fill = |total: usize| format!("{FCM}{}", "a".repeat(total.saturating_sub(FCM.len())));
    for h in [header(&seg(801), SIG, KEY), shortest()] {
        assert!(shaped(&h)?, "{h:.60}");
        expect(&wire(&fill(2_048), INFO, 512), &h, OK)?;
        expect(
            &wire(&fill(2_048), INFO, 513),
            &h,
            Err(RelayError::TooLarge),
        )?;
        expect(
            &wire(&fill(2_049), INFO, 512),
            &h,
            Err(RelayError::AddressRejected),
        )?;
    }
    Ok(())
}

/// DEC-726 item 2: each forward carries its own request's header, never one the relay adds.
#[test]
fn the_relay_adds_no_authorization_of_its_own() -> Result<(), RelayError> {
    let (first, second) = (header(B, SIG, KEY), shortest());
    assert!(shaped(&first)? && shaped(&second)?);
    let (mut seen, mut log) = (Headers(Vec::new()), Vec::new());
    for h in [&first, &second, &first] {
        assert_eq!(
            send_with(&wire(FCM, ACTION, 230), h, &mut seen, &mut log)?,
            OK
        );
    }
    assert_eq!(seen.0, [first.as_str(), &second, &first]);
    Ok(())
}

/// DEC-726 item 7 at rung 1: `RelayRequest` and `Forward` hold DEC-724's fields and
/// `authorization` alone (an exhaustive pattern stops compiling on any other), and neither
/// derives or implements `Debug` (NT-2).
#[test]
#[rustfmt::skip]
fn a_request_and_a_forward_gain_only_authorization_and_no_debug() -> std::io::Result<()> {
    let RelayRequest { relay_id: _, endpoint: _, urgency: _, ttl_s: _, authorization: _, ciphertext: _ } =
        RelayRequest { relay_id: ID, endpoint: FCM, urgency: "high", ttl_s: 3_600, authorization: "", ciphertext: &[] };
    let Forward { endpoint: _, urgency: _, ttl_s: _, authorization: _, body: _ } =
        Forward { endpoint: FCM, urgency: Urgency::High, ttl_s: 3_600, authorization: "", body: &[] };
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))?;
    for name in ["RelayRequest", "Forward"] {
        let declared = format!("pub struct {name}<'a> {{");
        let above = lib.split(&declared).next().unwrap_or_default().lines().rev();
        let attributes: String =
            above.take_while(|l| l.starts_with("///") || l.starts_with("#[")).filter(|l| l.starts_with("#[")).collect();
        assert!(lib.contains(&declared), "{name} is declared");
        assert!(!attributes.contains("Debug"), "{name} derives no Debug");
        assert!(!lib.contains(&format!("Debug for {name}")), "{name} implements no Debug");
    }
    Ok(())
}
