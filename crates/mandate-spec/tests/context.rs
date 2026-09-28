//! `ValidationContext::from_journal` (DEC-169): the context `validate` reads, folded from journaled
//! facts in journal order, with the refusing value wherever a fact is missing.
//!
//! Every test here reads at least one fact, so a stub that ignores its facts, or returns a constant
//! context, fails each of them: a refusing default is only ever asserted beside a value the fold must
//! have read from a fact.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{b, base, s, with, with_all};
use mandate_canon::Digest;
use mandate_domain::{AssetId, Environment};
use mandate_num::{NumError, Usd};
use mandate_spec::context::{AgentId, ContextArgs, JournaledFact, LOSS_CARRY_DAYS, Membership};
use mandate_spec::document::{ConnectionId, ModelId, Pointer, Provenance, ProvenanceMap, Source};
use mandate_spec::validate::{GroupId, PreviousVersion, RegisteredModel, validate};
use mandate_spec::{Mandate, SpecError, ValidationContext, Violation};
use mandate_time::Date;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const OURS: &str = "conn_alpaca_paper_01";
const THEIRS: &str = "conn_alpaca_paper_02";
const X: &str = "7b4a1c2e-aaaa-4a2b-9c3d-00000000000a";
const Y: &str = "7b4a1c2e-bbbb-4a2b-9c3d-00000000000b";
const Z: &str = "7b4a1c2e-cccc-4a2b-9c3d-00000000000c";
const W: &str = "7b4a1c2e-dddd-4a2b-9c3d-00000000000d";
const MODEL_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const DISCLOSURE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn usd(text: &str) -> Usd {
    Usd::parse(text).expect("a dollar amount")
}

fn date(text: &str) -> Date {
    Date::parse(text).expect("a date")
}

fn conn(id: &str) -> ConnectionId {
    ConnectionId::parse(id).expect("a connection id")
}

fn asset(id: &str) -> AssetId {
    AssetId::parse(id).expect("an asset id")
}

fn assets(ids: &[&str]) -> BTreeSet<AssetId> {
    ids.iter().map(|id| asset(id)).collect()
}

fn agent(id: &str) -> AgentId {
    AgentId::new(id)
}

fn digest(hex: &str) -> Digest {
    Digest::from_hex(hex).expect("a digest")
}

fn model_id(id: &str) -> ModelId {
    ModelId::parse(id).expect("a model id")
}

/// The base's one model, registered as the base writes it.
fn momentum(version: &str) -> RegisteredModel {
    RegisteredModel {
        version: version.to_owned(),
        content_hash: digest(MODEL_HASH),
        params: BTreeSet::from(["lookback_bars".to_owned()]),
        admits_instruments: false,
    }
}

/// The draft is agent `a` on our connection, validated on 2026-09-24, one user who approves.
fn args() -> ContextArgs {
    ContextArgs {
        agent: agent("a"),
        connection_id: conn(OURS),
        validation_date: date("2026-09-24"),
        provenance: owner_confirmed_everything(),
        membership: Some(Membership {
            workspace_users: 1,
            approver_users: 1,
        }),
        instrument_groups: BTreeMap::new(),
        eligibility_failures: BTreeSet::new(),
    }
}

/// One entry at the document root: the owner entered and confirmed every field.
fn owner_confirmed_everything() -> ProvenanceMap {
    ProvenanceMap::new(BTreeMap::from([(
        Pointer::new(""),
        Provenance {
            source: Source::UserEntered,
            confirmed: true,
        },
    )]))
}

/// The draft every fold test validates for: the common base.
fn draft() -> Mandate {
    Mandate::parse(&base()).expect("the base parses")
}

fn fold(facts: &[JournaledFact]) -> ValidationContext {
    ValidationContext::from_journal(&draft(), args(), facts).expect("the facts fold")
}

fn snapshot(connection: &str, equity: &str) -> JournaledFact {
    JournaledFact::AccountSnapshot {
        connection_id: conn(connection),
        equity_usd: usd(equity),
    }
}

fn established(connection: &str, environment: Environment) -> JournaledFact {
    JournaledFact::ConnectionEstablished {
        connection_id: conn(connection),
        environment,
    }
}

fn active(who: &str, connection: &str, allocation: &str, pinned: &[&str]) -> JournaledFact {
    JournaledFact::AgentVersionActive {
        agent: agent(who),
        connection_id: conn(connection),
        environment: Environment::Paper,
        allocation_usd: usd(allocation),
        pinned: assets(pinned),
    }
}

fn stopped(who: &str, connection: &str, on: &str, loss: &str) -> JournaledFact {
    JournaledFact::AgentStopped {
        agent: agent(who),
        connection_id: conn(connection),
        retired_on: date(on),
        loss_added_usd: usd(loss),
    }
}

fn universe(who: &str, instrument: &str, admitted: bool) -> JournaledFact {
    JournaledFact::UniverseChanged {
        agent: agent(who),
        instrument: asset(instrument),
        admitted,
    }
}

fn flat(who: &str) -> JournaledFact {
    JournaledFact::AgentFlat { agent: agent(who) }
}

#[test]
#[ignore = "pending E10-1"]
fn equity_is_the_latest_snapshot_on_the_drafts_connection_and_zero_without_one() {
    let read = fold(&[
        snapshot(OURS, "1000"),
        snapshot(THEIRS, "99999"),
        snapshot(OURS, "25000"),
    ]);
    assert_eq!(read.account_equity_usd, usd("25000"));
    let theirs_only = fold(&[snapshot(THEIRS, "99999")]);
    assert_eq!(theirs_only.account_equity_usd, Usd::ZERO);
}

#[test]
#[ignore = "pending E10-1"]
fn the_environment_is_the_connections_own_and_live_when_unknown_or_revoked() {
    let revoked = JournaledFact::ConnectionRevoked {
        connection_id: conn(OURS),
    };
    let cases: [(&str, Vec<JournaledFact>, Environment); 5] = [
        (
            "established paper",
            vec![established(OURS, Environment::Paper)],
            Environment::Paper,
        ),
        ("nothing known", vec![], Environment::Live),
        (
            "another connection's paper",
            vec![established(THEIRS, Environment::Paper)],
            Environment::Live,
        ),
        (
            "established, then revoked",
            vec![established(OURS, Environment::Paper), revoked.clone()],
            Environment::Live,
        ),
        (
            "revoked, then established again",
            vec![revoked, established(OURS, Environment::Paper)],
            Environment::Paper,
        ),
    ];
    for (name, facts, expected) in cases {
        assert_eq!(
            fold(&facts).connection_environment,
            Some(expected),
            "{name}"
        );
    }
}

#[test]
#[ignore = "pending E10-1"]
fn other_allocations_sum_the_other_active_agents_on_the_connection() {
    let mut facts = vec![
        active("a", OURS, "10000", &[]),
        active("b", OURS, "3000", &[]),
        active("c", OURS, "2000", &[]),
        active("d", THEIRS, "5000", &[]),
        active("b", OURS, "4000", &[]),
    ];
    assert_eq!(
        fold(&facts).other_allocations_usd,
        usd("6000"),
        "the draft's own agent and the other connection do not count, and b's latest version does"
    );
    facts.push(stopped("b", OURS, "2026-09-20", "0"));
    assert_eq!(
        fold(&facts).other_allocations_usd,
        usd("2000"),
        "a retired agent holds no allocation"
    );
    facts.push(active("b", OURS, "1500", &[]));
    assert_eq!(
        fold(&facts).other_allocations_usd,
        usd("3500"),
        "a redeployed agent holds its new allocation"
    );
}

#[test]
#[ignore = "pending E10-1"]
fn claims_are_the_other_agents_pinned_and_admitted_instruments_until_they_are_flat() {
    let mut facts = vec![
        active("a", OURS, "10000", &[W]),
        active("b", OURS, "3000", &[X]),
        active("c", OURS, "2000", &[]),
        universe("c", Y, true),
        universe("c", Z, true),
        universe("c", Y, false),
        active("d", THEIRS, "5000", &[W]),
        universe("a", Y, true),
    ];
    assert_eq!(fold(&facts).claimed_by_other_agents, assets(&[X, Z]));
    facts.push(stopped("b", OURS, "2026-09-20", "0"));
    facts.push(flat("c"));
    assert_eq!(
        fold(&facts).claimed_by_other_agents,
        assets(&[X, Z]),
        "a retired agent keeps its claims until it is flat, and a flat live agent keeps its own"
    );
    facts.push(flat("b"));
    assert_eq!(
        fold(&facts).claimed_by_other_agents,
        assets(&[Z]),
        "a retired agent that is flat claims nothing"
    );
}

#[test]
#[ignore = "pending E10-1"]
fn the_loss_carry_sums_retirements_on_the_connection_within_the_window() {
    assert_eq!(LOSS_CARRY_DAYS, 90);
    let facts = [
        stopped("b", OURS, "2026-06-26", "100"),
        stopped("c", OURS, "2026-06-25", "1000"),
        stopped("d", THEIRS, "2026-09-01", "10000"),
        stopped("e", OURS, "2026-09-24", "250"),
        stopped("f", OURS, "2026-09-01", "-500"),
    ];
    assert_eq!(
        fold(&facts).connection_loss_carry_usd,
        usd("350"),
        "90 days back counts and 91 does not; another connection's loss does not; a negative adds \
         nothing rather than lowering the carry"
    );
}

#[test]
#[ignore = "pending E10-1"]
fn the_registry_is_what_is_registered_and_not_withdrawn_and_is_never_unchecked() {
    let carry = model_id("quant.carry");
    let momentum_id = model_id("quant.momentum");
    let facts = [
        JournaledFact::ModelRegistered {
            id: momentum_id.clone(),
            model: momentum("1.0.0"),
        },
        JournaledFact::ModelRegistered {
            id: carry.clone(),
            model: momentum("1.0.0"),
        },
        JournaledFact::ModelWithdrawn { id: carry },
        JournaledFact::ModelRegistered {
            id: momentum_id.clone(),
            model: momentum("1.1.0"),
        },
    ];
    assert_eq!(
        fold(&facts).registry,
        Some(BTreeMap::from([(momentum_id, momentum("1.1.0"))]))
    );
    assert_eq!(
        fold(&[snapshot(OURS, "25000")]).registry,
        Some(BTreeMap::new()),
        "no registration is an empty registry, which V-007 refuses, never `None`, which it skips"
    );
}

#[test]
#[ignore = "pending E10-1"]
fn disclosures_are_every_accepted_version() {
    let other = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let facts = [
        JournaledFact::DisclosureAccepted {
            version: digest(DISCLOSURE),
        },
        JournaledFact::DisclosureAccepted {
            version: digest(other),
        },
    ];
    assert_eq!(
        fold(&facts).disclosures_accepted,
        BTreeSet::from([digest(DISCLOSURE), digest(other)])
    );
}

#[test]
#[ignore = "pending E10-1"]
fn the_previous_version_is_the_drafts_agents_last_active_version() {
    let later = JournaledFact::AgentVersionActive {
        agent: agent("a"),
        connection_id: conn(THEIRS),
        environment: Environment::Live,
        allocation_usd: usd("10000"),
        pinned: BTreeSet::new(),
    };
    let facts = [
        active("a", OURS, "10000", &[]),
        later,
        active("b", OURS, "3000", &[]),
    ];
    assert_eq!(
        fold(&facts).previous_version,
        Some(PreviousVersion {
            environment: Environment::Live,
            connection_id: conn(THEIRS),
        })
    );
    assert_eq!(
        fold(&[active("b", OURS, "3000", &[])]).previous_version,
        None,
        "an agent never deployed has no previous version"
    );
}

#[test]
#[ignore = "pending E10-1"]
fn membership_is_what_the_identity_service_supplied_and_zero_without_it() {
    let facts = [snapshot(OURS, "25000")];
    let mut two = args();
    two.membership = Some(Membership {
        workspace_users: 3,
        approver_users: 2,
    });
    let read = ValidationContext::from_journal(&draft(), two, &facts).expect("the facts fold");
    assert_eq!(
        (
            read.workspace_users,
            read.approver_users,
            read.account_equity_usd
        ),
        (3, 2, usd("25000"))
    );
    let mut none = args();
    none.membership = None;
    let read = ValidationContext::from_journal(&draft(), none, &facts).expect("the facts fold");
    assert_eq!(
        (
            read.workspace_users,
            read.approver_users,
            read.account_equity_usd
        ),
        (0, 0, usd("25000"))
    );
}

#[test]
#[ignore = "pending E10-1"]
fn the_owners_arguments_reach_the_context_unchanged() {
    let mut given = args();
    given.validation_date = date("2026-10-01");
    given.provenance = ProvenanceMap::new(BTreeMap::from([(
        Pointer::new(""),
        Provenance {
            source: Source::PlatformDefault,
            confirmed: false,
        },
    )]));
    given.instrument_groups = BTreeMap::from([(asset(X), GroupId::new("sp500"))]);
    given.eligibility_failures = assets(&[Y]);
    let read = ValidationContext::from_journal(&draft(), given.clone(), &[snapshot(OURS, "7")])
        .expect("the facts fold");
    assert_eq!(read.validation_date, given.validation_date);
    assert_eq!(read.provenance, given.provenance);
    assert_eq!(read.instrument_groups, given.instrument_groups);
    assert_eq!(read.eligibility_failures, given.eligibility_failures);
    assert_eq!(read.account_equity_usd, usd("7"));
}

#[test]
#[ignore = "pending E10-1"]
fn a_sum_the_arithmetic_cannot_hold_is_an_error() {
    let most = "79228162514264337593543950335";
    let facts = [active("b", OURS, most, &[]), active("c", OURS, most, &[])];
    match ValidationContext::from_journal(&draft(), args(), &facts) {
        Err(SpecError::Num(NumError::Overflow)) => {}
        other => panic!("expected an overflow, got {other:?}"),
    }
}

fn entry(source: Source, confirmed: bool) -> Provenance {
    Provenance { source, confirmed }
}

/// DEC-169 item 5 (the coordinator's ruling on #252): the draft's provenance is kept as given, and
/// every part of the document no entry covers is added as `user_entered` and unconfirmed. The walk
/// stops at a covered path, descends only where an entry lies below, and never adds the system fields.
#[test]
#[ignore = "pending E10-1"]
fn an_unmentioned_envelope_path_is_added_unconfirmed() {
    let mut given = args();
    given.provenance = ProvenanceMap::new(BTreeMap::from([
        (
            Pointer::new("/risk/max_drawdown"),
            entry(Source::UserEntered, true),
        ),
        (
            Pointer::new("/behavior/signal_models/0/weight"),
            entry(Source::UserStated, true),
        ),
        (
            Pointer::new("/notifications"),
            entry(Source::PlatformDefault, true),
        ),
    ]));
    let read = ValidationContext::from_journal(&draft(), given.clone(), &[snapshot(OURS, "1")])
        .expect("the facts fold");
    let entries = read.provenance.entries();
    for (path, kept) in given.provenance.entries() {
        assert_eq!(entries.get(path), Some(kept), "`{path}` is kept as given");
    }
    let unconfirmed = entry(Source::UserEntered, false);
    for added in [
        "/risk/max_daily_loss",
        "/risk/drawdown_ladder",
        "/behavior/signal_models/0/id",
        "/behavior/signal_models/0/params",
        "/behavior/description",
        "/autonomy",
        "/capital",
        "/name",
    ] {
        assert_eq!(
            entries.get(&Pointer::new(added)),
            Some(&unconfirmed),
            "`{added}` is mentioned by no entry"
        );
    }
    for absent in [
        "",
        "/risk",
        "/behavior",
        "/behavior/signal_models",
        "/behavior/signal_models/0",
        "/notifications/channels",
        "/mandate_schema_version",
        "/source_text_ref",
    ] {
        assert!(
            !entries.contains_key(&Pointer::new(absent)),
            "`{absent}` is covered, holds an entry below it, or is a system field"
        );
    }
    assert_eq!(
        read.account_equity_usd,
        usd("1"),
        "and the facts are still read"
    );
}

/// The coordinator's test on #252: a document with `admission: auto` and no provenance entries is
/// refused with V-022 (and V-020), where `ProvenanceMap::at`'s default alone would pass it.
#[test]
#[ignore = "pending E10-1"]
fn auto_with_no_provenance_is_v022() {
    let mandate =
        Mandate::parse(&with("/autonomy/admission", Some(s("auto")))).expect("the document parses");
    let mut given = args();
    given.provenance = ProvenanceMap::default();
    let facts = [
        snapshot(OURS, "25000"),
        established(OURS, Environment::Paper),
        JournaledFact::ModelRegistered {
            id: model_id("quant.momentum"),
            model: momentum("1.0.0"),
        },
    ];
    let context = ValidationContext::from_journal(&mandate, given, &facts).expect("the facts fold");
    let report = validate(&mandate, &context).expect("the mandate is evaluable");
    for code in [Violation::V020, Violation::V022] {
        assert!(
            report.violations.contains(&code),
            "{code} must be reported, got {:?}",
            report.violations
        );
    }
    let mut confirmed = args();
    confirmed.provenance = owner_confirmed_everything();
    let context =
        ValidationContext::from_journal(&mandate, confirmed, &facts).expect("the facts fold");
    assert!(
        validate(&mandate, &context)
            .expect("the mandate is evaluable")
            .violations
            .is_empty(),
        "the same draft, confirmed by its owner, breaks nothing"
    );
}

/// The base mandate with leveraged ETPs on, so the disclosure is one of the facts it needs.
fn leveraged_base() -> Mandate {
    let document = with_all(&[
        ("/universe/leveraged_etps_enabled", Some(b(true))),
        (
            "/universe/leveraged_etp_disclosure_version",
            Some(s(&format!("sha256:{DISCLOSURE}"))),
        ),
    ]);
    Mandate::parse(&document).expect("the document parses")
}

/// The facts a mandate that breaks no rule needs, each tagged with what leaving it out takes away.
fn required_facts() -> Vec<(&'static str, Option<JournaledFact>)> {
    vec![
        ("equity", Some(snapshot(OURS, "25000"))),
        ("environment", Some(established(OURS, Environment::Paper))),
        (
            "registration",
            Some(JournaledFact::ModelRegistered {
                id: model_id("quant.momentum"),
                model: momentum("1.0.0"),
            }),
        ),
        (
            "disclosure",
            Some(JournaledFact::DisclosureAccepted {
                version: digest(DISCLOSURE),
            }),
        ),
        ("membership", None),
        ("provenance", None),
    ]
}

#[test]
#[ignore = "pending E10-1"]
fn with_every_fact_the_base_is_valid_and_without_any_one_it_is_refused() {
    let mandate = leveraged_base();
    assert!(
        Mandate::parse(&with("/universe/leveraged_etps_enabled", Some(b(false)))).is_ok(),
        "the plain base parses as well"
    );
    for (left_out, _) in required_facts() {
        let mut given = args();
        let facts: Vec<JournaledFact> = required_facts()
            .into_iter()
            .filter(|(name, _)| *name != left_out)
            .filter_map(|(_, fact)| fact)
            .collect();
        if left_out == "membership" {
            given.membership = None;
        }
        if left_out == "provenance" {
            given.provenance = ProvenanceMap::default();
        }
        let context =
            ValidationContext::from_journal(&mandate, given, &facts).expect("the facts fold");
        let report = validate(&mandate, &context).expect("the mandate is evaluable");
        assert!(
            !report.violations.is_empty(),
            "without the {left_out}, the draft must be refused"
        );
    }
    let facts: Vec<JournaledFact> = required_facts()
        .into_iter()
        .filter_map(|(_, fact)| fact)
        .collect();
    let context =
        ValidationContext::from_journal(&mandate, args(), &facts).expect("the facts fold");
    let report = validate(&mandate, &context).expect("the mandate is evaluable");
    assert!(
        report.violations.is_empty(),
        "with every fact, the draft breaks nothing: {:?}",
        report.violations
    );
}

/// One generated fact: which agent (`a` is the draft's), which connection, and which kind.
#[derive(Debug, Clone)]
enum Step {
    Snapshot {
        ours: bool,
        cents: u32,
    },
    Active {
        who: u8,
        ours: bool,
        cents: u32,
        pin: u8,
    },
    Admit {
        who: u8,
        what: u8,
        admitted: bool,
    },
    Stop {
        who: u8,
        ours: bool,
        days_back: u8,
        cents: i32,
    },
    Flat {
        who: u8,
    },
}

const AGENTS: [&str; 3] = ["a", "b", "c"];
const INSTRUMENTS: [&str; 4] = [X, Y, Z, W];

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (any::<bool>(), 0u32..10_000_000).prop_map(|(ours, cents)| Step::Snapshot { ours, cents }),
        (0u8..3, any::<bool>(), 0u32..10_000_000, 0u8..4).prop_map(|(who, ours, cents, pin)| {
            Step::Active {
                who,
                ours,
                cents,
                pin,
            }
        }),
        (0u8..3, 0u8..4, any::<bool>()).prop_map(|(who, what, admitted)| Step::Admit {
            who,
            what,
            admitted
        }),
        (0u8..3, any::<bool>(), 0u8..120, -1_000_000i32..1_000_000).prop_map(
            |(who, ours, days_back, cents)| Step::Stop {
                who,
                ours,
                days_back,
                cents
            }
        ),
        (0u8..3).prop_map(|who| Step::Flat { who }),
    ]
}

fn name(index: u8) -> &'static str {
    AGENTS.get(usize::from(index)).copied().unwrap_or("a")
}

fn instrument(index: u8) -> &'static str {
    INSTRUMENTS.get(usize::from(index)).copied().unwrap_or(X)
}

fn connection(ours: bool) -> &'static str {
    if ours { OURS } else { THEIRS }
}

/// A whole number of cents as canonical decimal text: no trailing zero, no `-0`.
fn cents(amount: i64) -> Usd {
    let sign = if amount < 0 { "-" } else { "" };
    let magnitude = amount.unsigned_abs();
    let (whole, part) = (magnitude / 100, magnitude % 100);
    let text = match (part, part % 10) {
        (0, _) => format!("{sign}{whole}"),
        (_, 0) => format!("{sign}{whole}.{}", part / 10),
        _ => format!("{sign}{whole}.{part:02}"),
    };
    usd(&text)
}

/// `days_back` days before the validation date, walking back from 2026-09-24 through a calendar the
/// oracle writes out itself.
fn days_before_validation(days_back: u8) -> String {
    const MONTH_DAYS: [(u8, u8); 5] = [(9, 24), (8, 31), (7, 31), (6, 30), (5, 31)];
    let mut left = u32::from(days_back);
    for (month, days) in MONTH_DAYS {
        if left < u32::from(days) {
            return format!("2026-{month:02}-{:02}", u32::from(days) - left);
        }
        left -= u32::from(days);
    }
    "2026-04-01".to_owned()
}

fn to_fact(step: &Step) -> JournaledFact {
    match *step {
        Step::Snapshot { ours, cents: c } => JournaledFact::AccountSnapshot {
            connection_id: conn(connection(ours)),
            equity_usd: cents(i64::from(c)),
        },
        Step::Active {
            who,
            ours,
            cents: c,
            pin,
        } => JournaledFact::AgentVersionActive {
            agent: agent(name(who)),
            connection_id: conn(connection(ours)),
            environment: Environment::Paper,
            allocation_usd: cents(i64::from(c)),
            pinned: assets(&[instrument(pin)]),
        },
        Step::Admit {
            who,
            what,
            admitted,
        } => universe(name(who), instrument(what), admitted),
        Step::Stop {
            who,
            ours,
            days_back,
            cents: c,
        } => JournaledFact::AgentStopped {
            agent: agent(name(who)),
            connection_id: conn(connection(ours)),
            retired_on: date(&days_before_validation(days_back)),
            loss_added_usd: cents(i64::from(c)),
        },
        Step::Flat { who } => flat(name(who)),
    }
}

/// What the fold must produce, computed backwards: for each quantity, find the last step that decides
/// it, rather than replaying the steps forward as the fold does.
struct Expected {
    equity_cents: i64,
    other_allocation_cents: i64,
    carry_cents: i64,
    claims: BTreeSet<&'static str>,
    previous_on_ours: Option<bool>,
}

fn last<F: Fn(&Step) -> bool>(steps: &[Step], found: F) -> Option<usize> {
    steps.iter().rposition(found)
}

fn oracle(steps: &[Step]) -> Expected {
    let equity_cents = last(steps, |s| matches!(s, Step::Snapshot { ours: true, .. }))
        .and_then(|i| steps.get(i))
        .map_or(0, |s| match s {
            Step::Snapshot { cents, .. } => i64::from(*cents),
            _ => 0,
        });
    let mut other_allocation_cents = 0;
    let mut claims = BTreeSet::new();
    for who in 1u8..3 {
        let version = last(
            steps,
            |s| matches!(s, Step::Active { who: w, .. } if *w == who),
        );
        let stop = last(
            steps,
            |s| matches!(s, Step::Stop { who: w, .. } if *w == who),
        );
        let flat_at = last(steps, |s| matches!(s, Step::Flat { who: w } if *w == who));
        let Some(v) = version else { continue };
        let Some(Step::Active {
            ours, cents, pin, ..
        }) = steps.get(v)
        else {
            continue;
        };
        if !*ours {
            continue;
        }
        let retired = stop.is_some_and(|s| s > v);
        if !retired {
            other_allocation_cents += i64::from(*cents);
        }
        let released = retired && flat_at.zip(stop).is_some_and(|(f, s)| f > s);
        if !released {
            claims.insert(instrument(*pin));
            for what in 0u8..4 {
                let admitted = last(
                    steps,
                    |s| matches!(s, Step::Admit { who: w, what: t, .. } if *w == who && *t == what),
                )
                .and_then(|i| steps.get(i))
                .is_some_and(|s| matches!(s, Step::Admit { admitted: true, .. }));
                if admitted {
                    claims.insert(instrument(what));
                }
            }
        }
    }
    let carry_cents = steps
        .iter()
        .map(|s| match s {
            Step::Stop {
                ours: true,
                days_back,
                cents,
                ..
            } if u32::from(*days_back) <= LOSS_CARRY_DAYS => i64::from(*cents).max(0),
            _ => 0,
        })
        .sum();
    let previous_on_ours = last(steps, |s| matches!(s, Step::Active { who: 0, .. }))
        .and_then(|i| steps.get(i))
        .map(|s| matches!(s, Step::Active { ours: true, .. }));
    Expected {
        equity_cents,
        other_allocation_cents,
        carry_cents,
        claims,
        previous_on_ours,
    }
}

/// The fold agrees with an oracle that decides each quantity by its last deciding step, over random
/// interleavings of snapshots, deployments, universe changes, retirements, and flats on two
/// connections. A plain function over a `TestRunner`, because a pending test must not be one a macro
/// generates.
#[test]
#[ignore = "pending E10-1"]
fn the_fold_agrees_with_an_oracle_that_reads_the_journal_backwards() {
    let mut runner = TestRunner::default();
    let outcome = runner.run(&prop::collection::vec(step(), 1..40), |steps| {
        let facts: Vec<JournaledFact> = steps.iter().map(to_fact).collect();
        let read = ValidationContext::from_journal(&draft(), args(), &facts)
            .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
        let expected = oracle(&steps);
        prop_assert_eq!(read.account_equity_usd, cents(expected.equity_cents));
        prop_assert_eq!(
            read.other_allocations_usd,
            cents(expected.other_allocation_cents)
        );
        prop_assert_eq!(read.connection_loss_carry_usd, cents(expected.carry_cents));
        prop_assert_eq!(
            read.claimed_by_other_agents,
            expected
                .claims
                .iter()
                .map(|id| asset(id))
                .collect::<BTreeSet<_>>()
        );
        prop_assert_eq!(
            read.previous_version.map(|p| p.connection_id == conn(OURS)),
            expected.previous_on_ours
        );
        prop_assert_eq!(read.validation_date, date("2026-09-24"));
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
}

#[test]
#[ignore = "pending E10-1"]
fn the_oracle_itself_reads_a_hand_written_journal() {
    let steps = [
        Step::Snapshot {
            ours: true,
            cents: 2_500_000,
        },
        Step::Active {
            who: 1,
            ours: true,
            cents: 300_000,
            pin: 0,
        },
        Step::Admit {
            who: 1,
            what: 2,
            admitted: true,
        },
        Step::Active {
            who: 2,
            ours: false,
            cents: 700_000,
            pin: 1,
        },
        Step::Stop {
            who: 2,
            ours: true,
            days_back: 10,
            cents: 12_345,
        },
        Step::Active {
            who: 0,
            ours: true,
            cents: 1_000_000,
            pin: 3,
        },
    ];
    let expected = oracle(&steps);
    assert_eq!(
        (
            expected.equity_cents,
            expected.other_allocation_cents,
            expected.carry_cents,
            expected.previous_on_ours
        ),
        (2_500_000, 300_000, 12_345, Some(true))
    );
    assert_eq!(expected.claims, BTreeSet::from([X, Z]));
    let facts: Vec<JournaledFact> = steps.iter().map(to_fact).collect();
    let read = ValidationContext::from_journal(&draft(), args(), &facts).expect("the facts fold");
    assert_eq!(read.account_equity_usd, usd("25000"));
    assert_eq!(read.claimed_by_other_agents, assets(&[X, Z]));
}
