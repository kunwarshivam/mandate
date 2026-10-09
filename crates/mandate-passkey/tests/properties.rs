//! Properties of [`verify`] and [`enrol`], each against an oracle that shares no code with the
//! crate (AGENTS.md, "Independent oracles"): the software authenticator of `common` builds every
//! response, and the counter rule is restated here from identity spec §7.2 step 4 and DEC-660.
//!
//! - A response the authenticator signed for the issued challenge verifies, for either algorithm.
//! - Any change to the authenticator data, the client data, or the signature is refused.
//! - The counter is accepted exactly when it rises, or when it is zero on both sides.
//! - No input makes either entry point panic.

mod common;

use common::{Alg, Authenticator, Ceremony, OwnedAssertion, challenge, rp};
use mandate_passkey::{Assertion, Refusal, Registration, Verified, enrol, verify};
use proptest::prelude::*;
use proptest::sample::Index;

#[derive(Debug, Clone)]
enum Mutation {
    Flip(Index, u8),
    Truncate(Index),
    Append(u8),
}

impl Mutation {
    fn apply(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::Flip(at, bit) => {
                let at = at.index(bytes.len());
                bytes[at] ^= 1 << bit;
            }
            Self::Truncate(at) => bytes.truncate(at.index(bytes.len())),
            Self::Append(byte) => bytes.push(*byte),
        }
    }
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        (any::<Index>(), 0u8..8).prop_map(|(at, bit)| Mutation::Flip(at, bit)),
        any::<Index>().prop_map(Mutation::Truncate),
        any::<u8>().prop_map(Mutation::Append),
    ]
}

fn alg() -> impl Strategy<Value = Alg> {
    prop_oneof![Just(Alg::Es256), Just(Alg::Ed25519)]
}

#[derive(Debug, Clone, Copy)]
enum Part {
    AuthenticatorData,
    ClientData,
    Signature,
}

fn part() -> impl Strategy<Value = Part> {
    prop_oneof![
        Just(Part::AuthenticatorData),
        Just(Part::ClientData),
        Just(Part::Signature)
    ]
}

fn signed(alg: Alg, bytes: &[u8], counter: u32) -> (Authenticator, OwnedAssertion) {
    let authenticator = Authenticator::new(alg);
    let assertion = authenticator.assert(&Ceremony::get(bytes, counter));
    (authenticator, assertion)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn a_signed_response_for_the_issued_challenge_verifies(
        alg in alg(),
        bytes in prop::collection::vec(any::<u8>(), 16..64),
        stored in 0u32..1000,
        rise in 1u32..1000,
    ) {
        let (authenticator, assertion) = signed(alg, &bytes, stored + rise);
        let result = verify(&rp(), &challenge(&bytes), &authenticator.credential(stored), &assertion.view());
        prop_assert_eq!(result, Ok(Verified { sign_count: stored + rise, backup_state: false }));
    }

    #[test]
    fn any_change_to_a_signed_response_is_refused(
        alg in alg(),
        bytes in prop::collection::vec(any::<u8>(), 16..64),
        part in part(),
        mutation in mutation(),
    ) {
        let (authenticator, assertion) = signed(alg, &bytes, 9);
        let credential = authenticator.credential(8);
        let issued = challenge(&bytes);
        let untouched = verify(&rp(), &issued, &credential, &assertion.view());
        prop_assert!(untouched.is_ok(), "the unchanged response must verify first: {untouched:?}");
        let mut changed = assertion.clone();
        mutation.apply(match part {
            Part::AuthenticatorData => &mut changed.authenticator_data,
            Part::ClientData => &mut changed.client_data_json,
            Part::Signature => &mut changed.signature,
        });
        prop_assume!(changed.authenticator_data != assertion.authenticator_data
            || changed.client_data_json != assertion.client_data_json
            || changed.signature != assertion.signature);
        let result = verify(&rp(), &issued, &credential, &changed.view());
        prop_assert!(result.is_err(), "{part:?} {mutation:?} was accepted: {result:?}");
    }

    #[test]
    fn the_counter_is_accepted_exactly_when_it_rises_or_is_never_kept(
        stored in prop_oneof![Just(0u32), 0u32..8, any::<u32>()],
        presented in prop_oneof![Just(0u32), 0u32..8, any::<u32>()],
    ) {
        let (authenticator, assertion) = signed(Alg::Ed25519, &[3; 32], presented);
        let result = verify(&rp(), &challenge(&[3; 32]), &authenticator.credential(stored), &assertion.view());
        let rises = u64::from(presented) > u64::from(stored);
        let never_kept = stored == 0 && presented == 0;
        let expected = if rises || never_kept {
            Ok(Verified { sign_count: presented, backup_state: false })
        } else {
            Err(Refusal::CounterNotRising)
        };
        prop_assert_eq!(result, expected);
    }

    #[test]
    fn no_input_makes_either_entry_point_panic(
        first in prop::collection::vec(any::<u8>(), 0..300),
        second in prop::collection::vec(any::<u8>(), 0..300),
        third in prop::collection::vec(any::<u8>(), 0..100),
    ) {
        let authenticator = Authenticator::new(Alg::Es256);
        let issued = challenge(&[1; 16]);
        let registration = Registration { attestation_object: &first, client_data_json: &second };
        let _ = enrol(&rp(), &issued, &registration);
        let assertion = Assertion {
            credential_id: &authenticator.credential_id,
            authenticator_data: &first,
            client_data_json: &second,
            signature: &third,
        };
        let _ = verify(&rp(), &issued, &authenticator.credential(0), &assertion);
    }
}
