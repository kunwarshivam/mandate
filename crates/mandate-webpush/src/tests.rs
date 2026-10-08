//! E8-14 slice S8a. The RFC vectors are byte-exact; every other check decrypts or verifies on its
//! own path, with the receiver's key or the signer's public key, never through the code under test.

use super::{
    MAX_BODY_LEN, NoticeClass, PADDED_RECORD_LEN, PushEndpoint, PushPlaintext, PushRequest,
    PushText, SecureRandom, Subscription, Urgency, VapidSigner, VapidSubject, WebPushError,
    aes128gcm, build_request, encrypt, seal, vapid_authorization,
};
use aes_gcm::Aes128Gcm;
use aes_gcm::aead::{Aead, KeyInit};
use base64ct::{Base64UrlUnpadded, Encoding};
use hkdf::Hkdf;
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::{PublicKey, SecretKey};
use proptest::prelude::*;
use sha2::Sha256;

fn b64(text: &str) -> Vec<u8> {
    Base64UrlUnpadded::decode_vec(text).unwrap_or_default()
}

/// Hands out the bytes it was given, in order, and fails once they run out.
struct Scripted(Vec<u8>);

impl SecureRandom for Scripted {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), WebPushError> {
        if self.0.len() < out.len() {
            return Err(WebPushError::Random);
        }
        let rest = self.0.split_off(out.len());
        out.copy_from_slice(&self.0);
        self.0 = rest;
        Ok(())
    }
}

/// A test key only: RFC 8291 §5's sender key, published in the RFC.
struct TestSigner(SigningKey);

impl TestSigner {
    fn new() -> Self {
        Self(SigningKey::from_slice(&b64(AS_PRIVATE)).unwrap_or_else(|_| unreachable!()))
    }
}

impl VapidSigner for TestSigner {
    fn public_key(&self) -> [u8; 65] {
        let point = self.0.verifying_key().to_sec1_point(false);
        point.as_bytes().try_into().unwrap_or([0; 65])
    }
    fn sign_es256(&self, message: &[u8]) -> Result<[u8; 64], WebPushError> {
        let signature: Signature = self.0.sign(message);
        Ok(signature.to_bytes().into())
    }
}

const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";
const UA_PRIVATE: &str = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
const UA_PUBLIC: &str =
    "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
const AS_PRIVATE: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
const SALT: &str = "DGv6ra1nlYgDCS1FRnbzlw";
const ENDPOINT: &str = "https://push.example.net/push/JzLQ3raZJfFBR0aqvOMsLrt54w4rJUsV";
const NOW: u64 = 1_453_480_568;

fn subscription(endpoint: &str) -> Result<Subscription, WebPushError> {
    Subscription::new(PushEndpoint::parse(endpoint)?, &b64(UA_PUBLIC), &b64(AUTH))
}

fn rfc_random() -> Scripted {
    Scripted([b64(AS_PRIVATE), b64(SALT)].concat())
}

fn subject() -> Result<VapidSubject, WebPushError> {
    VapidSubject::parse("mailto:push@example.invalid")
}

/// The receiver's side of RFC 8291 §3 and RFC 8188 §2, written apart from the sender's.
fn open(body: &[u8]) -> Vec<u8> {
    let (salt, rest) = body.split_at(16);
    let (_, rest) = rest.split_at(5);
    let (as_public, sealed) = rest.split_at(65);
    let ua = SecretKey::from_slice(&b64(UA_PRIVATE)).unwrap_or_else(|_| unreachable!());
    let sender = PublicKey::from_sec1_bytes(as_public).unwrap_or_else(|_| unreachable!());
    let shared = p256::ecdh::diffie_hellman(ua.to_nonzero_scalar(), sender.as_affine());
    let mut ikm = [0u8; 32];
    let info = [b"WebPush: info\0".as_slice(), &b64(UA_PUBLIC), as_public].concat();
    let _ =
        Hkdf::<Sha256>::new(Some(&b64(AUTH)), shared.raw_secret_bytes()).expand(&info, &mut ikm);
    let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    let _ = hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek);
    let _ = hk.expand(b"Content-Encoding: nonce\0", &mut nonce);
    Aes128Gcm::new_from_slice(&cek)
        .unwrap_or_else(|_| unreachable!())
        .decrypt(&nonce.into(), sealed)
        .unwrap_or_default()
}

#[test]
fn an_endpoint_never_prints_its_address() {
    let endpoint = PushEndpoint {
        url: ENDPOINT.to_owned(),
        origin_len: "https://push.example.net".len(),
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
#[ignore = "pending E8-14"]
fn rfc_8291_section_5_message_is_reproduced_byte_exact() -> Result<(), WebPushError> {
    let plaintext = b"When I grow up, I want to be a watermelon";
    let body = seal(&subscription(ENDPOINT)?, plaintext, None, &mut rfc_random())?;
    let expected = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml\
                    mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT\
                    pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";
    assert_eq!(body, b64(expected), "RFC 8291 §5 and Appendix A");
    Ok(())
}

#[test]
#[ignore = "pending E8-14"]
fn rfc_8188_section_3_1_record_is_reproduced_byte_exact() -> Result<(), WebPushError> {
    let expected = b64("I1BsxtFttlv3u_Oo94xnmwAAEAAA-NAVub2qFgBEuQKRapoZu-IxkIva3MEB1PD-ly8Thjg");
    let salt: [u8; 16] = expected
        .get(..16)
        .and_then(|s| s.try_into().ok())
        .unwrap_or([0; 16]);
    let body = aes128gcm(
        &b64("yqdlZ-tYemfogSmv7Ws5PQ"),
        &salt,
        &[],
        b"I am the walrus",
        None,
    )?;
    assert_eq!(body, expected, "RFC 8188 §3.1");
    Ok(())
}

#[test]
#[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
fn the_vapid_token_verifies_against_the_signers_public_key() -> Result<(), WebPushError> {
    let signer = TestSigner::new();
    let endpoint = PushEndpoint::parse("https://push.example.net:8443/a/b?c")?;
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
        r#"{{"aud":"https://push.example.net:8443","exp":{exp},"sub":"mailto:push@example.invalid"}}"#
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
#[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
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
#[ignore = "pending E8-14"]
fn bad_endpoints_keys_and_subjects_are_refused() -> Result<(), WebPushError> {
    let endpoint = PushEndpoint::parse(ENDPOINT)?;
    assert_eq!(
        format!("{endpoint:?}"),
        "PushEndpoint(..)",
        "an address is never printed (NT-2)"
    );
    for url in [
        "http://push.example.net/x",
        "https:///x",
        "https://u@push.example.net/x",
        "https://a b/",
        "https://push.example.net/\"",
        "https://push.example.net/\\",
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
    let short = Subscription::new(endpoint, &b64(UA_PUBLIC), &[0; 15]).err();
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
        seal(
            &subscription(ENDPOINT)?,
            b"x",
            None,
            &mut Scripted(vec![0; 40])
        )
        .err(),
        Some(WebPushError::Random)
    );
    Ok(())
}

proptest! {
    /// NT-1: whatever the notice id and text key, the plaintext is exactly the closed payload, so
    /// every body is the same length and opens to the payload's own bytes.
    #[test]
    #[ignore = "pending E8-14"]
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
        let mut random = Scripted([seed.to_vec(), vec![9; 16]].concat());
        let body = encrypt(&subscription(ENDPOINT)?, &payload, &mut random);
        prop_assume!(body != Err(WebPushError::Random));
        let body = body?;
        prop_assert_eq!(body.len(), 86 + PADDED_RECORD_LEN + 16);
        let opened = open(&body);
        prop_assert_eq!(opened.get(..json.len()), Some(json.as_bytes()));
    }

    /// RFC 8292 §2: `exp` is in the future and never more than 24 hours ahead.
    #[test]
    #[ignore = "pending E8-14"]
    fn the_token_expires_within_24_hours(now in 0u64..=4_102_444_800) {
        let endpoint = PushEndpoint::parse(ENDPOINT)?;
        let header = vapid_authorization(&endpoint, &subject()?, &TestSigner::new(), now)?;
        let token = header.strip_prefix("vapid t=").and_then(|t| t.split('.').nth(1)).unwrap_or_default();
        let claims = String::from_utf8(b64(token)).unwrap_or_default();
        let exp: u64 = claims.split("\"exp\":").nth(1).and_then(|r| r.split(',').next()).and_then(|n| n.parse().ok()).unwrap_or(0);
        prop_assert!(exp > now && exp <= now.saturating_add(86_400), "exp {exp} now {now}");
    }
}
