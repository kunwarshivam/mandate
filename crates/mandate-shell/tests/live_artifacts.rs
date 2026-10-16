//! A live run's artifacts (the R0 rehearsal's R9, DEC-957): `Artifacts::from_registered` takes a
//! live mandate only from a version confirmed with all four of DEC-924 item 1's conditions held,
//! and only when the deployment and every object it binds (the model, fee schedule, calendar, rule
//! set and instrument snapshot) were journaled on one control stream, in the mandate's own
//! environment: the live workspace's own (DEC-952 item 1). A paper object in a live run, a live
//! object in a paper run, and any object from another workspace refuse before anything is sent;
//! a paper run is otherwise unchanged.

mod common;

use mandate_domain::Environment;
use mandate_executor::ExecutorError;
use mandate_shell::Cause;
use mandate_shell::control::{
    Build, Confirmation, DeploymentRefusal, DoorChecks, LiveAdmission, McpHost, Origin, RunFacts,
    confirmed_version,
};
use mandate_shell::paper::Artifacts;
use mandate_spec::context::{AgentId, Membership};
use mandate_time::Date;
use proptest::prelude::*;

use common::{FEE, RULES, SNAPSHOT, Stream, live_origin, paper_origin};

/// The refusal of a binding journaled anywhere but the deployment's own stream and environment.
const FOREIGN: &str = "every binding journaled on the deployment's own control stream";
/// The refusal of a fee schedule that names another environment than the mandate's.
const FEE_ELSEWHERE: &str = "a fee schedule for the mandate's environment";
/// The records [`Stream`] journals each binding as: the model, fee schedule, calendar, rule set
/// and instrument snapshot registrations, then the `AgentDeployed`.
const BINDINGS: [usize; 6] = [0, 1, 2, 3, 4, 7];
const PINNED: &str = "mcp.pinned.example";

fn day() -> Date {
    Date::parse("2026-09-28").unwrap()
}

/// Every one of DEC-924 item 1's four conditions holding.
fn admitted() -> LiveAdmission {
    LiveAdmission {
        build: Build::Live,
        confirmation: Confirmation::ConfirmLive,
        door: DoorChecks::Passed,
        host: McpHost {
            dialed: Some(PINNED.to_owned()),
            pinned: Some(PINNED),
        },
    }
}

fn other_workspace(environment: Environment) -> Origin {
    Origin {
        stream_id: "ctl:ws_other".to_owned(),
        environment,
    }
}

/// `stream`'s artifacts for a run that holds every live condition.
fn artifacts(stream: &Stream) -> Result<Artifacts, Cause> {
    let (confirmed, config) = stream.inputs_admitting(day(), admitted());
    Artifacts::from_registered(&confirmed, &config)
}

/// `stream` with record `index` journaled on `origin` instead of its own stream.
fn moved(mut stream: Stream, index: usize, origin: Origin) -> Stream {
    stream.records[index].origin = origin;
    stream
}

fn absent(answer: &Result<Artifacts, Cause>) -> Option<&'static str> {
    match answer {
        Err(Cause::Absent { what }) => Some(what),
        Ok(_) | Err(_) => None,
    }
}

/// The paper-only fee schedule's own refusal of a live stream (`paper_only_fee_config`): the one
/// check left after every provenance check passed, until a live fee schedule exists (R10).
fn refused_only_by_the_paper_only_fees(answer: &Result<Artifacts, Cause>) -> bool {
    matches!(
        answer,
        Err(Cause::Executor(ExecutorError::EnvironmentMismatch { opened, found }))
            if opened == "live" && found == "paper"
    )
}

/// DEC-957 item 1: a live version confirmed with every condition held, whose deployment and
/// bindings were all journaled on the live workspace's own stream, passes every provenance check
/// and the mandate's paper-only lock; what stops it is the paper-only fee schedule, which refuses
/// a live stream (R10), never the mandate's environment.
#[test]
fn a_live_version_binding_its_own_live_registrations_passes_every_provenance_check() {
    let answer = artifacts(&Stream::live("live"));
    assert!(
        refused_only_by_the_paper_only_fees(&answer),
        "{:?}",
        answer.err()
    );
    assert_eq!(absent(&answer), None);
}

/// DEC-957 item 1: a live version takes no part of the confirmation from a paper run: without
/// all four conditions no version exists to hand to the artifacts at all (DEC-924 item 2).
#[test]
fn a_live_version_exists_only_when_the_run_held_every_condition() {
    let stream = Stream::live("live");
    let mut missing_door = admitted();
    missing_door.door = DoorChecks::NotPassed;
    for live in [LiveAdmission::refusing(), missing_door] {
        let run = RunFacts {
            agent: AgentId::new("agent_spy"),
            validation_date: day(),
            membership: Membership {
                workspace_users: 1,
                approver_users: 1,
            },
            live,
        };
        let refused = confirmed_version(&stream.records, &stream.store, &run).err();
        assert_eq!(refused, Some(DeploymentRefusal::Live));
    }
}

/// DEC-957 item 3: a live run's fee schedule must name the live environment; a schedule that
/// names `paper` is a paper object even when registered on the live stream, and refuses before
/// its figures are read.
#[test]
fn a_live_run_refuses_a_fee_schedule_naming_paper() {
    let answer = artifacts(&Stream::live("paper"));
    assert_eq!(absent(&answer), Some(FEE_ELSEWHERE), "{:?}", answer.err());
}

/// DEC-957 item 2: each binding of a live run journaled on a paper stream, on another
/// workspace's live stream, or on another workspace's paper stream refuses, one at a time.
#[test]
fn a_live_run_refuses_each_binding_from_anywhere_but_its_own_live_stream() {
    for index in BINDINGS {
        for origin in [
            paper_origin(),
            other_workspace(Environment::Live),
            other_workspace(Environment::Paper),
        ] {
            let answer = artifacts(&moved(Stream::live("live"), index, origin.clone()));
            assert_eq!(absent(&answer), Some(FOREIGN), "{index} on {origin:?}");
        }
    }
}

/// DEC-957 item 2: a paper run is unchanged, and refuses a binding journaled on a live stream or
/// on another workspace's stream.
#[test]
fn a_paper_run_refuses_each_binding_from_anywhere_but_its_own_paper_stream() {
    assert!(artifacts(&Stream::deployed(SNAPSHOT, RULES, FEE)).is_ok());
    for index in BINDINGS {
        for origin in [
            live_origin(),
            other_workspace(Environment::Paper),
            other_workspace(Environment::Live),
        ] {
            let stream = moved(
                Stream::deployed(SNAPSHOT, RULES, FEE),
                index,
                origin.clone(),
            );
            let answer = artifacts(&stream);
            assert_eq!(absent(&answer), Some(FOREIGN), "{index} on {origin:?}");
        }
    }
}

/// DEC-957 item 2: a version deployed on a stream of the other environment refuses even when
/// every registration sits beside it there: the stream's environment must be the mandate's.
#[test]
fn a_deployment_journaled_in_the_other_environment_refuses() {
    for (stream, elsewhere) in [
        (Stream::live("live"), paper_origin()),
        (Stream::deployed(SNAPSHOT, RULES, FEE), live_origin()),
    ] {
        let stream = BINDINGS.into_iter().fold(stream, |stream, index| {
            moved(stream, index, elsewhere.clone())
        });
        let answer = artifacts(&stream);
        assert_eq!(absent(&answer), Some(FOREIGN), "{elsewhere:?}");
    }
}

/// DEC-957 item 4: a stored row's origin is its stream and its envelope's environment, read
/// exactly: any text but `paper` or `live` is no origin.
#[test]
fn a_rows_origin_is_its_stream_and_its_exact_environment() {
    assert_eq!(Origin::of("ctl:ws_live", "live"), Some(live_origin()));
    assert_eq!(Origin::of("ctl:ws_spy", "paper"), Some(paper_origin()));
    for text in ["", "Live", "PAPER", "paper ", "demo"] {
        assert_eq!(Origin::of("ctl:ws_spy", text), None, "{text:?}");
    }
}

/// The streams a binding can be journaled on: each environment on the run's own workspace, the
/// other workspace the fixtures name, and a third.
fn origins() -> [Origin; 6] {
    let on = |stream: &str, environment| Origin {
        stream_id: stream.to_owned(),
        environment,
    };
    [
        on("ctl:ws_spy", Environment::Paper),
        on("ctl:ws_spy", Environment::Live),
        on("ctl:ws_live", Environment::Paper),
        on("ctl:ws_live", Environment::Live),
        on("ctl:ws_other", Environment::Paper),
        on("ctl:ws_other", Environment::Live),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// DEC-957 items 1 and 2 over every environment and workspace combination: the artifacts pass
    /// the provenance checks exactly when the deployment's stream is in the mandate's environment
    /// and every binding was journaled on that same stream; otherwise they refuse with nothing
    /// built. The oracle compares each record's origin with the stream the fixture journaled it on.
    #[test]
    fn the_artifacts_bind_only_their_own_streams_registrations(
        live in any::<bool>(),
        choices in proptest::collection::vec(0..6usize, BINDINGS.len()),
    ) {
        let base = if live { Stream::live("live") } else { Stream::deployed(SNAPSHOT, RULES, FEE) };
        let mandate_environment = if live { Environment::Live } else { Environment::Paper };
        let deployed_on = origins()[choices[5]].clone();
        let stream = BINDINGS
            .into_iter()
            .zip(&choices)
            .fold(base, |stream, (index, choice)| moved(stream, index, origins()[*choice].clone()));
        let every_binding_beside_the_deployment =
            choices.iter().all(|choice| origins()[*choice] == deployed_on);
        let provenance_holds = every_binding_beside_the_deployment
            && deployed_on.environment == mandate_environment;
        let answer = artifacts(&stream);
        if !provenance_holds {
            prop_assert_eq!(absent(&answer), Some(FOREIGN), "{:?}", answer.err());
        } else if live {
            prop_assert!(refused_only_by_the_paper_only_fees(&answer), "{:?}", answer.err());
        } else {
            prop_assert!(answer.is_ok(), "{:?}", answer.err());
        }
    }
}
