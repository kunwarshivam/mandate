//! E8-14's deployment side of the relay (#1121 minors (a) and (c), DEC-726 item 3, DEC-727,
//! DEC-728). Every expectation is written here from the spec and the decisions, never read from
//! the adapter: the 12-hour lifetime as 43 200 s (DEC-790 item 4), the safety retry schedule from
//! spec §5.3, and the reasons that mark an address from spec §5.2.

use super::{Attempt, DispatchError, Prepared, Route, prepare, relay_refusal};
use base64ct::{Base64UrlUnpadded, Encoding};
use mandate_notify::canary::{CANARIES, scan};
use mandate_notify::{Class, Outcome, Reason, Retry, next_attempt};
use mandate_push_relay::RelayError;
use mandate_time::UtcNanos;
use mandate_webpush::{
    DEFAULT_PUSH_ALLOWLIST, NoticeClass, PushAllowlist, VapidSigner, VapidSubject, WebPushError,
};
use std::cell::Cell;
use std::error::Error;

type Checked = Result<(), Box<dyn Error>>;

const FCM: &str = "https://fcm.googleapis.com/fcm/send/dGVzdA";
const ROLE: &str = "mailto:push@example.invalid";
const PERSON: &str = "mailto:alice@example.invalid";
const RELAYED: Route<'static> = Route::Relayed {
    relay_id: "0123456789abcdef0123456789abcdef",
};
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
#[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
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
