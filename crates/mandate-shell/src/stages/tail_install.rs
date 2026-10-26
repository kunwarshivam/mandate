//! The tail's install probes (E7-26, PR #1330's round 2): the in-module pins a review ruling
//! required under DEC-77's 2026-09-27 amendment — "A test that a review ruling requires in an
//! implementation PR goes in-module, under `#[cfg(test)]` in the crate's `src/`" — against the
//! escape the review re-planted in scratch: four ways a wrong wiring of
//! [`crate::tracer::Session::tail_the_control_stream`] left the whole suite green (a body that
//! answers nothing, a call that never reads, a re-sorted read, a fabricated hand). Each probe
//! drives one full cycle over the doubles' world and asserts on the committed
//! [`crate::stages::doubles::Ledger`], read back by parsing bytes, never from the runner's own
//! state.
//!
//! The fixtures are the journal shapes the papers already name: the agent stream as a previous,
//! asking cycle left it ([`asked_agent_stream`]), and the workspace control stream as the
//! founder's CLI leaves it ([`answered_control_stream`]) — journal spec §2, §9.6, §9.7; DEC-155
//! item 2.

use std::collections::BTreeSet;

use mandate_canon::Value;
use mandate_journal::Environment;

use mandate_spec::document::ApproverRef;

use super::doubles::{
    AAPL, FIXTURE_CONNECTION, FixtureMandate, NOW, World, agent_stream, control_stream, output,
    seed, setup,
};
use super::{Admitted, MandateSource};
use crate::adapters::admitted_view;
use crate::error::Cause;
use crate::tracer::ProductionCycle;

/// The ask a previous cycle committed, as the agent stream's replay carries it (mandate spec
/// §6.4, journal spec §9.6 and §9.7): the request with every member the runtime's fold reads,
/// and the delivery that makes it grantable (EI-16). The content hash is the request's own
/// member repeated by the answer, never recomputed by anything (DEC-914 item 1).
const REQUEST: &str = "00000100000000000000000001";
const DELIVERED: &str = "00000100000000000000000008";
const ANSWER: &str = "00000100000000000000000002";
const LATER_ANSWER: &str = "00000100000000000000000003";
const ESTABLISHED: &str = "0000010000000000000000000E";
const FOUNDING: &str = "0000010000000000000000000F";
const DEACTIVATION: &str = "0000010000000000000000000D";
const CONTENT_HASH: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";
/// The founder answered 100 s before the run's clock (the M7 brief's `submitted_at`), and the
/// ask's deadline is still ahead at the answer's effective time (§9.7, DEC-156 item 1).
const ANSWERED_AT: u64 = 1790366300;
const ANSWER_DEADLINE: u64 = 1790367300;

fn requested_payload() -> String {
    format!(
        r#"{{"instrument":"{AAPL}","purpose":"open","asset_class":"us_equity","qty":"1","limit":"255.2","mandate_version":"1","decided_by":"default","combined_score":"0.5","deadline":{ANSWER_DEADLINE},"reference_mark":null,"approvers_required":1,"independent_required":false,"content_hash":"{CONTENT_HASH}"}}"#
    )
}

/// The agent stream as a previous, asking cycle left it: the request at seq 1, its delivery at
/// seq 2.
fn asked_agent_stream(world: &World) -> Result<(), String> {
    seed(
        world,
        &agent_stream(),
        1,
        REQUEST,
        "ApprovalRequested",
        "agent",
        &requested_payload(),
    )?;
    let delivered = format!(
        r#"{{"approval":"{REQUEST}","channel":"cli_inbox","status":"delivered","message_id":"00000100000000000000000009"}}"#
    );
    seed(
        world,
        &agent_stream(),
        2,
        DELIVERED,
        "ApprovalDelivered",
        "agent",
        &delivered,
    )
}

/// One committed answer of the workspace control stream: the founder's `cli_confirm` grant,
/// journaled while no run was open (journal §2, §9.7; DEC-155 item 2).
fn answered_control_stream(world: &World, seq: u64, event_id: &str) -> Result<(), String> {
    answered_by(world, seq, event_id, "user-founder")
}

/// The founder's connection record at the control stream's `seq` 1, the record DEC-953's owner is
/// read from, which the run reads again at each control-stream read (DEC-2384 item 6).
pub(super) fn established(world: &World) -> Result<(), String> {
    let payload = format!(
        r#"{{"connection_id":"{FIXTURE_CONNECTION}","environment":"paper","user":"user-founder"}}"#
    );
    seed(
        world,
        &control_stream(),
        1,
        ESTABLISHED,
        "ConnectionEstablished",
        "user",
        &payload,
    )
}

/// `responder`'s `cli_confirm` grant of the ask at the control stream's `seq`.
fn answered_by(world: &World, seq: u64, event_id: &str, responder: &str) -> Result<(), String> {
    let payload = format!(
        r#"{{"agent":"tracer-aapl","approval":"{REQUEST}","verdict":"approved","content_hash":"{CONTENT_HASH}","submitted_at":{ANSWERED_AT},"step_up":{{"assertion_id":"first-gesture","authenticated_at":{ANSWERED_AT},"method":"cli_confirm"}},"responder":"{responder}","role":"approver"}}"#
    );
    seed(
        world,
        &control_stream(),
        seq,
        event_id,
        "ApprovalResponseSubmitted",
        "user",
        &payload,
    )
}

/// The fixture's mandate with one approver named: the stock fixture's empty approver set is
/// TI-9's no-person shape, which refuses every grant, and the probes ask what an admitted grant
/// does, so the view changes only its `approvers` (admission check 3's list).
pub(super) struct ApprovableMandate {
    pub(super) world: World,
}

impl MandateSource for ApprovableMandate {
    fn admitted(&self) -> Result<Admitted, Cause> {
        let mut admitted = FixtureMandate {
            world: self.world.clone(),
            environment: Environment::Paper,
        }
        .admitted()?;
        admitted.view.approval.approvers = BTreeSet::from(["user-founder".to_owned()]);
        admitted.approvers.references = vec![reference("user:user-founder")?];
        Ok(admitted)
    }
}

fn reference(text: &str) -> Result<ApproverRef, Cause> {
    ApproverRef::parse(text).map_err(|_| Cause::Absent {
        what: "an approver reference",
    })
}

/// The founder's principal in a workspace `mandate workspace found` founded (DEC-991).
const FOUNDER: &str = "01K6C00000000000000000000F";

/// The fixture's mandate in a founded workspace: `role:approver`, which the founder resolved to
/// when the run was admitted (DEC-2384).
struct FoundedMandate {
    world: World,
}

impl MandateSource for FoundedMandate {
    fn admitted(&self) -> Result<Admitted, Cause> {
        let mut admitted = FixtureMandate {
            world: self.world.clone(),
            environment: Environment::Paper,
        }
        .admitted()?;
        admitted.view.approval.approvers = BTreeSet::from([FOUNDER.to_owned()]);
        admitted.approvers.references = vec![reference("role:approver")?];
        Ok(admitted)
    }
}

/// The founding grant of [`FOUNDER`] at the control stream's `seq` 1, committed at the fixture's
/// clock with no cool-off (journal spec §9.12).
fn founded(world: &World) -> Result<(), String> {
    let payload = format!(
        r#"{{"cool_off_ends_at":"{NOW}","independent_approval_required":false,"invitation":null,"member":"{FOUNDER}","reason":"founding","roles":["approver","operator","workspace_admin"]}}"#
    );
    seed(
        world,
        &control_stream(),
        1,
        FOUNDING,
        "MemberActivated",
        "system",
        &payload,
    )
}

/// One cycle over the tailed answer, with every stage the fixture's permissive doubles, whose
/// report shows the keystone's one submission.
fn run_of(world: &World) -> Result<crate::tracer::Report, String> {
    run_with(
        world,
        Box::new(ApprovableMandate {
            world: world.clone(),
        }),
    )
}

/// One cycle as [`run_of`]'s, with `mandate` the mandate stage.
fn run_with(
    world: &World,
    mandate: Box<dyn MandateSource>,
) -> Result<crate::tracer::Report, String> {
    let mut stages = world.stages();
    stages.mandate = mandate;
    let output = output().map_err(|cause| cause.to_string())?;
    ProductionCycle::new(stages, setup()?)
        .run(output)
        .map_err(|error| error.to_string())
}

/// The `ApprovalResponded` copies the ledger holds, parsed from the committed bytes.
fn responded_copies(world: &World) -> Vec<Value> {
    world.ledger.borrow().parsed("ApprovalResponded")
}

fn result_member(copy: &Value) -> Option<&str> {
    copy.get("payload")
        .and_then(|payload| payload.get("result"))
        .and_then(Value::as_str)
}

/// Review round 2's probe P1 (DEC-77's 2026-09-27 amendment, in-module): the open cycle hands
/// the control stream's tailed rows to the runtime as the `Input::Journal` its owner-input
/// routing reads, so an answer committed while no run was open reaches the replayed fold and is
/// copied once: the ledger gains the `ApprovalResponded` whose `causation_id` is the committed
/// answer's own event id, admitted as the grant it is. Kills the nothing-handed wrong wirings — a
/// body that answers nothing, a read that never happens, a hand that replaces the tail.
#[test]
fn the_open_cycle_hands_the_tailed_ctl_rows_to_the_runtime() -> Result<(), String> {
    let world = World::default();
    asked_agent_stream(&world)?;
    established(&world)?;
    answered_control_stream(&world, 2, ANSWER)?;
    let report = run_of(&world)?;
    assert!(
        report.submitted.len() == 1,
        "the all-doubles keystone holds: {report:?}"
    );
    let copies = responded_copies(&world);
    let [copy] = &copies[..] else {
        return Err(format!(
            "the committed answer was copied {} times, not once",
            copies.len()
        ));
    };
    assert_eq!(
        copy.get("causation_id").and_then(Value::as_str),
        Some(ANSWER),
        "the copy names the committed answer as its cause"
    );
    assert_eq!(
        result_member(copy),
        Some("admitted"),
        "the envelope's own user actor carries the founder's grant through"
    );
    Ok(())
}

/// Review round 2's probe P2: the tailed rows reach the runtime in the journal's own order, the
/// copies proving it — the first answer naming the ask admits (DEC-912 item 1: the first
/// answer naming the ask's approval decides it), and the second, however much later it was
/// committed, is refused as not pending; both copies land on the agent stream in the read's
/// own order. Kills the re-sorted read: reversed, the copies swap and the refused one is
/// admitted.
#[test]
fn the_tailed_rows_reach_the_runtime_in_the_journals_own_order() -> Result<(), String> {
    let world = World::default();
    asked_agent_stream(&world)?;
    established(&world)?;
    answered_control_stream(&world, 2, ANSWER)?;
    answered_control_stream(&world, 3, LATER_ANSWER)?;
    run_of(&world)?;
    let copies = responded_copies(&world);
    let [first, second] = &copies[..] else {
        return Err(format!(
            "the committed answers were copied {} times, not two",
            copies.len()
        ));
    };
    assert_eq!(
        first.get("causation_id").and_then(Value::as_str),
        Some(ANSWER),
        "the copies reach the journal in the journal's own order"
    );
    assert_eq!(
        second.get("causation_id").and_then(Value::as_str),
        Some(LATER_ANSWER),
        "the journal's own order is the feed's order, never the reverse"
    );
    assert_eq!(
        result_member(first),
        Some("admitted"),
        "the first answer decides"
    );
    assert_eq!(
        result_member(second),
        Some("refused"),
        "the second answer finds the decided approval"
    );
    assert_eq!(
        second
            .get("payload")
            .and_then(|payload| payload.get("reason"))
            .and_then(Value::as_str),
        Some("not_pending"),
        "a decided approval refuses the later answer of the same ask"
    );
    Ok(())
}

/// The one `ApprovalResponded` copy the ledger holds: its result and refusal reason.
fn the_copy(world: &World) -> Result<(Option<String>, Option<String>), String> {
    let copies = responded_copies(world);
    let [copy] = &copies[..] else {
        return Err(format!("{} copies, not one", copies.len()));
    };
    let reason = copy
        .get("payload")
        .and_then(|payload| payload.get("reason"))
        .and_then(Value::as_str);
    Ok((
        result_member(copy).map(str::to_owned),
        reason.map(str::to_owned),
    ))
}

/// DEC-2384: in a founded workspace the founder's grant is admitted, its approvers resolved from
/// the control stream's membership fold.
#[test]
fn a_founded_workspaces_founder_grant_is_admitted() -> Result<(), String> {
    let world = World::default();
    asked_agent_stream(&world)?;
    founded(&world)?;
    answered_by(&world, 2, ANSWER, FOUNDER)?;
    run_with(
        &world,
        Box::new(FoundedMandate {
            world: world.clone(),
        }),
    )?;
    assert_eq!(the_copy(&world)?, (Some("admitted".to_owned()), None));
    Ok(())
}

/// DEC-2384 item 6: the run resolves its approvers again from the control stream it reads at
/// admission, so a grant from a member deactivated after the run was admitted, and before the
/// grant is, is refused `not_an_approver` and nothing is sent.
#[test]
fn a_grant_from_a_member_deactivated_before_its_admission_is_not_an_approvers() -> Result<(), String>
{
    let world = World::default();
    asked_agent_stream(&world)?;
    founded(&world)?;
    let payload = format!(r#"{{"member":"{FOUNDER}"}}"#);
    seed(
        &world,
        &control_stream(),
        2,
        DEACTIVATION,
        "MemberDeactivated",
        "system",
        &payload,
    )?;
    answered_by(&world, 3, ANSWER, FOUNDER)?;
    let report = run_with(
        &world,
        Box::new(FoundedMandate {
            world: world.clone(),
        }),
    )?;
    assert_eq!(
        the_copy(&world)?,
        (
            Some("refused".to_owned()),
            Some("not_an_approver".to_owned())
        )
    );
    assert!(report.submitted.is_empty(), "nothing is sent: {report:?}");
    Ok(())
}

/// DEC-2384 item 6: a revoked connection names no owner at the read that admits the grant, so the
/// grant DEC-953's owner made before the revocation was read is refused `not_an_approver`.
#[test]
fn a_grant_after_the_connection_was_revoked_is_not_an_approvers() -> Result<(), String> {
    let world = World::default();
    asked_agent_stream(&world)?;
    established(&world)?;
    let payload = format!(r#"{{"connection_id":"{FIXTURE_CONNECTION}","reason":"owner"}}"#);
    seed(
        &world,
        &control_stream(),
        2,
        DEACTIVATION,
        "ConnectionRevoked",
        "user",
        &payload,
    )?;
    answered_control_stream(&world, 3, ANSWER)?;
    run_of(&world)?;
    assert_eq!(
        the_copy(&world)?,
        (
            Some("refused".to_owned()),
            Some("not_an_approver".to_owned())
        )
    );
    Ok(())
}

/// The same view, or its approvers narrowed, is the admitted view; a wider or different
/// approver set, or any other member changed, is not.
#[test]
fn only_a_narrowed_approver_set_is_still_the_admitted_view() -> Result<(), String> {
    let fixture = FixtureMandate {
        world: World::default(),
        environment: Environment::Paper,
    };
    let mut own = fixture.admitted().map_err(|e| e.to_string())?.view;
    own.approval.approvers = approvers(&["a", "b"]);
    let with = |ids: &[&str]| {
        let mut view = own.clone();
        view.approval.approvers = approvers(ids);
        view
    };
    assert!(admitted_view(own.clone(), &own), "the same view");
    assert!(admitted_view(own.clone(), &with(&["a"])), "narrowed");
    assert!(admitted_view(own.clone(), &with(&[])), "narrowed to no one");
    assert!(
        !admitted_view(own.clone(), &with(&["a", "b", "c"])),
        "wider"
    );
    assert!(
        !admitted_view(own.clone(), &with(&["c"])),
        "another approver"
    );
    let mut other = own.clone();
    other.version = "2".to_owned();
    assert!(!admitted_view(own.clone(), &other), "another version");
    let mut other = with(&["a"]);
    other.approval.timeout_s += 1;
    assert!(!admitted_view(own, &other), "narrowed, but another timeout");
    Ok(())
}

fn approvers(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}
