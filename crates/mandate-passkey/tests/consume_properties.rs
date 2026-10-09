//! `consume`'s order as a property (DEC-662 item 6): with any set of failures injected into a case
//! every check passes, the refusal is the first of them in the order this file lists, restated
//! from identity spec §7.3 and §7.2 step 4 rather than taken from the crate.

mod common;
mod stepup;

use mandate_identity::StepUpActionKind;
use mandate_passkey::Refusal;
use mandate_passkey::stepup::{Action, Environment, Missing, StepUpRefusal};
use proptest::prelude::*;

use stepup::{Case, Kind, State, issued_plus};

/// The injectable failures in the order the spec checks them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Failure {
    LiveCliConfirm,
    Unknown,
    Used,
    Expired,
    OtherPrincipal,
    NoCredential,
    CoolingOff,
    BadSignature,
    OtherAction,
}

fn expected(failure: Failure) -> StepUpRefusal {
    match failure {
        Failure::LiveCliConfirm => StepUpRefusal::Method,
        Failure::Unknown => StepUpRefusal::Missing(Missing::UnknownChallenge),
        Failure::Used => StepUpRefusal::Reused,
        Failure::Expired => StepUpRefusal::Stale,
        Failure::OtherPrincipal => StepUpRefusal::Mismatch,
        Failure::NoCredential => StepUpRefusal::Missing(Missing::NotTheirCredential),
        Failure::CoolingOff => StepUpRefusal::Missing(Missing::CoolingOff),
        Failure::BadSignature => {
            StepUpRefusal::Missing(Missing::Passkey(Refusal::ChallengeMismatch))
        }
        Failure::OtherAction => StepUpRefusal::Mismatch,
    }
}

fn inject(case: &mut Case, failure: Failure) {
    match failure {
        Failure::LiveCliConfirm => {
            case.environment = Environment::Live;
            case.kind = Kind::CliConfirm;
        }
        Failure::Unknown => case.state = State::Unknown,
        Failure::Used => case.state = State::Used,
        Failure::Expired => case.now = issued_plus(300, 0),
        Failure::OtherPrincipal => case.principal = stepup::mallory(),
        Failure::NoCredential => case.enrolled = None,
        Failure::CoolingOff => {
            let now = case.now;
            if let Some(enrolled) = case.enrolled.as_mut() {
                enrolled.cool_off_ends = issued_plus(now.secs() - issued_plus(0, 0).secs() + 1, 0);
            }
        }
        Failure::BadSignature => case.sign(&common::sha256(b"another record"), |_| {}),
        Failure::OtherAction => {
            case.action = Action {
                kind: StepUpActionKind::Deploy,
                ..case.action
            }
        }
    }
}

/// A set of failures a case can hold at once: `Unknown` and `Used` are both challenge states.
fn failures() -> impl Strategy<Value = Vec<Failure>> {
    let all = [
        Failure::LiveCliConfirm,
        Failure::Unknown,
        Failure::Used,
        Failure::Expired,
        Failure::OtherPrincipal,
        Failure::NoCredential,
        Failure::CoolingOff,
        Failure::BadSignature,
        Failure::OtherAction,
    ];
    proptest::sample::subsequence(all.to_vec(), 0..=all.len())
        .prop_filter("one challenge state", |f| {
            !(f.contains(&Failure::Unknown) && f.contains(&Failure::Used))
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    #[ignore = "pending E9-4"]
    fn the_refusal_is_the_first_injected_failure_in_spec_order(chosen in failures()) {
        let mut case = Case::new();
        for failure in &chosen {
            inject(&mut case, *failure);
        }
        let result = case.run();
        match chosen.iter().min() {
            None => prop_assert_eq!(
                result.map(|consumed| consumed.evidence().authenticated_at),
                Ok(stepup::at(stepup::ISSUED)),
                "no failure injected: the evidence is stamped with the challenge's issue time",
            ),
            Some(first) => prop_assert_eq!(result, Err(expected(*first)), "{:?}", chosen),
        }
    }
}
