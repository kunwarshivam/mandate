//! `consume`'s order (DEC-662 item 6) over every combination of failures: injected into a case
//! every check passes, the refusal is the first of them in the order this file lists, restated
//! from identity spec §7.3, §7.2 step 4, and DEC-662 item 6's reading of step 6 as
//! `mandate_passkey::verify`'s order, rather than taken from the crate.

mod common;
mod stepup;

use mandate_canon::Digest;
use mandate_identity::StepUpActionKind;
use mandate_passkey::Refusal;
use mandate_passkey::stepup::{Action, EnrolledCredential, Environment, Missing, StepUpRefusal};

use common::{Ceremony, OwnedAssertion, UP, UV};
use stepup::{
    COOL_OFF_ENDS, Case, Kind, State, alice, approval, at, challenge_bytes, issued_plus, mallory,
    other_workspace, workspace,
};

/// The injectable failures in the order they are checked: the method (§7.3); the challenge
/// exists, is unused, is not stale, and names this principal and workspace; the credential is
/// the principal's and past its cool-off; the assertion verifies, in `verify`'s order
/// (`clientDataJSON`'s challenge, user verification, the signature, the counter); and the action
/// is the challenge's, kind and digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Failure {
    LiveCliConfirm,
    Unknown,
    Used,
    Expired,
    BeforeIssue,
    OtherPrincipal,
    OtherWorkspace,
    NoCredential,
    OthersCredential,
    CoolingOff,
    OtherChallenge,
    NoUserVerification,
    BadSignature,
    CounterNotRising,
    OtherKind,
    OtherDigest,
}

use Failure::*;

/// Each group adds none or one of its failures to a combination: a challenge has one state, a
/// case one instant, and a passkey one holder and one cool-off.
const GROUPS: &[&[Failure]] = &[
    &[LiveCliConfirm],
    &[Unknown, Used],
    &[Expired, BeforeIssue],
    &[OtherPrincipal],
    &[OtherWorkspace],
    &[NoCredential, OthersCredential, CoolingOff],
    &[OtherChallenge],
    &[NoUserVerification],
    &[BadSignature],
    &[CounterNotRising],
    &[OtherKind],
    &[OtherDigest],
];

fn expected(failure: Failure) -> StepUpRefusal {
    let passkey = |refusal| StepUpRefusal::Missing(Missing::Passkey(refusal));
    match failure {
        LiveCliConfirm => StepUpRefusal::Method,
        Unknown => StepUpRefusal::Missing(Missing::UnknownChallenge),
        Used => StepUpRefusal::Reused,
        Expired | BeforeIssue => StepUpRefusal::Stale,
        OtherPrincipal | OtherWorkspace => StepUpRefusal::Mismatch,
        NoCredential | OthersCredential => StepUpRefusal::Missing(Missing::NotTheirCredential),
        CoolingOff => StepUpRefusal::Missing(Missing::CoolingOff),
        OtherChallenge => passkey(Refusal::ChallengeMismatch),
        NoUserVerification => passkey(Refusal::UserNotVerified),
        BadSignature => passkey(Refusal::SignatureInvalid),
        CounterNotRising => passkey(Refusal::CounterNotRising),
        OtherKind | OtherDigest => StepUpRefusal::Mismatch,
    }
}

/// Every combination, one member or none from each group.
fn combinations() -> Vec<Vec<Failure>> {
    GROUPS.iter().fold(vec![Vec::new()], |sofar, group| {
        sofar
            .iter()
            .flat_map(|chosen| {
                std::iter::once(chosen.clone()).chain(group.iter().map(|failure| {
                    let mut more = chosen.clone();
                    more.push(*failure);
                    more
                }))
            })
            .collect()
    })
}

/// The assertion for each subset of the assertion's own failures, indexed by
/// [`assertion_index`], all signed by `case`'s authenticator over the counter 8.
fn assertions(case: &mut Case) -> Vec<OwnedAssertion> {
    let other = common::sha256(b"another record");
    (0..8)
        .map(|index| {
            let bytes = pick(index & 1 != 0, other.clone(), challenge_bytes());
            let flags = pick(index & 2 != 0, UP, UP | UV);
            case.sign(&bytes, |ceremony: &mut Ceremony| ceremony.flags = flags);
            let mut assertion = case.assertion.clone();
            let last = assertion.signature.last_mut().expect("a signature");
            *last ^= u8::from(index & 4 != 0);
            assertion
        })
        .collect()
}

fn assertion_index(chosen: &[Failure]) -> usize {
    usize::from(chosen.contains(&OtherChallenge))
        | usize::from(chosen.contains(&NoUserVerification)) << 1
        | usize::from(chosen.contains(&BadSignature)) << 2
}

/// `failing` when the case holds the failure, else `passing`.
fn pick<T>(holds: bool, failing: T, passing: T) -> T {
    if holds { failing } else { passing }
}

/// Sets every field `consume` reads from the passing case and `chosen`, in a fixed order: the
/// instant first, so a cool-off still running is measured from it.
fn arrange(case: &mut Case, chosen: &[Failure], signed: &[OwnedAssertion]) {
    let has = |failure| chosen.contains(&failure);
    let on_time = pick(has(BeforeIssue), (0, -1), (30, 0));
    let (secs, nanos) = pick(has(Expired), (300, 0), on_time);
    case.now = issued_plus(secs, nanos);
    case.environment = Environment::Live;
    case.kind = pick(has(LiveCliConfirm), Kind::CliConfirm, Kind::Passkey);
    let state = pick(has(Used), State::Used, State::Issued);
    case.state = pick(has(Unknown), State::Unknown, state);
    case.principal = pick(has(OtherPrincipal), mallory(), alice());
    case.workspace = pick(has(OtherWorkspace), other_workspace(), workspace());
    let counter = pick(has(CounterNotRising), 8, 7);
    let cool_off_ends = pick(
        has(CoolingOff),
        issued_plus(secs, nanos + 1),
        at(COOL_OFF_ENDS),
    );
    case.enrolled = (!has(NoCredential)).then(|| EnrolledCredential {
        principal_id: pick(has(OthersCredential), mallory(), alice()),
        credential: case.authenticator.credential(counter),
        cool_off_ends,
    });
    case.assertion = signed[assertion_index(chosen)].clone();
    let other = Action {
        kind: StepUpActionKind::Deploy,
        digest: Digest::of(b"another approval's content object"),
    };
    case.action = Action {
        kind: pick(has(OtherKind), other.kind, approval().kind),
        digest: pick(has(OtherDigest), other.digest, approval().digest),
    };
}

#[test]
fn the_refusal_is_the_first_injected_failure_in_spec_order() {
    let all = combinations();
    assert_eq!(all.len(), 2 * 3 * 3 * 2 * 2 * 4 * 2 * 2 * 2 * 2 * 2 * 2);
    let mut case = Case::new();
    let signed = assertions(&mut case);
    let mut wrong = Vec::new();
    for chosen in &all {
        arrange(&mut case, chosen, &signed);
        let result = case.run();
        let expected = match chosen.iter().min() {
            None => Ok(at(stepup::ISSUED)),
            Some(first) => Err(expected(*first)),
        };
        let got = result.map(|consumed| consumed.evidence().authenticated_at);
        if got != expected {
            wrong.push((chosen.clone(), got, expected));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} combinations answered otherwise; the first (failures, got, expected): {:?}",
        wrong.len(),
        all.len(),
        &wrong[..wrong.len().min(5)],
    );
}
