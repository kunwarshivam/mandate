//! The coverage walk of a requested verification run (workspace API §4.8.1 "Coverage" and
//! "The record", AU-6 and AU-8; journal spec §11's per-event checks and §9.13's `VerificationRun`,
//! rules 107, 110, 111, 132 and 133; DEC-787 item 1, DEC-788 items 1 and 2, DEC-897 items 4
//! and 5(a), DEC-916). [`walk`] plans nothing: [`plan`](crate::plan) has already resolved the
//! range and its trusted start, and the walk judges the range's events one position at a time
//! from that start. The run left over is its record — §4.8 has the pure crate return the
//! `VerificationRun` version 2 draft for the API's control-stream writer to append before
//! anything is served (API-16, AU-6), and the members `GET /verifications/{id}` serves rebuild
//! from that record alone (§4.8.1 "The record", "Result").

use mandate_canon::Digest;
use mandate_identity::demand::{Permitted, ReadRecords};
use mandate_journal::{ArtifactSource, Environment, StoredEvent};
use mandate_time::UtcNanos;

use crate::verification::{
    ColdSource, ControlSnapshot, Plan, RecordedStart, VerificationRefusal, VerificationRequest,
};

/// §4.8.1's `first_failure` and the record's `failure` (§9.13 rule 111): the check's code, one
/// of §11's, and the position it was reported at — for a per-event check, the position walked
/// (`from_seq` first, the previous `seq` plus 1 after it), never the `seq` the failing row's
/// body claims; `None` for a check the run order reports for the range as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirstFailure {
    /// §9.13's `failure.check`: one of §11's codes, as the record writes it.
    pub check: &'static str,
    /// §9.13's `failure.seq`: the position the check was reported at, or `None` for the range.
    pub seq: Option<u64>,
}

/// §9.13's `incomplete` (rule 132): the one check the range could not finish, and why. Only the
/// token check can be incomplete at this version (§11 "Incomplete", DEC-789), so the check is
/// [`IncompleteCheck::check`]'s word and only `cause` is data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncompleteCheck {
    /// §9.13's `incomplete.cause`: why the token check could not finish.
    pub cause: IncompleteCause,
}

impl IncompleteCheck {
    /// §9.13's `incomplete.check`: §11's token check, the only one that can be incomplete.
    pub fn check(&self) -> &'static str {
        "tsa_token_invalid"
    }
}

/// Why a range's token check could not finish (§9.13's `incomplete.cause`, DEC-789 items 7 and 9,
/// DEC-896 item 3): a stamped token whose imprint matches that no one can verify yet, or an
/// anchor the range must check whose `token` is `null`. A range that meets both causes records
/// `anchor_unstamped`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncompleteCause {
    TokenUnverifiable,
    AnchorUnstamped,
}

impl IncompleteCause {
    /// The cause word the record writes.
    pub fn code(&self) -> &'static str {
        match self {
            Self::TokenUnverifiable => "token_unverifiable",
            Self::AnchorUnstamped => "anchor_unstamped",
        }
    }
}

/// §4.8.1's `result` over one range, journal rules 132 and 133: `fail` exactly when the walk
/// found a failure, `incomplete` exactly when it found none and a check could not finish, `pass`
/// only when the range was walked whole and every check finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunResult {
    Pass,
    Incomplete,
    Fail,
}

impl RunResult {
    /// The result word the API serves.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Incomplete => "incomplete",
            Self::Fail => "fail",
        }
    }
}

/// The envelope members that are the API writer's, not the run's (journal spec §3, DEC-788
/// item 2, DEC-897 item 4): §3.4 derives the run's `event_id` from the request's
/// `Idempotency-Key`, `event_time` is when the request arrived, and `requester` is the principal
/// that asked. The record's `causation_id` is not the writer's to choose: the requested run
/// claims no cause, so [`walk`] writes it `null` (DEC-897 item 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEnvelope {
    /// §3's `environment`: `paper`, `live`, or `backtest`, as the writer appends in.
    pub environment: Environment,
    /// The run's `event_id`, §3.4's derivation from the request's `Idempotency-Key`
    /// (DEC-788 item 2): the `verification_id` the route returns.
    pub event_id: String,
    /// §3's `event_time`: when the request arrived.
    pub event_time: UtcNanos,
    /// §3's `clock_source`, one of `broker`, `exchange`, `local`, `scheduler`.
    pub clock_source: String,
    /// §3's `correlation_id`, when the request carries one.
    pub correlation_id: Option<String>,
    /// The principal that asked, as rule 110 passes it.
    pub requester: Requester,
}

/// The actor of a requested run's record (§9.13 "Writers", rule 110): the requesting `user`, or
/// the workspace's service account (`system`), nothing else; a platform operator, an agent, a
/// broker, and a client cannot ask for a verification, and rules 110's list refuses their kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requester {
    /// Rule 110's `actor.kind` for a request: `user` or `system`.
    pub kind: RequesterKind,
    /// The principal's opaque id (§6.4).
    pub id: String,
    /// §3's actor `version`.
    pub version: String,
    /// §3's actor `build` (a `sha256:` ref); required for `system` (§3 refuses a null one).
    pub build: Option<Digest>,
}

/// Rule 110's two actor kinds a requested run's record can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequesterKind {
    User,
    System,
}

impl RequesterKind {
    /// The `actor.kind` word the record writes.
    pub fn code(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System => "system",
        }
    }
}

/// §4.8.1's run over one planned range: what the walk found, the result the API serves, and the
/// `VerificationRun` version 2 draft that records it. Only [`walk`] builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    stream_id: String,
    from_seq: u64,
    to_seq: u64,
    checked: u64,
    failure: Option<FirstFailure>,
    incomplete: Option<IncompleteCheck>,
    to_hash: Option<Digest>,
    start: RecordedStart,
    draft: Vec<u8>,
}

impl Run {
    /// §4.8.1's `result` member, rules 132 and 133: `fail` exactly when a failure was found,
    /// `incomplete` exactly when none was and a check could not finish, `pass` otherwise.
    pub fn result(&self) -> RunResult {
        if self.failure.is_some() {
            RunResult::Fail
        } else if self.incomplete.is_some() {
            RunResult::Incomplete
        } else {
            RunResult::Pass
        }
    }

    /// The range's `stream_id` (rule 107: a stream of the record's own workspace).
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }

    /// §4.8.1's `from_seq`: the position the walk judged first.
    pub fn from_seq(&self) -> u64 {
        self.from_seq
    }

    /// §4.8.1's `to_seq`: the resolved last `seq`, never `null` (`Plan::to_seq` resolved it).
    pub fn to_seq(&self) -> u64 {
        self.to_seq
    }

    /// §4.8.1's `checked`: the events of the range the walk walked (AU-8), never more than
    /// `to_seq − from_seq + 1` (rule 132).
    pub fn checked(&self) -> u64 {
        self.checked
    }

    /// §4.8.1's `first_failure`: the first failure §11 reports for the range, `None` when it
    /// found none.
    pub fn first_failure(&self) -> Option<FirstFailure> {
        self.failure
    }

    /// §4.8.1's `incomplete`, `None` while no check could not finish, always `None` on a failed
    /// range (rule 132: a range has one outcome).
    pub fn incomplete(&self) -> Option<IncompleteCheck> {
        self.incomplete
    }

    /// The record's `to_hash` (rule 111): the head the walk verified, `None` when a failure left
    /// none.
    pub fn to_hash(&self) -> Option<Digest> {
        self.to_hash
    }

    /// §4.8.1's `trusted_start` as the record's `start` names it (DEC-788 item 1): the member of
    /// the kind named non-null and the other null.
    pub fn trusted_start(&self) -> &RecordedStart {
        &self.start
    }

    /// §4.8's draft: the `VerificationRun` version 2 bytes the API's control-stream writer
    /// appends before anything is served (API-16, AU-6); the result rebuilds from it alone.
    pub fn draft(&self) -> &[u8] {
        &self.draft
    }
}

/// §4.8.1's coverage walk (AU-8): the events of `from_seq` to `to_seq` of the request's stream,
/// the positions the [`Plan`] resolved, walked in `seq` order from the plan's trusted start,
/// each judged by §11's checks 1 to 6, the first failure reported at the position walked
/// whatever the row there claims (§9.13 rule 111), an event the walk expects and does not find
/// failing `seq_gap` at the expected `seq`, and `checked` counting the positions that passed;
/// `pass` and `incomplete` each require `checked = to_seq − from_seq + 1` (rule 132). The run's
/// record comes out of the walk: the `VerificationRun` version 2 draft of §9.13's shape —
/// `trigger` `request`, one checked range, `causation_id` `null` (DEC-897 item 4) — and the
/// result members §4.8.1 serves, which rebuild from that draft alone.
///
/// `rows` is the request's stream's own rows in the run's one snapshot, `seq` 1 to the head it
/// holds (DEC-787 item 1): the walk judges the range's positions in them and only those, the
/// rows before `from_seq` are the folds' prefix (§4.8.1 "Inputs"), and the rows after `to_seq`
/// are the segment checks' remainder, compared but never counted (§4.8.1 "Coverage"). A row of
/// any other stream — another workspace's, another of the same workspace's, or one whose
/// `stream_id` does not parse — is the one [`VerificationRefusal::NotFound`] of AU-1 and
/// DEC-760 item 4, before any position is judged and with nothing recorded (DEC-916). `snapshot`
/// and `cold` are the same snapshot the plan read: §4.8.1's per-range steps read the control
/// rows and the cold store's manifests from them.
///
/// # Errors
/// [`VerificationRefusal::NotFound`] until DEC-916's row scope holds and the walk has not run;
/// the walk records what it finds, so no other refusal is the walk's before DEC-897 item 5(b)'s
/// unread-token refusal, whose name is still open.
#[allow(
    clippy::too_many_arguments,
    reason = "each is one of §4.8.1's reads or one envelope member's source; bundling them would \
              name a type for one caller"
)]
pub fn walk(
    tenant: &Permitted<'_, ReadRecords>,
    request: &VerificationRequest<'_>,
    plan: &Plan,
    snapshot: &ControlSnapshot<'_>,
    rows: &[StoredEvent],
    cold: &dyn ColdSource,
    artifacts: &dyn ArtifactSource,
    envelope: &RunEnvelope,
) -> Result<Run, VerificationRefusal> {
    let _ = (
        tenant, request, plan, snapshot, rows, cold, artifacts, envelope,
    );
    Err(VerificationRefusal::Unimplemented { story: "E12-3" })
}

#[cfg(test)]
mod tests {
    use mandate_canon::Digest;

    use super::{
        FirstFailure, IncompleteCause, IncompleteCheck, RecordedStart, RequesterKind, Run,
        RunResult,
    };

    fn run_of(failed: Option<u64>, incomplete: Option<IncompleteCause>) -> Run {
        let anchor = "01J8Z3C6A00000000000000R2".to_owned();
        Run {
            stream_id: format!("acct:{}:ACCT1", "ws"),
            from_seq: 4,
            to_seq: 12,
            checked: failed.unwrap_or(9),
            failure: failed.map(|seq| FirstFailure {
                check: "seq_gap",
                seq: Some(seq),
            }),
            incomplete: incomplete.map(|cause| IncompleteCheck { cause }),
            to_hash: failed.is_none().then(|| Digest::of(b"the head walked")),
            start: RecordedStart::Anchor {
                anchor_event_id: anchor,
            },
            draft: b"the record the writer appends".to_vec(),
        }
    }

    #[test]
    fn every_member_of_the_record_reads_back() {
        let passed = run_of(None, Some(IncompleteCause::AnchorUnstamped));
        assert_eq!(
            (
                passed.stream_id().to_owned(),
                passed.from_seq(),
                passed.to_seq(),
                passed.checked(),
                passed.first_failure(),
                passed.incomplete(),
                passed.to_hash(),
                passed.trusted_start().clone(),
                passed.draft().to_vec(),
            ),
            (
                "acct:ws:ACCT1".to_owned(),
                4,
                12,
                9,
                None,
                Some(IncompleteCheck {
                    cause: IncompleteCause::AnchorUnstamped
                }),
                Some(Digest::of(b"the head walked")),
                RecordedStart::Anchor {
                    anchor_event_id: "01J8Z3C6A00000000000000R2".to_owned()
                },
                b"the record the writer appends".to_vec(),
            )
        );
    }

    #[test]
    fn result_follows_rules_132_and_133() {
        let rows = [
            (run_of(Some(7), None), RunResult::Fail),
            (
                run_of(None, Some(IncompleteCause::TokenUnverifiable)),
                RunResult::Incomplete,
            ),
            (run_of(None, None), RunResult::Pass),
            (
                run_of(Some(7), Some(IncompleteCause::TokenUnverifiable)),
                RunResult::Fail,
            ),
        ];
        for (run, want) in rows {
            assert_eq!(run.result(), want);
        }
    }

    #[test]
    fn the_served_and_recorded_words_are_the_specs() {
        let words = [
            (RunResult::Pass.code(), "pass"),
            (RunResult::Incomplete.code(), "incomplete"),
            (RunResult::Fail.code(), "fail"),
            (
                IncompleteCause::TokenUnverifiable.code(),
                "token_unverifiable",
            ),
            (IncompleteCause::AnchorUnstamped.code(), "anchor_unstamped"),
            (
                IncompleteCheck {
                    cause: IncompleteCause::TokenUnverifiable,
                }
                .check(),
                "tsa_token_invalid",
            ),
            (RequesterKind::User.code(), "user"),
            (RequesterKind::System.code(), "system"),
        ];
        for (got, want) in words {
            assert_eq!(got, want);
        }
    }
}
