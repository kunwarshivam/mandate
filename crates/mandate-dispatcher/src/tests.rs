//! E8-14's deployment side of the relay (#1121 minors (a) and (c), DEC-726 item 3, DEC-727,
//! DEC-728). Every expectation is written here from the spec and the decisions, never read from
//! the adapter: the 12-hour lifetime as 43 200 s (DEC-790 item 4), the safety retry schedule from
//! spec §5.3, and the reasons that mark an address from spec §5.2.

use super::{Attempt, DispatchError, Prepared, Route, prepare, push_status, relay_refusal};
use base64ct::{Base64UrlUnpadded, Encoding};
use mandate_notify::canary::{CANARIES, scan};
use mandate_notify::{Class, Outcome, Reason, Retry, next_attempt};
use mandate_push_relay::RelayError;
use mandate_time::UtcNanos;
use mandate_webpush::{
    DEFAULT_PUSH_ALLOWLIST, NoticeClass, PushAllowlist, Urgency, VapidSigner, VapidSubject,
    WebPushError,
};
use std::cell::Cell;
use std::error::Error;

type Checked = Result<(), Box<dyn Error>>;

const FCM: &str = "https://fcm.googleapis.com/fcm/send/dGVzdA";
const ROLE: &str = "mailto:push@example.invalid";
const PERSON: &str = "mailto:alice@example.invalid";
const RELAY_ID: &str = "0123456789abcdef0123456789abcdef";
const RELAYED: Route<'static> = Route::Relayed { relay_id: RELAY_ID };
/// An `https` address whose host is well formed but on no allowlist.
const OFF_LIST: &str = "https://push.example.invalid/send/dGVzdA";
const NOW: u64 = 1_791_000_000;
const TWELVE_HOURS_S: u64 = 43_200;
const SAFETY_WINDOW_S: u64 = 86_400;
const BODY: [u8; 230] = [0x5a; 230];
const PROVIDER_ERROR: Outcome = Outcome::Permanent {
    reason: Reason::ProviderError,
};
/// Spec §5.2: the reasons that mark an address `unreachable` and raise `channel_lost`.
#[rustfmt::skip]
const MARKS: [Reason; 5] = [
    Reason::AddressRejected, Reason::AuthFailed, Reason::Bounced, Reason::Complained,
    Reason::Unsubscribed,
];

/// A signer with fixed bytes that counts its signatures, or fails every one.
#[derive(Default)]
struct Counting {
    signed: Cell<u32>,
    fails: bool,
}

impl VapidSigner for Counting {
    fn public_key(&self) -> [u8; 65] {
        [4; 65]
    }
    fn sign_es256(&self, _: &[u8]) -> Result<[u8; 64], WebPushError> {
        self.signed.set(self.signed.get().saturating_add(1));
        (!self.fails)
            .then_some([0xfb; 64])
            .ok_or(WebPushError::Signer)
    }
}

/// One `safety` attempt to [`FCM`]; a stub's report ends the test (DEC-77).
fn attempt(
    signer: &Counting,
    subject: &str,
    route: Route<'_>,
    now: u64,
) -> Result<Prepared, Box<dyn Error>> {
    let allowlist = PushAllowlist::parse(&DEFAULT_PUSH_ALLOWLIST)?;
    let subject = VapidSubject::parse(subject)?;
    Ok(prepare(&Attempt {
        endpoint: FCM,
        allowlist: &allowlist,
        body: &BODY,
        class: NoticeClass::Safety,
        signer,
        subject: &subject,
        now_unix_s: now,
        route,
    })?)
}

/// The header a built attempt carries, checking that the route is the one asked for.
fn header(prepared: Prepared, route: Route<'_>) -> Result<String, Box<dyn Error>> {
    match (prepared, route) {
        (Prepared::Direct(push), Route::Direct) => Ok(push.authorization),
        (Prepared::Relayed(r), Route::Relayed { .. }) => Ok(r.request()?.authorization.to_owned()),
        _ => Err("the attempt was refused or took the other route".into()),
    }
}

/// The `exp` claim, decoded from the header's claims segment on its own.
fn exp_of(header: &str) -> Result<u64, Box<dyn Error>> {
    let claims = header
        .strip_prefix("vapid t=")
        .and_then(|token| token.split('.').nth(1))
        .ok_or("no claims segment")?;
    let decoded = Base64UrlUnpadded::decode_vec(claims).map_err(|_| "claims not base64url")?;
    let json = String::from_utf8(decoded)?;
    let exp = json
        .split("\"exp\":")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .ok_or("no exp claim")?;
    Ok(exp.parse()?)
}

fn plus(at: u64, secs: u64) -> Result<u64, Box<dyn Error>> {
    Ok(at.checked_add(secs).ok_or("overflow")?)
}

/// #1121 (a), DEC-726 item 3: two attempts at t1 and t2 sign two headers, each with its own time,
/// so their `exp` claims differ by t2 − t1 and neither is the other's.
#[test]
fn each_attempt_signs_its_own_header_with_its_own_time() -> Checked {
    let (t1, t2) = (NOW, plus(NOW, 15)?);
    for route in [Route::Direct, RELAYED] {
        let signer = Counting::default();
        let first = header(attempt(&signer, ROLE, route, t1)?, route)?;
        let second = header(attempt(&signer, ROLE, route, t2)?, route)?;
        assert_eq!(exp_of(&first)?, plus(t1, TWELVE_HOURS_S)?);
        assert_eq!(exp_of(&second)?.checked_sub(exp_of(&first)?), Some(15));
        assert_ne!(first, second, "a header is never reused");
        assert_eq!(signer.signed.get(), 2, "one signature per attempt");
    }
    Ok(())
}

/// #1121 (a), DEC-790 item 4, DEC-726 item 3: over spec §5.3's `safety` schedule (at once, then
/// after 15 s, 60 s and 5 min, then every 15 min, for 24 hours) every attempt's header expires 12
/// hours after that attempt, so none has lapsed at its own send time, though a header reused from
/// the first attempt would have lapsed long before the window ends.
#[test]
fn no_header_lapses_inside_the_safety_retry_window() -> Checked {
    let mut sends = vec![NOW];
    for gap in [15, 60, 300].into_iter().chain(std::iter::repeat(900)) {
        let next = plus(*sends.last().ok_or("no attempt")?, gap)?;
        if next > plus(NOW, SAFETY_WINDOW_S)? {
            break;
        }
        sends.push(next);
    }
    let last = *sends.last().ok_or("no attempt")?;
    assert!(
        plus(NOW, TWELVE_HOURS_S)? < last,
        "the window outlives one header"
    );
    for route in [Route::Direct, RELAYED] {
        for &at in &sends {
            let exp = exp_of(&header(
                attempt(&Counting::default(), ROLE, route, at)?,
                route,
            )?)?;
            assert!(exp > at, "a header has lapsed at its own attempt");
            assert_eq!(exp, plus(at, TWELVE_HOURS_S)?);
        }
    }
    Ok(())
}

/// DEC-727 item 3, spec §4.6: a relayed send whose subject is not a role mailbox builds no header,
/// so it signs nothing, and is refused `permanent { provider_error }` (DEC-728 item 2); a direct
/// send keeps DEC-790 item 4's rule, and a role mailbox goes through the relay.
#[test]
fn a_relayed_send_checks_the_subject_before_any_header() -> Checked {
    #[rustfmt::skip]
    let refused = [
        PERSON, "mailto:push+alice@example.invalid", "mailto:Push@example.invalid",
        "https://example.invalid/contact",
    ];
    for subject in refused {
        let signer = Counting::default();
        let prepared = attempt(&signer, subject, RELAYED, NOW)?;
        assert!(
            matches!(prepared, Prepared::Refused(PROVIDER_ERROR)),
            "{subject}"
        );
        assert_eq!(signer.signed.get(), 0, "{subject}: a header was built");
    }
    let signer = Counting::default();
    header(attempt(&signer, PERSON, Route::Direct, NOW)?, Route::Direct)?;
    header(attempt(&signer, ROLE, RELAYED, NOW)?, RELAYED)?;
    assert_eq!(signer.signed.get(), 2);
    Ok(())
}

/// Every refusal the relay can answer, kept whole by [`expected`].
#[rustfmt::skip]
const REFUSALS: [RelayError; 7] = [
    RelayError::Unimplemented { story: "E8-14" }, RelayError::InvalidRelayId,
    RelayError::AddressRejected, RelayError::InvalidEnvelope, RelayError::TooLarge,
    RelayError::Unreachable, RelayError::InvalidAuthorization,
];

/// Each refusal's outcome, written here from spec §5.2, §5.3, DEC-724 item 4 and DEC-728 item 1;
/// it fails to compile when `RelayError` gains a variant.
fn expected(refusal: RelayError) -> Outcome {
    match refusal {
        RelayError::AddressRejected => Outcome::Permanent {
            reason: Reason::AddressRejected,
        },
        RelayError::Unreachable => Outcome::Retryable {
            reason: Reason::Timeout,
        },
        RelayError::Unimplemented { .. }
        | RelayError::InvalidRelayId
        | RelayError::InvalidEnvelope
        | RelayError::TooLarge
        | RelayError::InvalidAuthorization => PROVIDER_ERROR,
    }
}

/// #1121 (c), DEC-724 item 4, DEC-728 item 1, spec §5.2 and §5.3: the relay's `address_rejected`
/// marks the address and stops; its `unreachable` is a timeout retried 15 s later, inside the
/// `safety` window; and every deployment-side fault, `invalid_authorization` included, is a
/// permanent `provider_error`, never `auth_failed`, that marks nothing and stops.
#[test]
fn each_relay_refusal_maps_to_its_section_5_2_outcome() -> Checked {
    let at = UtcNanos::from_parts(1_791_000_000, 0)?;
    let retry = Retry::At(UtcNanos::from_parts(1_791_000_015, 0)?);
    for (n, refusal) in REFUSALS.into_iter().enumerate() {
        let outcome = relay_refusal(refusal)?;
        assert_eq!(outcome, expected(refusal), "refusal {n}");
        let after = next_attempt(Class::Safety, at, at, 1, &outcome)?;
        match outcome {
            Outcome::Permanent { reason } => {
                let marks = refusal == RelayError::AddressRejected;
                assert_eq!(MARKS.contains(&reason), marks, "refusal {n}");
                assert_ne!(reason, Reason::AuthFailed, "refusal {n}");
                assert_eq!(after, Retry::Failed(reason), "refusal {n}");
            }
            Outcome::Retryable { .. } => assert_eq!(after, retry, "refusal {n}"),
            Outcome::Accepted { .. } => return Err("a refusal was accepted".into()),
        }
    }
    Ok(())
}

/// NT-1, NT-2, DEC-726 item 3: subjects whose local part or domain, and so whose claims, carry
/// canaries leave none in anything the adapter returns that could be journaled or logged: a
/// refusal's outcome and its reason key, a failed signature's error, and every relay refusal's
/// outcome.
#[test]
fn nothing_journaled_or_logged_carries_the_subject() -> Checked {
    let person = format!("mailto:{}@example.invalid", CANARIES.join("."));
    let role = format!("mailto:push@{}.invalid", CANARIES.join("."));
    assert_eq!(
        scan(person.as_bytes())?.len(),
        CANARIES.len(),
        "the oracle sees them"
    );
    let mut captured = String::new();
    let Prepared::Refused(outcome) = attempt(&Counting::default(), &person, RELAYED, NOW)? else {
        return Err("a person's subject went through the relay".into());
    };
    captured.push_str(&format!("{outcome:?}"));
    let failing = Counting {
        signed: Cell::new(0),
        fails: true,
    };
    for route in [Route::Direct, RELAYED] {
        let Err(error) = attempt(&failing, &role, route, NOW) else {
            return Err("a failed signature built a request".into());
        };
        let error = error.downcast::<DispatchError>()?;
        captured.push_str(&format!("{error} {error:?}"));
    }
    for refusal in REFUSALS {
        captured.push_str(&format!("{:?}", relay_refusal(refusal)?));
    }
    assert_eq!(scan(captured.as_bytes())?, Vec::<&str>::new());
    Ok(())
}

/// A live test, true before and after the adapter is built, so the mutation gate has a test to
/// judge this crate by (DEC-139): what an attempt prepares holds the signed header and the sealed
/// body, so neither `Prepared` nor `Relayed` derives or implements `Debug` (NT-2, DEC-726 item 3).
#[test]
fn a_prepared_attempt_has_no_debug() -> std::io::Result<()> {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))?;
    for declared in ["pub enum Prepared {", "pub struct Relayed {"] {
        let above = lib.split(declared).next().unwrap_or_default().lines().rev();
        let attributes: String = above
            .take_while(|l| l.starts_with("///") || l.starts_with("#["))
            .filter(|l| l.starts_with("#["))
            .collect();
        assert!(lib.contains(declared), "{declared} is declared");
        assert!(!attributes.contains("Debug"), "{declared} derives no Debug");
    }
    for name in ["Prepared", "Relayed"] {
        assert!(
            !lib.contains(&format!("Debug for {name}")),
            "{name} implements no Debug"
        );
    }
    Ok(())
}

/// DEC-700 item 3 and spec §4.6, written out here: each class's fixed `urgency`, as the enum and
/// as the relay's wire string, and its TTL in seconds.
#[rustfmt::skip]
const CLASS_PAIRS: [(NoticeClass, Urgency, &str, u32); 3] = [
    (NoticeClass::Action, Urgency::High, "high", 3_600),
    (NoticeClass::Safety, Urgency::High, "high", 86_400),
    (NoticeClass::Info, Urgency::Normal, "normal", 21_600),
];

/// A body whose every byte differs from its neighbours, so a truncation, a reordering, or a
/// changed byte shows.
fn distinct_body() -> Vec<u8> {
    (0..=u8::MAX).collect()
}

/// One attempt with every input chosen by the test; the outer `Result` is the fixture's own
/// parsing, the inner one is what [`prepare`] answered.
fn prepare_with(
    signer: &Counting,
    class: NoticeClass,
    endpoint: &str,
    body: &[u8],
    subject: &str,
    route: Route<'_>,
) -> Result<Result<Prepared, DispatchError>, Box<dyn Error>> {
    let allowlist = PushAllowlist::parse(&DEFAULT_PUSH_ALLOWLIST)?;
    let subject = VapidSubject::parse(subject)?;
    Ok(prepare(&Attempt {
        endpoint,
        allowlist: &allowlist,
        body,
        class,
        signer,
        subject: &subject,
        now_unix_s: NOW,
        route,
    }))
}

/// #1180 minor (A), DEC-700 item 3, spec §4.6: every class's request carries that class's fixed
/// `urgency` and TTL, the same direct or relayed, and the relay's request carries the same pair.
#[test]
fn each_class_carries_its_fixed_urgency_and_ttl_on_both_routes() -> Checked {
    for (class, urgency, wire, ttl_s) in CLASS_PAIRS {
        let direct = prepare_with(&Counting::default(), class, FCM, &BODY, ROLE, Route::Direct)??;
        let Prepared::Direct(push) = direct else {
            return Err("a direct attempt took another path".into());
        };
        assert_eq!(
            (push.urgency, push.ttl_s),
            (urgency, ttl_s),
            "{wire} direct"
        );
        let relayed = prepare_with(&Counting::default(), class, FCM, &BODY, ROLE, RELAYED)??;
        let Prepared::Relayed(relayed) = relayed else {
            return Err("a relayed attempt took another path".into());
        };
        assert_eq!(
            (relayed.push.urgency, relayed.push.ttl_s),
            (urgency, ttl_s),
            "{wire} relayed"
        );
        let request = relayed.request()?;
        assert_eq!(
            (request.urgency, request.ttl_s),
            (wire, ttl_s),
            "{wire} relay request"
        );
    }
    Ok(())
}

/// #1180 minor (A), DEC-726 item 7, spec §4.6: the relay's request carries the attempt's relay id,
/// endpoint, sealed body, and the prepared push request's own header, each unchanged; that header
/// is the one a direct send of the same attempt signs, since the test signer's bytes are fixed.
#[test]
fn the_relay_request_carries_the_attempt_through_unchanged() -> Checked {
    let body = distinct_body();
    let action = NoticeClass::Action;
    let relayed = prepare_with(&Counting::default(), action, FCM, &body, ROLE, RELAYED)??;
    let Prepared::Relayed(relayed) = relayed else {
        return Err("a relayed attempt took another path".into());
    };
    let direct = prepare_with(
        &Counting::default(),
        action,
        FCM,
        &body,
        ROLE,
        Route::Direct,
    )??;
    let direct_header = header(direct, Route::Direct)?;
    let request = relayed.request()?;
    assert_eq!(request.relay_id, RELAY_ID);
    assert_eq!(request.endpoint, FCM);
    assert_eq!(request.ciphertext, body.as_slice());
    assert_eq!(request.authorization, relayed.push.authorization);
    assert_eq!(request.authorization, direct_header);
    assert!(
        request.authorization.starts_with("vapid t="),
        "a VAPID header"
    );
    Ok(())
}

/// #1180 minor (C), spec §4.6: the body `mandate_webpush::encrypt` sealed is sent byte for byte,
/// direct or relayed, for every class.
#[test]
fn the_sealed_body_arrives_byte_for_byte_on_both_routes() -> Checked {
    let body = distinct_body();
    for (class, _, wire, _) in CLASS_PAIRS {
        let direct = prepare_with(&Counting::default(), class, FCM, &body, ROLE, Route::Direct)??;
        let Prepared::Direct(push) = direct else {
            return Err("a direct attempt took another path".into());
        };
        assert_eq!(push.body, body, "{wire} direct");
        let relayed = prepare_with(&Counting::default(), class, FCM, &body, ROLE, RELAYED)??;
        let Prepared::Relayed(relayed) = relayed else {
            return Err("a relayed attempt took another path".into());
        };
        assert_eq!(relayed.push.body, body, "{wire} relayed");
        assert_eq!(
            relayed.request()?.ciphertext,
            body.as_slice(),
            "{wire} relay request"
        );
    }
    Ok(())
}

/// #1180 minor (B), DEC-727 item 3, DEC-728 item 2, spec §4.6: a relayed attempt with both a
/// person's subject and an address off the allowlist is refused for its subject, signing nothing,
/// since the subject is checked first; with a role subject, or on a direct route, the same address
/// is still the endpoint error, and nothing is signed either.
#[test]
fn a_relayed_send_refuses_the_subject_before_the_endpoint() -> Checked {
    let signer = Counting::default();
    let safety = NoticeClass::Safety;
    let both_bad = prepare_with(&signer, safety, OFF_LIST, &BODY, PERSON, RELAYED)?;
    assert!(
        matches!(both_bad, Ok(Prepared::Refused(PROVIDER_ERROR))),
        "the subject refusal comes first"
    );
    let endpoint_error = DispatchError::WebPush(WebPushError::InvalidEndpoint);
    for (subject, route) in [
        (ROLE, RELAYED),
        (ROLE, Route::Direct),
        (PERSON, Route::Direct),
    ] {
        let Err(error) = prepare_with(&signer, safety, OFF_LIST, &BODY, subject, route)? else {
            return Err(format!("{subject}: an address off the allowlist was prepared").into());
        };
        assert_eq!(error, endpoint_error, "{subject}");
    }
    assert_eq!(signer.signed.get(), 0, "nothing was signed");
    Ok(())
}

/// The id a test's caller journals for an accepted push.
const MESSAGE_ID: &str = "push-1";

/// `retryable { reason }`.
const fn retry(reason: Reason) -> Outcome {
    Outcome::Retryable { reason }
}

/// `permanent { reason }`.
const fn stop(reason: Reason) -> Outcome {
    Outcome::Permanent { reason }
}

/// `accepted { MESSAGE_ID }`.
fn accepted() -> Outcome {
    Outcome::Accepted {
        provider_message_id: MESSAGE_ID.to_owned(),
    }
}

/// Every status [`push_status`] names, and the codes at each class boundary, each with the outcome
/// written here from its source: spec §4.6 (a `3xx`), §5.3 (a `429`), RFC 8030 (`201`, `404`,
/// `410`, `413`), and DEC-729 for the rest. The `401` and `403` rows are DEC-729 item 4's interim,
/// Proposed for the founder, who may change them; such a change is a tests-correction PR.
fn push_rows() -> Vec<(u16, Outcome)> {
    let unknown = retry(Reason::ProviderError);
    let mut rows: Vec<(u16, Outcome)> =
        [0, 100, 199].map(|status| (status, unknown.clone())).into();
    rows.extend([200, 201, 202, 204, 299].map(|status| (status, accepted())));
    for status in [300, 301, 302, 304, 307, 308, 399, 404, 410] {
        rows.push((status, stop(Reason::AddressRejected)));
    }
    for status in [400, 401, 403] {
        rows.push((status, stop(Reason::ProviderError)));
    }
    rows.push((413, stop(Reason::TooLarge)));
    rows.push((429, retry(Reason::RateLimited)));
    for status in [
        402, 405, 406, 408, 409, 411, 412, 414, 415, 422, 428, 430, 451, 499,
    ] {
        rows.push((status, unknown.clone()));
    }
    for status in [500, 501, 502, 503, 504, 599, 600, 999, u16::MAX] {
        rows.push((status, unknown.clone()));
    }
    rows
}

/// DEC-724 item 6, DEC-729, spec §4.6, §5.2, §5.3: each push service status, returned by the relay
/// unchanged or received by a direct send, maps to the outcome its row writes.
///
/// The founder may change the `401` and `403` rows under DEC-729 item 4, now Proposed; until then
/// they are its interim, `permanent { provider_error }`.
#[test]
#[ignore = "pending E8-14"]
fn each_push_status_maps_to_its_section_5_2_outcome() -> Checked {
    for (status, outcome) in push_rows() {
        assert_eq!(push_status(status, MESSAGE_ID)?, outcome, "status {status}");
    }
    Ok(())
}

/// Spec §4.6, §5.2, §5.6, DEC-728, DEC-729: over every `u16`, only a redirect, `404` or `410`
/// marks the address; nothing is ever `auth_failed`, so no deployment-side VAPID fault marks an
/// address; and exactly the `2xx` statuses are accepted, carrying the caller's id unchanged.
///
/// That no status is `auth_failed` is DEC-729 item 4's interim for `401` and `403`, which the
/// founder may change; a change there corrects this assertion in a tests-correction PR.
#[test]
#[ignore = "pending E8-14"]
fn only_a_redirect_or_a_gone_subscription_marks_the_address() -> Checked {
    for status in 0..=u16::MAX {
        let outcome = push_status(status, MESSAGE_ID)?;
        let gone = (300..=399).contains(&status) || status == 404 || status == 410;
        let marks = matches!(outcome, Outcome::Permanent { reason } if MARKS.contains(&reason));
        assert_eq!(marks, gone, "status {status}");
        let auth_failed = matches!(
            outcome,
            Outcome::Permanent {
                reason: Reason::AuthFailed
            }
        );
        assert!(
            !auth_failed,
            "status {status}: no status is auth_failed under DEC-729 item 4's interim, which the founder may change"
        );
        assert_eq!(
            outcome == accepted(),
            (200..=299).contains(&status),
            "status {status}"
        );
    }
    Ok(())
}

/// Spec §5.3, DEC-729: over every `u16`, each status outside the `2xx` and the named permanent ones
/// (any `3xx`, `400`, `401`, `403`, `404`, `410`, `413`) is retryable, so a `safety` notice's next
/// attempt is 15 s later, inside its 24-hour window; each named one stops that channel.
#[test]
#[ignore = "pending E8-14"]
fn an_unexpected_status_is_retried_inside_the_safety_window() -> Checked {
    let at = UtcNanos::from_parts(1_791_000_000, 0)?;
    let retry_at = Retry::At(UtcNanos::from_parts(1_791_000_015, 0)?);
    let named = [400, 401, 403, 404, 410, 413];
    for status in 0..=u16::MAX {
        let outcome = push_status(status, MESSAGE_ID)?;
        if (200..=299).contains(&status) {
            continue;
        }
        let permanent = (300..=399).contains(&status) || named.contains(&status);
        let after = next_attempt(Class::Safety, at, at, 1, &outcome)?;
        match outcome {
            Outcome::Permanent { reason } if permanent => {
                assert_eq!(after, Retry::Failed(reason), "status {status}");
            }
            Outcome::Retryable { .. } if !permanent => {
                assert_eq!(after, retry_at, "status {status}");
            }
            other => return Err(format!("status {status} gave {other:?}").into()),
        }
    }
    Ok(())
}

/// DEC-729 item 1: the journal refuses `delivered` without an id, so a `2xx` with an empty id is
/// the closed `NoMessageId`, never an `accepted` with no id; any other status maps as it would with
/// an id, so an empty id never loses a `404`'s mark or a `5xx`'s retry.
#[test]
#[ignore = "pending E8-14"]
fn an_accepted_push_with_no_message_id_is_refused() -> Checked {
    for status in [200, 201, 202, 204, 299] {
        match push_status(status, "") {
            Err(DispatchError::NoMessageId) => {}
            Err(other) => return Err(other.into()),
            Ok(outcome) => return Err(format!("status {status} gave {outcome:?}").into()),
        }
    }
    for (status, outcome) in push_rows() {
        if !(200..=299).contains(&status) {
            assert_eq!(push_status(status, "")?, outcome, "status {status}");
        }
    }
    Ok(())
}
