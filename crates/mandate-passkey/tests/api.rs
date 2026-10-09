//! The parts of the API that hold no verification: the challenge's minimum length and the stable
//! refusal codes (ADR-0001 ES-09). Live from the tests PR on, so the mutation gate has a test to
//! judge the crate with (DEC-139).

use mandate_passkey::{Challenge, ChallengeTooShort, Refusal};

#[test]
fn a_challenge_has_at_least_sixteen_bytes() {
    assert_eq!(Challenge::new(&[0; 15]), Err(ChallengeTooShort));
    assert!(Challenge::new(&[0; 16]).is_ok());
    assert_ne!(Challenge::new(&[0; 16]), Challenge::new(&[1; 16]));
}

#[test]
fn every_refusal_has_its_own_stable_code() {
    let codes = [
        (Refusal::ClientDataMalformed, "client_data_malformed"),
        (Refusal::ClientDataType, "client_data_type"),
        (Refusal::ChallengeMismatch, "challenge_mismatch"),
        (Refusal::OriginMismatch, "origin_mismatch"),
        (Refusal::CrossOrigin, "cross_origin"),
        (Refusal::AttestationMalformed, "attestation_malformed"),
        (Refusal::AttestationFormat, "attestation_format"),
        (
            Refusal::AuthenticatorDataMalformed,
            "authenticator_data_malformed",
        ),
        (Refusal::RpIdMismatch, "rp_id_mismatch"),
        (Refusal::UserNotPresent, "user_not_present"),
        (Refusal::UserNotVerified, "user_not_verified"),
        (Refusal::BackupFlags, "backup_flags"),
        (Refusal::PublicKeyMalformed, "public_key_malformed"),
        (Refusal::AlgorithmUnsupported, "algorithm_unsupported"),
        (Refusal::CredentialMismatch, "credential_mismatch"),
        (Refusal::SignatureInvalid, "signature_invalid"),
        (Refusal::CounterNotRising, "counter_not_rising"),
    ];
    for (refusal, code) in codes {
        assert_eq!(refusal.code(), code);
    }
}
