//! Assertion verification ([`verify`]): identity spec §6.1 and §7.2 step 4 (signature, user
//! verification, and a counter that never goes backwards) and the strict readings of DEC-660.
//! Each refusal test changes one field of an assertion every check passes; the stored credential
//! comes from the authenticator itself, not from [`mandate_passkey::enrol`].

mod common;

use common::{
    AT, Alg, Authenticator, BE, BS, Ceremony, ED, OwnedAssertion, UP, UV, cbor_int, challenge, rp,
};
use mandate_passkey::{Credential, Refusal, Verified, verify};

/// Encodes to `-_v7` repeated, so the standard alphabet and the URL-safe one differ.
const CHALLENGE: [u8; 32] = [0xfb; 32];

fn verify_with(credential: &Credential, assertion: &OwnedAssertion) -> Result<Verified, Refusal> {
    verify(&rp(), &challenge(&CHALLENGE), credential, &assertion.view())
}

fn refused(change: impl FnOnce(&mut Ceremony)) -> Result<Verified, Refusal> {
    let authenticator = Authenticator::new(Alg::Es256);
    let mut ceremony = Ceremony::get(&CHALLENGE, 5);
    change(&mut ceremony);
    verify_with(
        &authenticator.credential(4),
        &authenticator.assert(&ceremony),
    )
}

#[test]
fn es256_and_ed25519_assertions_verify_and_return_the_new_counter() {
    for alg in [Alg::Es256, Alg::Ed25519] {
        let authenticator = Authenticator::new(alg);
        let assertion = authenticator.assert(&Ceremony::get(&CHALLENGE, 5));
        let expected = Verified {
            sign_count: 5,
            backup_state: false,
        };
        assert_eq!(
            verify_with(&authenticator.credential(4), &assertion),
            Ok(expected),
            "{alg:?}"
        );
    }
}

#[test]
fn an_assertion_without_user_verification_is_refused() {
    assert_eq!(refused(|c| c.flags &= !UV), Err(Refusal::UserNotVerified));
}

#[test]
fn an_assertion_without_user_presence_is_refused() {
    assert_eq!(refused(|c| c.flags &= !UP), Err(Refusal::UserNotPresent));
}

#[test]
fn a_create_ceremony_is_not_an_assertion() {
    let result = refused(|c| c.kind = "webauthn.create".to_owned());
    assert_eq!(result, Err(Refusal::ClientDataType));
}

#[test]
fn a_challenge_that_is_not_the_issued_one_is_refused() {
    let padded = format!("{}=", common::b64url(&CHALLENGE));
    let standard_alphabet = common::b64url(&CHALLENGE)
        .replace('-', "+")
        .replace('_', "/");
    for encoded in [
        common::b64url(&[0xfc; 32]),
        common::b64url(&CHALLENGE[..31]),
        padded,
        standard_alphabet,
        String::new(),
    ] {
        let result = refused(|c| c.challenge = encoded.clone());
        assert_eq!(result, Err(Refusal::ChallengeMismatch), "{encoded}");
    }
}

#[test]
fn an_origin_that_only_shares_the_prefix_is_refused() {
    for origin in [
        "https://app.owlhead.ai.evil.example",
        "https://app.owlhead.ai:443",
        "https://app.owlhead.a",
        "HTTPS://APP.OWLHEAD.AI",
        "https://api.owlhead.ai",
        "https://evil.owlhead.ai",
        "https://owlhead.ai",
    ] {
        let result = refused(|c| c.origin = origin.to_owned());
        assert_eq!(result, Err(Refusal::OriginMismatch), "{origin}");
    }
}

#[test]
fn a_cross_origin_assertion_is_refused_and_an_absent_flag_is_not() {
    assert_eq!(
        refused(|c| c.cross_origin = Some(true)),
        Err(Refusal::CrossOrigin)
    );
    assert!(refused(|c| c.cross_origin = None).is_ok());
}

#[test]
fn an_assertion_for_another_rp_id_is_refused() {
    for rp_id in ["owlhead.ai", "evil.owlhead.ai", "evil.example"] {
        let result = refused(|c| c.rp_id = rp_id.to_owned());
        assert_eq!(result, Err(Refusal::RpIdMismatch), "{rp_id}");
    }
}

/// Verifies an assertion whose `clientDataJSON` is `json`, signed over that JSON.
fn with_client_data(json: &str) -> Result<Verified, Refusal> {
    let authenticator = Authenticator::new(Alg::Es256);
    let mut assertion = authenticator.assert(&Ceremony::get(&CHALLENGE, 5));
    assertion.client_data_json = json.as_bytes().to_vec();
    assertion.signature =
        authenticator.sign_response(&assertion.authenticator_data, &assertion.client_data_json);
    verify_with(&authenticator.credential(4), &assertion)
}

fn good_client_data() -> String {
    String::from_utf8(Ceremony::get(&CHALLENGE, 5).client_data_json()).expect("utf-8")
}

#[test]
fn client_data_that_is_not_the_expected_object_is_refused() {
    let good = good_client_data();
    let challenge = format!(r#""challenge":"{}","#, common::b64url(&CHALLENGE));
    let malformed = [
        good.replacen('{', r#"{"type":"webauthn.get","#, 1),
        good.replacen('{', r#"{"origin":"https://app.owlhead.ai","#, 1),
        format!(
            r#"{},"crossOrigin":false,"crossOrigin":false}}"#,
            &good[..good.len() - 1]
        ),
        good.replacen('{', &format!("{{{challenge}"), 1),
        good.replace(r#","origin":"https://app.owlhead.ai""#, ""),
        "[]".to_owned(),
        good[..good.len() - 1].to_owned(),
        format!(" {good}"),
        format!("\u{feff}{good}"),
    ];
    for json in malformed {
        assert_eq!(
            with_client_data(&json),
            Err(Refusal::ClientDataMalformed),
            "{json}"
        );
    }
}

#[test]
fn unknown_client_data_members_are_ignored_even_when_repeated() {
    let unknown = [
        r#"{"other_keys_can_be_added_here":"do not compare clientDataJSON against a template","topOrigin":"https://app.owlhead.ai","#,
        r#"{"other_keys_can_be_added_here":1,"other_keys_can_be_added_here":2,"#,
    ];
    let expected = Verified {
        sign_count: 5,
        backup_state: false,
    };
    for members in unknown {
        let json = good_client_data().replacen('{', members, 1);
        assert_eq!(with_client_data(&json), Ok(expected), "{json}");
    }
}

#[test]
fn authenticator_data_shorter_than_its_header_is_malformed() {
    let authenticator = Authenticator::new(Alg::Es256);
    let mut assertion = authenticator.assert(&Ceremony::get(&CHALLENGE, 5));
    for length in [36, 33, 32, 0] {
        assertion.authenticator_data.truncate(length);
        assertion.signature =
            authenticator.sign_response(&assertion.authenticator_data, &assertion.client_data_json);
        let result = verify_with(&authenticator.credential(4), &assertion);
        assert_eq!(
            result,
            Err(Refusal::AuthenticatorDataMalformed),
            "{length} bytes"
        );
    }
}

#[test]
fn attested_data_or_unflagged_trailing_bytes_are_refused() {
    let authenticator = Authenticator::new(Alg::Es256);
    let credential = authenticator.credential(4);
    let extensions = [common::cbor_map(&[]), cbor_int(0)].concat();
    for (flags, tail) in [
        (AT, Vec::new()),
        (0, cbor_int(1)),
        (ED, Vec::new()),
        (ED, extensions),
    ] {
        let mut ceremony = Ceremony::get(&CHALLENGE, 5);
        ceremony.flags |= flags;
        let mut assertion = authenticator.assert(&ceremony);
        assertion.authenticator_data.extend_from_slice(&tail);
        assertion.signature =
            authenticator.sign_response(&assertion.authenticator_data, &assertion.client_data_json);
        assert_eq!(
            verify_with(&credential, &assertion),
            Err(Refusal::AuthenticatorDataMalformed),
            "{flags:#x}"
        );
    }
}

#[test]
fn backup_eligibility_never_changes_after_enrolment() {
    let authenticator = Authenticator::new(Alg::Es256);
    let mut ceremony = Ceremony::get(&CHALLENGE, 5);
    ceremony.flags |= BE | BS;
    let assertion = authenticator.assert(&ceremony);
    let eligible = Credential {
        backup_eligible: true,
        ..authenticator.credential(4)
    };
    assert_eq!(
        verify_with(&eligible, &assertion),
        Ok(Verified {
            sign_count: 5,
            backup_state: true
        })
    );
    assert_eq!(
        verify_with(&authenticator.credential(4), &assertion),
        Err(Refusal::BackupFlags)
    );
    assert_eq!(refused(|c| c.flags |= BS), Err(Refusal::BackupFlags));
    let plain = authenticator.assert(&Ceremony::get(&CHALLENGE, 5));
    assert_eq!(verify_with(&eligible, &plain), Err(Refusal::BackupFlags));
}

#[test]
fn an_assertion_naming_another_credential_is_refused() {
    let authenticator = Authenticator::new(Alg::Es256);
    let mut assertion = authenticator.assert(&Ceremony::get(&CHALLENGE, 5));
    assertion.credential_id[0] ^= 1;
    assert_eq!(
        verify_with(&authenticator.credential(4), &assertion),
        Err(Refusal::CredentialMismatch)
    );
}

#[test]
fn a_signature_by_another_key_is_refused() {
    for alg in [Alg::Es256, Alg::Ed25519] {
        let enrolled = Authenticator::new(alg);
        let mut other = Authenticator::new(alg);
        other.credential_id = enrolled.credential_id.clone();
        let assertion = other.assert(&Ceremony::get(&CHALLENGE, 5));
        assert_eq!(
            verify_with(&enrolled.credential(4), &assertion),
            Err(Refusal::SignatureInvalid),
            "{alg:?}"
        );
    }
}

#[test]
fn a_counter_that_does_not_rise_is_refused() {
    let authenticator = Authenticator::new(Alg::Ed25519);
    for (stored, presented) in [(5, 4), (5, 5), (5, 0), (1, 0)] {
        let assertion = authenticator.assert(&Ceremony::get(&CHALLENGE, presented));
        let result = verify_with(&authenticator.credential(stored), &assertion);
        assert_eq!(
            result,
            Err(Refusal::CounterNotRising),
            "stored {stored}, presented {presented}"
        );
    }
}

#[test]
fn a_rising_counter_or_one_never_kept_is_accepted() {
    let authenticator = Authenticator::new(Alg::Ed25519);
    for (stored, presented) in [(0, 0), (0, 1), (u32::MAX - 1, u32::MAX)] {
        let assertion = authenticator.assert(&Ceremony::get(&CHALLENGE, presented));
        let result = verify_with(&authenticator.credential(stored), &assertion);
        assert_eq!(
            result,
            Ok(Verified {
                sign_count: presented,
                backup_state: false
            })
        );
    }
}
