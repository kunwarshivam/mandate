//! Step-up evidence and the owner commands that need it (DEC-155 item 4, DEC-156 item 8, DEC-158
//! option (c)).

use std::collections::BTreeSet;

use mandate_canon::{Digest, Value, to_canonical};

use crate::content::{ConfirmationCode, ContentHash, confirmation_code, object, text};
use crate::{ApprovalError, RiskClock};

/// How long step-up evidence stays fresh, in seconds (mandate spec §6.4: "within the 5 minutes").
pub const STEP_UP_WINDOW_S: i64 = 300;

/// A step-up gesture's id: usable once per workspace (EI-11).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssertionId(pub String);

/// The only v0 method. The owner re-types the code the CLI printed, which proves the owner read
/// this content and does not authenticate against an identity provider, so it is accepted on a
/// `paper` stream only (DEC-155 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepUpMethod {
    CliConfirm,
}

/// The stream's trading environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Paper,
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepUp {
    pub assertion: AssertionId,
    pub authenticated_at: RiskClock,
    pub method: StepUpMethod,
}

/// Why step-up evidence does not count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StepUpRefusal {
    Missing,
    Stale,
    Reused,
    Method,
}

/// The owner commands that are judged here. The kill switch is not one of them: it has its own
/// entry point, [`kill_switch`], which cannot refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerCommandKind {
    /// Needs no step-up (PX-4): always applies.
    Pause,
    /// Judged when the runtime processes it.
    Resume,
    Stop,
    Acknowledge,
    /// Judged when the owner committed it (DEC-156 item 8).
    OwnerExit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAuthority {
    Apply,
    Refused(StepUpRefusal),
}

/// What a kill switch may do. There is no refusing variant (DEC-158 option (c)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillSwitchAuthority {
    /// Stop and flatten as an automated flatten does, equities waiting for the regular session
    /// (MC-F04), because the evidence did not count for the reason given.
    AutomatedFlatten(StepUpRefusal),
    /// Valid evidence as the owner committed it: the owner-exit privileges apply as well (selling
    /// equities outside the regular session at a confirmed bid, DEC-58, DEC-66).
    OwnerExitPrivileges,
}

/// Brief check 6, judged at `at`: the first failing condition in the order missing, stale, reused,
/// method names the refusal (DEC-165 item 4), and `None` means the evidence counts.
///
/// Fresh means authenticated no more than [`STEP_UP_WINDOW_S`] before `at`, inclusive, and not
/// after it, so a skewed clock fails closed. The age is a checked subtraction: evidence at either
/// end of the clock's range, whose age `i64` cannot hold, is stale rather than a panic or a wrapped
/// age that looks fresh.
pub(crate) fn judge_step_up(
    evidence: Option<&StepUp>,
    at: RiskClock,
    environment: Environment,
    used: &BTreeSet<AssertionId>,
) -> Option<StepUpRefusal> {
    let Some(evidence) = evidence else {
        return Some(StepUpRefusal::Missing);
    };
    let fresh =
        at.0.checked_sub(evidence.authenticated_at.0)
            .is_some_and(|age| (0..=STEP_UP_WINDOW_S).contains(&age));
    if !fresh {
        return Some(StepUpRefusal::Stale);
    }
    if used.contains(&evidence.assertion) {
        return Some(StepUpRefusal::Reused);
    }
    match (evidence.method, environment) {
        (StepUpMethod::CliConfirm, Environment::Paper) => None,
        (StepUpMethod::CliConfirm, Environment::Live) => Some(StepUpRefusal::Method),
    }
}

/// Judges an owner command's step-up evidence. `committed_at` is the command's `submitted_at` on
/// the control stream; `processed_at` is the runtime's folded clock when it reads it. Pause always
/// applies (PX-4); an owner exit is judged at `committed_at`, and resume, Stop, and acknowledge at
/// `processed_at` (DEC-156 item 8).
///
/// # Errors
/// None: every input has an answer. The `Result` is the stub API's shape.
pub fn owner_command(
    kind: OwnerCommandKind,
    evidence: Option<&StepUp>,
    committed_at: RiskClock,
    processed_at: RiskClock,
    environment: Environment,
    used: &BTreeSet<AssertionId>,
) -> Result<CommandAuthority, ApprovalError> {
    let at = match kind {
        OwnerCommandKind::Pause => return Ok(CommandAuthority::Apply),
        OwnerCommandKind::OwnerExit => committed_at,
        OwnerCommandKind::Resume | OwnerCommandKind::Stop | OwnerCommandKind::Acknowledge => {
            processed_at
        }
    };
    Ok(judge_step_up(evidence, at, environment, used)
        .map_or(CommandAuthority::Apply, CommandAuthority::Refused))
}

/// Judges a kill switch's evidence at the moment the owner committed it. Never refuses: evidence
/// that does not count, malformed evidence included, still stops and flattens.
///
/// # Errors
/// None, ever (DEC-165 item 5). A runtime that meets one anyway reads it as
/// [`KillSwitchAuthority::AutomatedFlatten`], never as a refusal.
pub fn kill_switch(
    evidence: Option<&StepUp>,
    committed_at: RiskClock,
    environment: Environment,
    used: &BTreeSet<AssertionId>,
) -> Result<KillSwitchAuthority, ApprovalError> {
    Ok(
        match judge_step_up(evidence, committed_at, environment, used) {
            None => KillSwitchAuthority::OwnerExitPrivileges,
            Some(why) => KillSwitchAuthority::AutomatedFlatten(why),
        },
    )
}

/// What a kill switch reaches, as the owner typed it (trading-domain spec §5.5). The runtime owns
/// what each scope does; here it only binds the confirmation code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillScope {
    Agent(String),
    Connection(String),
    Workspace,
}

/// The code `mandate agent kill` asks the owner to type, computed on the owner's host from the
/// scope typed and the control stream's head (DEC-155 item 4): no network, identity provider,
/// runtime, or model state. The head is the one the CLI reads anyway to append the command, so a
/// code typed for one scope, or before another command landed, does not confirm a different one.
///
/// The code is the first digits of the SHA-256 of a canonical object that names what it confirms
/// (`kill_switch`), the scope's kind and id, and the head as decimal text, so it is never the code
/// of an approval's content object and the head needs no canonical integer bound.
///
/// # Errors
/// Never in practice: every key is a fixed valid one, and a SHA-256 hex digest always has a code's
/// digits.
pub fn kill_switch_code(
    scope: &KillScope,
    control_head: u64,
) -> Result<ConfirmationCode, ApprovalError> {
    let (kind, target) = match scope {
        KillScope::Agent(id) => ("agent", text(id)),
        KillScope::Connection(id) => ("connection", text(id)),
        KillScope::Workspace => ("workspace", Value::Null),
    };
    let confirmed = object([
        ("confirms", text("kill_switch")),
        ("control_head", text(&control_head.to_string())),
        ("scope", text(kind)),
        ("target", target),
    ])?;
    confirmation_code(&ContentHash(Digest::of(&to_canonical(&confirmed))))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const T0: i64 = 1_789_999_200;

    fn evidence(authenticated_at: i64) -> StepUp {
        StepUp {
            assertion: AssertionId("assertion-1".to_owned()),
            authenticated_at: RiskClock(authenticated_at),
            method: StepUpMethod::CliConfirm,
        }
    }

    fn kill(
        authenticated_at: i64,
        committed_at: i64,
    ) -> Result<KillSwitchAuthority, ApprovalError> {
        kill_switch(
            Some(&evidence(authenticated_at)),
            RiskClock(committed_at),
            Environment::Paper,
            &BTreeSet::new(),
        )
    }

    /// Freshness on `i128`, where the difference of any two `i64` clocks fits, so the oracle's
    /// subtraction never fails and shares no overflow case with [`judge_step_up`]'s.
    fn fresh_by_i128(authenticated_at: i64, at: i64) -> bool {
        i128::from(at)
            .checked_sub(i128::from(authenticated_at))
            .is_some_and(|age| (0..=i128::from(STEP_UP_WINDOW_S)).contains(&age))
    }

    /// The #240 gap: evidence at the clock's extremes, whose age an unchecked `i64` subtraction
    /// would panic on or wrap into a fresh-looking one, never errs and never grants the owner-exit
    /// privileges unless it really is fresh (DEC-158 option (c), DEC-165 item 5).
    #[test]
    fn a_kill_switch_with_evidence_at_the_clocks_extremes_still_stops_and_flattens() {
        let stale = [
            (i64::MIN, T0),
            (i64::MAX, T0),
            (i64::MIN, i64::MAX),
            (i64::MAX, i64::MIN),
            (0, i64::MIN),
            (i64::MIN, 0),
            (i64::MIN, i64::MIN + STEP_UP_WINDOW_S + 1),
            (i64::MAX - STEP_UP_WINDOW_S - 1, i64::MAX),
            (i64::MIN + 1, i64::MIN),
        ];
        for (authenticated_at, committed_at) in stale {
            assert_eq!(
                kill(authenticated_at, committed_at),
                Ok(KillSwitchAuthority::AutomatedFlatten(StepUpRefusal::Stale)),
                "evidence at {authenticated_at}, committed at {committed_at}"
            );
        }
        let fresh = [
            (i64::MAX, i64::MAX),
            (i64::MIN, i64::MIN),
            (i64::MIN, i64::MIN + STEP_UP_WINDOW_S),
            (i64::MAX - STEP_UP_WINDOW_S, i64::MAX),
        ];
        for (authenticated_at, committed_at) in fresh {
            assert_eq!(
                kill(authenticated_at, committed_at),
                Ok(KillSwitchAuthority::OwnerExitPrivileges),
                "evidence at {authenticated_at}, committed at {committed_at}"
            );
        }
    }

    /// The same extremes through every owner command that needs step-up, at the moment each is
    /// judged: nothing panics, stale evidence is refused, and evidence exactly as old as the
    /// window allows, or as new as the clock, applies.
    #[test]
    fn owner_commands_with_evidence_at_the_clocks_extremes_are_judged_without_overflow() {
        let judged = [
            OwnerCommandKind::Resume,
            OwnerCommandKind::Stop,
            OwnerCommandKind::Acknowledge,
            OwnerCommandKind::OwnerExit,
        ];
        let cases = [
            (
                i64::MIN,
                i64::MAX,
                CommandAuthority::Refused(StepUpRefusal::Stale),
            ),
            (
                i64::MAX,
                i64::MIN,
                CommandAuthority::Refused(StepUpRefusal::Stale),
            ),
            (i64::MAX, i64::MAX, CommandAuthority::Apply),
            (
                i64::MIN,
                i64::MIN + STEP_UP_WINDOW_S,
                CommandAuthority::Apply,
            ),
        ];
        for kind in judged {
            for (authenticated_at, at, expected) in cases {
                assert_eq!(
                    owner_command(
                        kind,
                        Some(&evidence(authenticated_at)),
                        RiskClock(at),
                        RiskClock(at),
                        Environment::Paper,
                        &BTreeSet::new(),
                    ),
                    Ok(expected),
                    "{kind:?}, evidence at {authenticated_at}, judged at {at}"
                );
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

        /// Over the whole `i64` clock, extremes weighted in, a kill switch answers for every pair
        /// and grants the privileges exactly when the `i128` oracle says the evidence is fresh.
        #[test]
        fn a_kill_switch_matches_the_i128_oracle_over_the_whole_clock(
            authenticated_at in prop_oneof![
                Just(i64::MIN), Just(i64::MAX), Just(0i64), any::<i64>(),
                (i64::MIN..i64::MIN + 400), (i64::MAX - 400..=i64::MAX),
            ],
            committed_at in prop_oneof![
                Just(i64::MIN), Just(i64::MAX), Just(0i64), any::<i64>(),
                (i64::MIN..i64::MIN + 400), (i64::MAX - 400..=i64::MAX),
            ],
        ) {
            let expected = if fresh_by_i128(authenticated_at, committed_at) {
                KillSwitchAuthority::OwnerExitPrivileges
            } else {
                KillSwitchAuthority::AutomatedFlatten(StepUpRefusal::Stale)
            };
            prop_assert_eq!(kill(authenticated_at, committed_at), Ok(expected));
        }
    }
}
