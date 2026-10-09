//! E8-14 slice S8b. Every verdict is written here from spec §4.6 and DEC-724, never read from the
//! relay: the class pairs from DEC-700 item 3, the cap as the literal 512, and endpoint tables.

use super::{
    Forward, LogEntry, PushAnswer, PushService, RelayError, RelayId, RelayLog, RelayRequest, relay,
};
use mandate_notify::canary::{CANARIES, scan};
use mandate_webpush::{DEFAULT_PUSH_ALLOWLIST, PushAllowlist};
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
    let allowlist =
        PushAllowlist::parse(&DEFAULT_PUSH_ALLOWLIST).map_err(|_| RelayError::AddressRejected)?;
    #[rustfmt::skip]
    let request = RelayRequest { relay_id: &w.0, endpoint: &w.1, urgency: &w.2, ttl_s: w.3, ciphertext: &w.4 };
    match relay(&request, &allowlist, pushes, log) {
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
#[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
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
    #[ignore = "pending E8-14"]
    fn the_cap_admits_512_bytes_and_refuses_513_and_over(
        len in prop_oneof![Just(512usize), Just(513), 0usize..=2_048]
    ) {
        let (answer, posts, _) = run(&wire(FCM, SAFETY, len), PushAnswer::Status(201))?;
        let expected = if len <= 512 { OK } else { Err(RelayError::TooLarge) };
        prop_assert_eq!((answer, posts.len()), (expected, usize::from(len <= 512)));
    }

    #[test]
    #[ignore = "pending E8-14"]
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
    #[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
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
const CODES: [(RelayError, &str); 6] = [
    (RelayError::Unimplemented { story: "E8-14" }, "unimplemented"), (RelayError::InvalidRelayId, "invalid_relay_id"),
    (RelayError::AddressRejected, "address_rejected"), (RelayError::InvalidEnvelope, "invalid_envelope"),
    (RelayError::TooLarge, "too_large"), (RelayError::Unreachable, "unreachable"),
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
