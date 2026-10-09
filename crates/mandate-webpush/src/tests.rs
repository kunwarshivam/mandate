//! E8-14 slice S8a. The RFC vectors are byte-exact; every other check decrypts or verifies on its
//! own path, with the receiver's key or the signer's public key, never through the code under test.

use super::{
    DEFAULT_PUSH_ALLOWLIST, MAX_BODY_LEN, NoticeClass, PADDED_RECORD_LEN, PushAllowlist,
    PushEndpoint, PushPlaintext, PushRequest, PushText, Subscription, Urgency, VapidSigner,
    VapidSubject, WebPushError, aes128gcm, build_request, encrypt, seal, vapid_authorization,
};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use proptest::prelude::*;
use vectors::{
    AUTH, RFC_8188_IKM, RFC_8188_MESSAGE, RFC_8291_MESSAGE, SALT, TestSigner, UA_PUBLIC, b64, open,
    rfc_random, seeded_random, short_auth, zero_random,
};

#[path = "../tests/vectors/mod.rs"]
mod vectors;

const ENDPOINT: &str = "https://fcm.googleapis.com/fcm/send/JzLQ3raZJfFBR0aqvOMsLrt54w4rJUsV";
const NOW: u64 = 1_453_480_568;

fn subscription(endpoint: &str) -> Result<Subscription, WebPushError> {
    Subscription::new(PushEndpoint::parse(endpoint)?, &b64(UA_PUBLIC), &b64(AUTH))
}

fn subject() -> Result<VapidSubject, WebPushError> {
    VapidSubject::parse("mailto:push@example.invalid")
}

/// Every `.rs` file under `dir`, at any depth.
fn rust_files(dir: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            found.extend(rust_files(&path)?);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
    Ok(found)
}

#[test]
fn no_product_code_reaches_the_rfc_vectors() -> std::io::Result<()> {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = crate_dir.join("src");
    let tests_module = src.join("tests.rs");
    let product = rust_files(&src)?;
    assert!(
        product.len() >= 3,
        "lib.rs, payload.rs and tests.rs at least"
    );
    for file in product.iter().filter(|f| **f != tests_module) {
        let source = std::fs::read_to_string(file)?;
        assert!(
            !source.contains("vectors"),
            "{} must not include the test vectors (DEC-794)",
            file.display()
        );
    }
    let lib = std::fs::read_to_string(src.join("lib.rs"))?;
    assert!(
        lib.contains("#[cfg(test)]\nmod tests;"),
        "the tests module is test-only"
    );
    let mut held: Vec<String> = std::fs::read_dir(crate_dir.join("tests/vectors"))?
        .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()?;
    held.sort();
    assert_eq!(
        held,
        ["mod.rs"],
        "the ignored directory holds only the vectors module"
    );
    Ok(())
}

#[test]
fn an_endpoint_never_prints_its_address() {
    let endpoint = PushEndpoint {
        url: ENDPOINT.to_owned(),
        origin_len: "https://fcm.googleapis.com".len(),
    };
    assert_eq!(format!("{endpoint:?}"), "PushEndpoint(..)", "NT-2");
}

#[test]
fn the_closed_tables_are_the_specs() {
    let text = [
        (PushText::ApprovalNeeded, "approval_needed"),
        (PushText::AttentionNeeded, "attention_needed"),
        (PushText::AccountChanged, "account_changed"),
        (PushText::BriefReady, "brief_ready"),
    ];
    for (key, wire) in text {
        let payload = PushPlaintext::new([0x0f; 16], key);
        let json = format!(r#"{{"notice":"0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f","text":"{wire}"}}"#);
        assert_eq!(payload.as_bytes(), json.as_bytes(), "spec §4.2");
        assert_eq!(format!("{payload:?}"), "PushPlaintext(..)");
    }
    let classes = [
        (NoticeClass::Action, Urgency::High, "high", 3_600),
        (NoticeClass::Safety, Urgency::High, "high", 86_400),
        (NoticeClass::Info, Urgency::Normal, "normal", 21_600),
    ];
    for (class, urgency, wire, ttl_s) in classes {
        assert_eq!(
            (class.urgency(), class.urgency().as_str(), class.ttl_s()),
            (urgency, wire, ttl_s)
        );
    }
    let codes = [
        (
            WebPushError::Unimplemented { story: "E8-14" },
            "unimplemented",
        ),
        (WebPushError::InvalidEndpoint, "invalid_endpoint"),
        (WebPushError::InvalidKey, "invalid_key"),
        (WebPushError::InvalidAuthSecret, "invalid_auth_secret"),
        (WebPushError::InvalidSubject, "invalid_subject"),
        (WebPushError::Random, "random"),
        (WebPushError::Signer, "signer"),
        (WebPushError::TooLarge, "too_large"),
        (WebPushError::Clock, "clock"),
    ];
    for (error, code) in codes {
        assert_eq!(error.code(), code);
    }
    assert_eq!(PushRequest::CONTENT_ENCODING, "aes128gcm");
}

#[test]
fn rfc_8291_section_5_message_is_reproduced_byte_exact() -> Result<(), WebPushError> {
    let plaintext = b"When I grow up, I want to be a watermelon";
    let body = seal(&subscription(ENDPOINT)?, plaintext, None, &mut rfc_random())?;
    assert_eq!(body, b64(RFC_8291_MESSAGE), "RFC 8291 §5 and Appendix A");
    Ok(())
}

#[test]
fn rfc_8188_section_3_1_record_is_reproduced_byte_exact() -> Result<(), WebPushError> {
    let expected = b64(RFC_8188_MESSAGE);
    let salt: [u8; 16] = expected
        .get(..16)
        .and_then(|s| s.try_into().ok())
        .unwrap_or([0; 16]);
    let body = aes128gcm(&b64(RFC_8188_IKM), &salt, &[], b"I am the walrus", None)?;
    assert_eq!(body, expected, "RFC 8188 §3.1");
    Ok(())
}

#[test]
fn a_notice_decrypts_with_the_subscriptions_key_to_its_padded_payload() -> Result<(), WebPushError>
{
    let notice = PushPlaintext::new([0xab; 16], PushText::ApprovalNeeded);
    let body = encrypt(&subscription(ENDPOINT)?, &notice, &mut rfc_random())?;
    let json = br#"{"notice":"abababababababababababababababab","text":"approval_needed"}"#;
    let mut expected = [json.as_slice(), &[2]].concat();
    expected.resize(PADDED_RECORD_LEN, 0);
    assert_eq!(
        open(&body),
        expected,
        "the payload, its delimiter, then zeros"
    );
    assert_eq!(
        body.get(..16),
        Some(b64(SALT).as_slice()),
        "the salt is the second draw"
    );
    let brief = PushPlaintext::new([0xab; 16], PushText::BriefReady);
    let shorter = encrypt(&subscription(ENDPOINT)?, &brief, &mut rfc_random())?;
    assert_eq!(
        shorter.len(),
        body.len(),
        "the text key's length never shows"
    );
    Ok(())
}

#[test]
fn the_vapid_token_verifies_against_the_signers_public_key() -> Result<(), WebPushError> {
    let signer = TestSigner::new();
    let endpoint = PushEndpoint::parse("https://fcm.googleapis.com/a/b?c")?;
    let header = vapid_authorization(&endpoint, &subject()?, &signer, NOW)?;
    let overflow = vapid_authorization(&endpoint, &subject()?, &signer, u64::MAX);
    assert_eq!(overflow, Err(WebPushError::Clock));
    let (token, key) = header
        .strip_prefix("vapid t=")
        .and_then(|rest| rest.split_once(", k="))
        .unwrap_or_default();
    assert_eq!(
        b64(key),
        signer.public_key(),
        "k= is the signer's public key"
    );
    let (signed, signature) = token.rsplit_once('.').unwrap_or_default();
    let (head, claims) = signed.split_once('.').unwrap_or_default();
    assert_eq!(b64(head), br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = String::from_utf8(b64(claims)).unwrap_or_default();
    let exp = NOW + 43_200;
    let want = format!(
        r#"{{"aud":"https://fcm.googleapis.com","exp":{exp},"sub":"mailto:push@example.invalid"}}"#
    );
    assert_eq!(claims, want, "aud is the origin, exp is 12 h ahead");
    let verifying = VerifyingKey::from_sec1_bytes(&b64(key)).unwrap_or_else(|_| unreachable!());
    let signature = Signature::from_slice(&b64(signature)).unwrap_or_else(|_| unreachable!());
    assert!(
        verifying.verify(signed.as_bytes(), &signature).is_ok(),
        "ES256 over h.c"
    );
    Ok(())
}

#[test]
fn the_envelope_is_fixed_by_class_and_the_endpoint_passes_through() -> Result<(), WebPushError> {
    let table = [
        (NoticeClass::Action, "high", 3_600),
        (NoticeClass::Safety, "high", 86_400),
        (NoticeClass::Info, "normal", 21_600),
    ];
    for (class, urgency, ttl_s) in table {
        let notice = PushPlaintext::new([7; 16], PushText::AttentionNeeded);
        let sub = subscription(ENDPOINT)?;
        let request = build_request(
            &sub,
            &notice,
            class,
            &TestSigner::new(),
            &subject()?,
            NOW,
            &mut rfc_random(),
        )?;
        assert_eq!(
            (request.urgency.as_str(), request.ttl_s),
            (urgency, ttl_s),
            "{class:?}"
        );
        assert_eq!(request.endpoint, ENDPOINT);
        assert!(request.authorization.starts_with("vapid t="));
        assert_eq!(
            open(&request.body).get(..16),
            Some(br#"{"notice":"07070"#.as_slice())
        );
    }
    Ok(())
}

#[test]
fn a_body_over_512_octets_is_refused_and_one_at_512_is_not() -> Result<(), WebPushError> {
    let sub = subscription(ENDPOINT)?;
    let at_cap = seal(&sub, &[b'a'; 409], None, &mut rfc_random())?;
    assert_eq!(at_cap.len(), MAX_BODY_LEN);
    let over = seal(&sub, &[b'a'; 410], None, &mut rfc_random());
    assert_eq!(over, Err(WebPushError::TooLarge));
    let unpaddable = seal(
        &sub,
        &[b'a'; 128],
        Some(PADDED_RECORD_LEN),
        &mut rfc_random(),
    );
    assert_eq!(
        unpaddable,
        Err(WebPushError::TooLarge),
        "no room for the delimiter"
    );
    let snug = seal(
        &sub,
        &[b'a'; 127],
        Some(PADDED_RECORD_LEN),
        &mut rfc_random(),
    )?;
    assert_eq!(
        snug.len(),
        86 + PADDED_RECORD_LEN + 16,
        "the delimiter fills the record"
    );
    Ok(())
}

#[test]
fn bad_endpoints_keys_and_subjects_are_refused() -> Result<(), WebPushError> {
    let endpoint = PushEndpoint::parse(ENDPOINT)?;
    assert_eq!(
        format!("{endpoint:?}"),
        "PushEndpoint(..)",
        "an address is never printed (NT-2)"
    );
    let with_port = "https://fcm.googleapis.com:443/fcm/send/x";
    assert_eq!(
        PushEndpoint::parse(with_port).map(|e| e.url),
        Ok(with_port.to_owned()),
        "port 443 written out"
    );
    for url in [
        "http://push.example.net/x",
        "https:///x",
        "https://u@push.example.net/x",
        "https://a b/",
        "https://push.example.net/\"",
        "https://push.example.net/\\",
        "https://fcm.googleapis.com:0443/x",
        "https://fcm.googleapis.com:+443/x",
    ] {
        assert_eq!(
            PushEndpoint::parse(url),
            Err(WebPushError::InvalidEndpoint),
            "{url}"
        );
    }
    let mut off_curve = b64(UA_PUBLIC);
    if let Some(last) = off_curve.last_mut() {
        *last ^= 1;
    }
    let compressed = [
        vec![2u8],
        b64(UA_PUBLIC).get(1..33).unwrap_or_default().to_vec(),
    ]
    .concat();
    for key in [off_curve, compressed] {
        let refused = Subscription::new(endpoint.clone(), &key, &b64(AUTH)).err();
        assert_eq!(refused, Some(WebPushError::InvalidKey));
    }
    let short = Subscription::new(endpoint, &b64(UA_PUBLIC), &short_auth()).err();
    assert_eq!(short, Some(WebPushError::InvalidAuthSecret));
    for uri in [
        "push@example.com",
        "mailto:",
        "http://example.com",
        "mailto:a\"b",
        "mailto:a\\b",
        "https://",
        "mailto:a b",
    ] {
        assert_eq!(
            VapidSubject::parse(uri),
            Err(WebPushError::InvalidSubject),
            "{uri}"
        );
    }
    assert_eq!(
        seal(&subscription(ENDPOINT)?, b"x", None, &mut zero_random()).err(),
        Some(WebPushError::Random)
    );
    Ok(())
}

/// DEC-792 item 2's default list, written out from the decision.
const SPEC_DEFAULT_LIST: [&str; 4] = [
    "fcm.googleapis.com",
    "updates.push.services.mozilla.com",
    "*.push.apple.com",
    "*.notify.windows.com",
];

const REFUSED: Result<&str, WebPushError> = Err(WebPushError::InvalidEndpoint);

/// Each row is accepted with its address unchanged, or refused with `invalid_endpoint`.
fn check_endpoints(
    allowlist: &PushAllowlist,
    table: &[(&str, Result<&str, WebPushError>)],
) -> Result<(), WebPushError> {
    assert!(table.len() >= 4, "a table with rows to check");
    for (url, expected) in table {
        let got = PushEndpoint::parse_allowed(url, allowlist).map(|e| e.url);
        assert_eq!(got, expected.clone().map(str::to_owned), "{url}");
    }
    Ok(())
}

#[test]
fn the_default_list_is_the_decisions() {
    assert_eq!(DEFAULT_PUSH_ALLOWLIST, SPEC_DEFAULT_LIST, "DEC-792 item 2");
}

/// The shared table (DEC-792 item 3, spec §4.6): the browsers' own push services on the default
/// list, and every syntax DEC-792 item 1 refuses rather than normalizes.
#[test]
fn the_shared_endpoint_table_holds_on_the_default_list() -> Result<(), WebPushError> {
    let list = PushAllowlist::parse(&SPEC_DEFAULT_LIST)?;
    let table = [
        (
            "https://fcm.googleapis.com/fcm/send/abc",
            Ok("https://fcm.googleapis.com/fcm/send/abc"),
        ),
        (
            "https://fcm.googleapis.com/fcm/send/AbC-123",
            Ok("https://fcm.googleapis.com/fcm/send/AbC-123"),
        ),
        (
            "https://updates.push.services.mozilla.com/wpush/v2/gA",
            Ok("https://updates.push.services.mozilla.com/wpush/v2/gA"),
        ),
        (
            "https://web.push.apple.com/QGuQ",
            Ok("https://web.push.apple.com/QGuQ"),
        ),
        (
            "https://a.b.push.apple.com/x",
            Ok("https://a.b.push.apple.com/x"),
        ),
        (
            "https://wns2-bl2p.notify.windows.com/w/?token=AQE",
            Ok("https://wns2-bl2p.notify.windows.com/w/?token=AQE"),
        ),
        (
            "https://fcm.googleapis.com:443/fcm/send/abc",
            Ok("https://fcm.googleapis.com:443/fcm/send/abc"),
        ),
        ("https://push.apple.com/x", REFUSED),
        ("https://notify.windows.com/x", REFUSED),
        ("https://fcm.googleapis.com:8443/fcm/send/abc", REFUSED),
        ("https://fcm.googleapis.com:80/fcm/send/abc", REFUSED),
        ("https://fcm.googleapis.com:0443/fcm/send/abc", REFUSED),
        ("https://fcm.googleapis.com:/fcm/send/abc", REFUSED),
        ("http://fcm.googleapis.com/fcm/send/abc", REFUSED),
        ("https://u@fcm.googleapis.com/fcm/send/abc", REFUSED),
        ("https://u:p@web.push.apple.com/x", REFUSED),
        ("https://142.250.72.10/fcm/send/abc", REFUSED),
        ("https://[::1]/x", REFUSED),
        ("https://fcm.googleapis.com./fcm/send/abc", REFUSED),
        ("https://web.push.apple.com./x", REFUSED),
        ("https://xn--80ak6aa92e.push.apple.com/x", REFUSED),
        ("https://web.xn--push-9qa.apple.com/x", REFUSED),
        ("https://wéb.push.apple.com/x", REFUSED),
        ("https://FCM.googleapis.com/fcm/send/abc", REFUSED),
        ("https://Web.Push.Apple.com/x", REFUSED),
        ("https://fcm%2egoogleapis.com/fcm/send/abc", REFUSED),
        ("https://web%2Epush.apple.com/x", REFUSED),
        ("https://evilfcm.googleapis.com/fcm/send/abc", REFUSED),
        ("https://a.fcm.googleapis.com/fcm/send/abc", REFUSED),
        ("https://fcm.googleapis.com.evil.net/fcm/send/abc", REFUSED),
        ("https://x.notify.windows.com.evil.net/w", REFUSED),
        ("https://evilpush.apple.com/x", REFUSED),
        ("https://android.googleapis.com/gcm/send/abc", REFUSED),
    ];
    check_endpoints(&list, &table)
}

/// RFC 8292 §2: `aud` is the push resource's origin, whose ASCII serialization (RFC 6454 §6.2)
/// omits the default port, so a written `:443` never reaches the token; the address keeps it.
#[test]
fn the_vapid_audience_omits_a_written_default_port() -> Result<(), WebPushError> {
    let url = "https://fcm.googleapis.com:443/fcm/send/x";
    let endpoint =
        PushEndpoint::parse_allowed(url, &PushAllowlist::parse(&DEFAULT_PUSH_ALLOWLIST)?)?;
    assert_eq!(endpoint.url, url, "the stored address is unchanged");
    let header = vapid_authorization(&endpoint, &subject()?, &TestSigner::new(), NOW)?;
    let claims = header
        .strip_prefix("vapid t=")
        .and_then(|t| t.split('.').nth(1))
        .unwrap_or_default();
    let claims = String::from_utf8(b64(claims)).unwrap_or_default();
    let aud = claims
        .split(r#""aud":""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next());
    assert_eq!(aud, Some("https://fcm.googleapis.com"), "RFC 6454 §6.2");
    Ok(())
}

#[test]
fn a_wildcard_entry_matches_proper_subdomains_at_any_depth_and_never_the_domain()
-> Result<(), WebPushError> {
    let list = PushAllowlist::parse(&["*.example.net"])?;
    let table = [
        ("https://a.example.net/p", Ok("https://a.example.net/p")),
        (
            "https://a.b.c.example.net/p",
            Ok("https://a.b.c.example.net/p"),
        ),
        ("https://example.net/p", REFUSED),
        ("https://aexample.net/p", REFUSED),
        ("https://a.example.net.evil/p", REFUSED),
        ("https://a.example.org/p", REFUSED),
    ];
    check_endpoints(&list, &table)
}

#[test]
fn an_exact_entry_matches_only_its_own_host() -> Result<(), WebPushError> {
    let list = PushAllowlist::parse(&["push.example.net"])?;
    let table = [
        (
            "https://push.example.net/p",
            Ok("https://push.example.net/p"),
        ),
        (
            "https://push.example.net:443/p",
            Ok("https://push.example.net:443/p"),
        ),
        ("https://a.push.example.net/p", REFUSED),
        ("https://xpush.example.net/p", REFUSED),
        ("https://push.example.ne/p", REFUSED),
        ("https://example.net/p", REFUSED),
    ];
    check_endpoints(&list, &table)
}

/// A customer-run deployment may shorten the list (DEC-792 item 2); what it dropped is refused.
#[test]
fn a_shortened_list_refuses_the_hosts_it_dropped() -> Result<(), WebPushError> {
    let list = PushAllowlist::parse(&["fcm.googleapis.com"])?;
    let table = [
        (
            "https://fcm.googleapis.com/fcm/send/abc",
            Ok("https://fcm.googleapis.com/fcm/send/abc"),
        ),
        (
            "https://updates.push.services.mozilla.com/wpush/v2/gA",
            REFUSED,
        ),
        ("https://web.push.apple.com/QGuQ", REFUSED),
        ("https://wns2-bl2p.notify.windows.com/w/?token=AQE", REFUSED),
    ];
    check_endpoints(&list, &table)
}

/// An entry is one exact host or `*.` and a domain (DEC-792 item 2); one no accepted endpoint
/// could match is a configuration error, refused rather than kept.
#[test]
fn a_malformed_allowlist_entry_is_refused() {
    let accepted = [
        "fcm.googleapis.com",
        "updates.push.services.mozilla.com",
        "*.push.apple.com",
        "*.notify.windows.com",
        "push-1.example.net",
    ];
    for entry in accepted {
        assert_eq!(
            PushAllowlist::parse(&[entry]).map(|_| ()),
            Ok(()),
            "{entry}"
        );
    }
    let refused = [
        "",
        "*",
        "*.",
        ".",
        "*.*.example.net",
        "a.*.example.net",
        "*example.net",
        "**.example.net",
        "example.net.",
        "*.example.net.",
        "a..example.net",
        "Fcm.googleapis.com",
        "*.Push.apple.com",
        "127.0.0.1",
        "*.0.1",
        "[::1]",
        "xn--80ak6aa92e.example",
        "*.xn--p1ai",
        "wéb.example.net",
        "fcm%2egoogleapis.com",
        "fcm.googleapis.com:443",
        "https://fcm.googleapis.com",
        "fcm.googleapis.com/",
        " fcm.googleapis.com",
        "u@fcm.googleapis.com",
    ];
    for entry in refused {
        let one = PushAllowlist::parse(&[entry]).map(|_| ());
        assert_eq!(one, Err(WebPushError::InvalidEndpoint), "{entry:?}");
        let among = PushAllowlist::parse(&["fcm.googleapis.com", entry]).map(|_| ());
        assert_eq!(
            among,
            Err(WebPushError::InvalidEndpoint),
            "{entry:?} after a good one"
        );
    }
}

/// The entries DEC-722 keeps: the default list, a host or a wildcard's domain of two labels, and a
/// hyphen inside a label (`a--b` is not an `xn--` label).
const STILL_ACCEPTED_ENTRIES: [&str; 9] = [
    "fcm.googleapis.com",
    "updates.push.services.mozilla.com",
    "*.push.apple.com",
    "*.notify.windows.com",
    "a.b",
    "*.a.b",
    "a--b.com",
    "*.a--b.com",
    "push-1.example.net",
];

/// Every entry DEC-722 keeps is accepted, so a parser that refuses everything fails here; then each
/// of `refused` is refused alone and after a good one.
fn check_refused_entries(refused: &[&str], rule: &str) {
    assert!(refused.len() >= 4, "a table with rows to check");
    for entry in STILL_ACCEPTED_ENTRIES {
        assert_eq!(
            PushAllowlist::parse(&[entry]).map(|_| ()),
            Ok(()),
            "{entry} stays accepted"
        );
    }
    for entry in refused {
        let one = PushAllowlist::parse(&[entry]).map(|_| ());
        assert_eq!(one, Err(WebPushError::InvalidEndpoint), "{entry:?}, {rule}");
        let among = PushAllowlist::parse(&["fcm.googleapis.com", entry]).map(|_| ());
        assert_eq!(
            among,
            Err(WebPushError::InvalidEndpoint),
            "{entry:?} after a good one, {rule}"
        );
    }
}

/// DEC-722 keeps two-label entries and inner hyphens, and they match as DEC-792 item 2 says.
#[test]
fn the_entries_dec_722_keeps_are_accepted_and_match() -> Result<(), WebPushError> {
    for entry in STILL_ACCEPTED_ENTRIES {
        assert_eq!(
            PushAllowlist::parse(&[entry]).map(|_| ()),
            Ok(()),
            "{entry}"
        );
    }
    let list = PushAllowlist::parse(&["a.b", "*.x.y", "*.a--b.com"])?;
    let table = [
        ("https://a.b/p", Ok("https://a.b/p")),
        ("https://c.x.y/p", Ok("https://c.x.y/p")),
        ("https://c-d.a--b.com/p", Ok("https://c-d.a--b.com/p")),
        ("https://x.y/p", REFUSED),
        ("https://c.a.b/p", REFUSED),
        ("https://a--b.com/p", REFUSED),
    ];
    check_endpoints(&list, &table)
}

/// DEC-722 item 1: an exact entry names two labels or more, and so does a wildcard's domain, so one
/// typo cannot open the sender to every host under a top-level domain.
#[test]
fn an_allowlist_entry_names_at_least_two_labels() {
    let refused = ["com", "a", "localhost", "*.com", "*.a", "*.net"];
    check_refused_entries(&refused, "DEC-722 item 1");
}

/// DEC-722 item 2 (RFC 1123): no label of an entry starts or ends with a hyphen, in any position.
#[test]
fn no_label_of_an_allowlist_entry_starts_or_ends_with_a_hyphen() {
    let refused = [
        "-a.com",
        "a-.com",
        "*.-a.com",
        "*.a-.com",
        "-.example.net",
        "push.-example.net",
        "push.example-.net",
        "push.example.-net",
        "push.example.net-",
        "*.push.apple-.com",
        "-a-b.com",
        "a-b-.com",
    ];
    check_refused_entries(&refused, "DEC-722 item 2");
}

/// DEC-722 item 2: an endpoint whose host has a label with an edge hyphen is refused although a
/// wildcard entry would match it; a hyphen inside a label is still accepted.
#[test]
fn no_label_of_an_endpoint_host_starts_or_ends_with_a_hyphen() -> Result<(), WebPushError> {
    let list = PushAllowlist::parse(&SPEC_DEFAULT_LIST)?;
    let table = [
        (
            "https://a--b.push.apple.com/x",
            Ok("https://a--b.push.apple.com/x"),
        ),
        (
            "https://wns2-bl2p.notify.windows.com/w",
            Ok("https://wns2-bl2p.notify.windows.com/w"),
        ),
        ("https://-a.push.apple.com/x", REFUSED),
        ("https://a-.push.apple.com/x", REFUSED),
        ("https://-.push.apple.com/x", REFUSED),
        ("https://a.-b.push.apple.com/x", REFUSED),
        ("https://a.b-.push.apple.com/x", REFUSED),
        ("https://-a.push.apple.com:443/x", REFUSED),
        ("https://-wns2.notify.windows.com/w", REFUSED),
        ("https://wns2-.notify.windows.com/w", REFUSED),
    ];
    check_endpoints(&list, &table)
}

proptest! {
    /// NT-1: whatever the notice id and text key, the plaintext is exactly the closed payload, so
    /// every body is the same length and opens to the payload's own bytes.
    #[test]
    fn every_payload_is_the_closed_pair_and_every_body_one_size(
        notice in any::<[u8; 16]>(),
        text in 0usize..4,
        seed in any::<[u8; 32]>(),
    ) {
        let keys = ["approval_needed", "attention_needed", "account_changed", "brief_ready"];
        let all = [PushText::ApprovalNeeded, PushText::AttentionNeeded, PushText::AccountChanged, PushText::BriefReady];
        let (key, text) = (keys.get(text).copied().unwrap_or_default(), all.get(text).copied().unwrap_or(PushText::BriefReady));
        let hex: String = notice.iter().map(|b| format!("{b:02x}")).collect();
        let json = format!(r#"{{"notice":"{hex}","text":"{key}"}}"#);
        let payload = PushPlaintext::new(notice, text);
        prop_assert_eq!(payload.as_bytes(), json.as_bytes());
        let mut random = seeded_random(seed);
        let body = encrypt(&subscription(ENDPOINT)?, &payload, &mut random);
        prop_assume!(body != Err(WebPushError::Random));
        let body = body?;
        prop_assert_eq!(body.len(), 86 + PADDED_RECORD_LEN + 16);
        let opened = open(&body);
        prop_assert_eq!(opened.get(..json.len()), Some(json.as_bytes()));
    }

    /// RFC 8292 §2: `exp` is in the future and never more than 24 hours ahead.
    #[test]
    fn the_token_expires_within_24_hours(now in 0u64..=4_102_444_800) {
        let endpoint = PushEndpoint::parse(ENDPOINT)?;
        let header = vapid_authorization(&endpoint, &subject()?, &TestSigner::new(), now)?;
        let token = header.strip_prefix("vapid t=").and_then(|t| t.split('.').nth(1)).unwrap_or_default();
        let claims = String::from_utf8(b64(token)).unwrap_or_default();
        let exp: u64 = claims.split("\"exp\":").nth(1).and_then(|r| r.split(',').next()).and_then(|n| n.parse().ok()).unwrap_or(0);
        prop_assert!(exp > now && exp <= now.saturating_add(86_400), "exp {exp} now {now}");
    }
}

/// DEC-727, written by hand: a relayed send signs as a role mailbox (item 2's list), never as a
/// person, so `sub` names no member to the relay or the push service (NT-2).
#[test]
fn a_relayed_subject_is_a_role_mailbox_never_a_person() -> Result<(), WebPushError> {
    let roles = ["push", "notifications", "postmaster", "abuse", "security"];
    for role in roles {
        VapidSubject::parse(&format!("mailto:{role}@example.invalid"))?.check_relayed()?;
    }
    VapidSubject::parse("mailto:push@notify.owlhead.ai")?.check_relayed()?;
    #[rustfmt::skip]
    let people = [
        "mailto:alice@example.invalid", "mailto:alice.smith@example.invalid",
        "mailto:push+alice@example.invalid", "mailto:Push@example.invalid", "mailto:pushes@example.invalid",
        "mailto:noreply@example.invalid", "mailto:alice@push.example.invalid", "mailto:push@",
        "mailto:push@example.invalid?cc=alice@example.invalid", "mailto:push@alice@example.invalid",
        "https://example.invalid/contact", "https://example.invalid/~alice",
        "mailto:push@EXAMPLE.invalid", "mailto:push@exa_mple.invalid", "mailto:push@example.invalid/",
        "mailto:push@example.invalid#alice", "mailto:push@example.invalid:443",
    ];
    for uri in people {
        let checked = VapidSubject::parse(uri)?.check_relayed();
        assert_eq!(checked, Err(WebPushError::InvalidSubject), "{uri}");
    }
    Ok(())
}
