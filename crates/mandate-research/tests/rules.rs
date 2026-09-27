//! The rule logic this tests PR carries **live** rather than as a stub, pinned by tests that run
//! unignored.
//!
//! Three things are real code here and not `Unimplemented`: `RefusalReason`'s ordinals, codes and
//! ignored-output predicate; `PolicyOverlay`'s six stricter-of rules; and `group_of`. Each is a total
//! function of its arguments that the stubs never call, and each states a rule the brief relies on —
//! the §8.5 order, §4.3's "a child may only tighten", and trading spec §7.1's default group.
//!
//! They stay live because a stub cannot hold a rule, and the whole point of interpretations 4, 12 and
//! 22 is that these three are written down exactly once. What DEC-83 asks in exchange is that nothing
//! live goes unverified: every branch below is asserted here, unignored, so
//! `cargo mutants -p mandate-research` reaches them and the tests PR body reports the result. The
//! pending tests cannot do that job — cargo-mutants runs the suite, and an `#[ignore]` test does not
//! run.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Digest;
use mandate_research::{
    AdmissionChange, AdmissionDecision, AutonomyDecision, ContentHash, Corroboration, FoldStep,
    GroupId, InstrumentGroup, Invalidation, Lineage, LineageId, LineageState, ModelId,
    ModelVersion, PolicyOverlay, RefusalReason, ResearchError, SchemaDec, SourceId, ThesisId,
    UniverseChangeReason, WorkspaceId, group_of,
};

mod common;

use common::{INSTRUMENT_2, INSTRUMENT_5, asset, usd};
use mandate_research::AssetId;

/// §8.5's order is the enum's order, and every ordinal from 1 to 17 appears exactly once.
#[test]
fn every_refusal_reason_carries_its_own_ordinal_once() {
    let all = RefusalReason::all();

    assert_eq!(all.len(), 17, "§8.5 states seventeen checks");
    let ordinals: Vec<u8> = all.iter().map(|r| r.check_number()).collect();
    assert_eq!(
        ordinals,
        (1..=17).collect::<Vec<u8>>(),
        "`all` is in the spec's order and `check_number` agrees with the position"
    );
}

/// Each reason's journaled code, in §8.5's order. Spelled out so a renamed code is a test failure and
/// not a silent change to what the journal carries (ES-09).
#[test]
fn every_refusal_reason_has_the_codes_the_spec_table_states() {
    let codes: Vec<&str> = RefusalReason::all().iter().map(|r| r.code()).collect();

    assert_eq!(
        codes,
        vec![
            "direction_not_allowed",
            "horizon_mismatch",
            "revision_without_predecessor",
            "research_disabled",
            "universe_pinned",
            "admission_denied",
            "cost_cap_reached",
            "not_in_data_universe",
            "operator_halt",
            "not_allowed_asset_class",
            "leveraged_etp_not_enabled",
            "eligibility_floor",
            "instrument_group_claimed",
            "source_not_allowlisted",
            "no_corroboration",
            "lineage_retired",
            "universe_full",
        ],
        "the reason a refusal journals is the spec's own identifier"
    );
    let unique: std::collections::BTreeSet<&str> = codes.iter().copied().collect();
    assert_eq!(unique.len(), codes.len(), "no two checks share a code");
}

/// §8.2: exactly checks 1 to 3 are ignored outputs. Asserted over all seventeen, so neither a missing
/// nor an extra variant can pass.
#[test]
fn ignored_outputs_are_exactly_the_first_three_checks() {
    for reason in RefusalReason::all() {
        assert_eq!(
            reason.is_ignored_output(),
            reason.check_number() <= 3,
            "{} is check {} and should{} be an ignored output",
            reason.code(),
            reason.check_number(),
            if reason.check_number() <= 3 {
                ""
            } else {
                " not"
            }
        );
    }
}

/// §4.3, a maximum: the effective `max_instruments` is the lower of the mandate's and the policy's,
/// and an absent policy value constrains nothing.
#[test]
fn a_maximum_takes_the_lower_of_the_two_bounds() {
    let none = PolicyOverlay::permissive();
    assert_eq!(
        none.effective_max_instruments(5),
        5,
        "no ceiling, no change"
    );

    let lower = PolicyOverlay {
        max_instruments: Some(3),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(lower.effective_max_instruments(5), 3, "the policy binds");

    let higher = PolicyOverlay {
        max_instruments: Some(9),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(
        higher.effective_max_instruments(5),
        5,
        "a looser policy never raises the mandate's own limit (MI-16)"
    );

    let equal = PolicyOverlay {
        max_instruments: Some(5),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(equal.effective_max_instruments(5), 5, "equal is either");
}

/// The revision cap is a maximum too (DEC-111 item 4, §4.3).
#[test]
fn the_revision_cap_takes_the_lower_of_the_two_bounds() {
    let none = PolicyOverlay::permissive();
    assert_eq!(none.effective_max_revisions_per_lineage(3), 3);

    let lower = PolicyOverlay {
        max_revisions_per_lineage: Some(1),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(
        lower.effective_max_revisions_per_lineage(3),
        1,
        "the policy binds"
    );

    let higher = PolicyOverlay {
        max_revisions_per_lineage: Some(5),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(
        higher.effective_max_revisions_per_lineage(3),
        3,
        "a looser policy never lifts the mandate's cap"
    );
}

/// The cost cap is a maximum in dollars (DEC-120), compared exactly on `Usd`.
#[test]
fn the_cost_cap_takes_the_lower_of_the_two_amounts() {
    let none = PolicyOverlay::permissive();
    assert_eq!(none.effective_cost_cap(usd("5")), usd("5"));

    let lower = PolicyOverlay {
        research_cost_cap_usd_per_day: Some(usd("2.5")),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(
        lower.effective_cost_cap(usd("5")),
        usd("2.5"),
        "the policy binds"
    );

    let higher = PolicyOverlay {
        research_cost_cap_usd_per_day: Some(usd("10")),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(
        higher.effective_cost_cap(usd("5")),
        usd("5"),
        "a looser policy never raises the owner's own cap"
    );

    let equal = PolicyOverlay {
        research_cost_cap_usd_per_day: Some(usd("5")),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(
        equal.effective_cost_cap(usd("5")),
        usd("5"),
        "equal is either"
    );
}

/// §8.4, DEC-123: `stagger_window_s` is a **minimum**, so the effective value is the higher of the
/// two — the opposite direction from a maximum, which is why it has its own test.
#[test]
fn a_minimum_takes_the_higher_of_the_two_bounds() {
    let none = PolicyOverlay::permissive();
    assert_eq!(
        none.effective_stagger_window_s(900),
        900,
        "no floor, no change"
    );

    let higher = PolicyOverlay {
        stagger_window_s: Some(1_800),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(
        higher.effective_stagger_window_s(900),
        1_800,
        "a higher policy floor lengthens the wait"
    );

    let lower = PolicyOverlay {
        stagger_window_s: Some(300),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(
        lower.effective_stagger_window_s(900),
        900,
        "a lower policy floor never shortens the mandate's own wait"
    );
}

/// The proposal interval is a minimum as well (§8.4).
#[test]
fn the_proposal_interval_takes_the_higher_of_the_two_bounds() {
    let none = PolicyOverlay::permissive();
    assert_eq!(none.effective_research_interval_s(3_600), 3_600);

    let higher = PolicyOverlay {
        research_interval_s: Some(7_200),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(
        higher.effective_research_interval_s(3_600),
        7_200,
        "the floor binds"
    );

    let lower = PolicyOverlay {
        research_interval_s: Some(60),
        ..PolicyOverlay::permissive()
    };
    assert_eq!(
        lower.effective_research_interval_s(3_600),
        3_600,
        "a lower floor never makes the agent propose more often"
    );
}

/// §4.3: `auto` evaluates as `ask` once `admission_auto_allowed` is false, and the overlay never
/// loosens a stricter mandate value (MI-17). All three decisions, both ways.
#[test]
fn the_admission_ceiling_only_ever_tightens() {
    let allowed = PolicyOverlay::permissive();
    for decision in [
        AutonomyDecision::Auto,
        AutonomyDecision::Ask,
        AutonomyDecision::Deny,
    ] {
        assert_eq!(
            allowed.effective_admission(decision),
            decision,
            "with auto allowed, the mandate's own value governs"
        );
    }

    let forbidden = PolicyOverlay {
        admission_auto_allowed: false,
        ..PolicyOverlay::permissive()
    };
    assert_eq!(
        forbidden.effective_admission(AutonomyDecision::Auto),
        AutonomyDecision::Ask,
        "DEC-103: with admission_auto_allowed false, auto evaluates as ask"
    );
    assert_eq!(
        forbidden.effective_admission(AutonomyDecision::Ask),
        AutonomyDecision::Ask,
        "ask stays ask"
    );
    assert_eq!(
        forbidden.effective_admission(AutonomyDecision::Deny),
        AutonomyDecision::Deny,
        "deny is stricter than ask, so it survives — the overlay never loosens"
    );
}

/// DEC-103's internal research profile, field by field: the research agent on, **every admission
/// `ask`**, at most three revisions per lineage, and the 900 s stagger floor of DEC-123. These are the
/// thin slice's numbers, so a change to any of them is a change to what the slice permits.
#[test]
fn the_internal_research_profile_carries_the_thin_slices_own_values() {
    let profile = PolicyOverlay::internal_research_profile();

    assert!(
        profile.research_agent_allowed,
        "DEC-103: the thin slice is where the research agent runs"
    );
    assert!(
        !profile.admission_auto_allowed,
        "DEC-103: every admission in the thin slice is ask"
    );
    assert_eq!(
        profile.effective_admission(AutonomyDecision::Auto),
        AutonomyDecision::Ask,
        "which is what that flag means at an admission"
    );
    assert_eq!(
        profile.max_revisions_per_lineage,
        Some(3),
        "DEC-103 caps the thin slice at three revisions per lineage"
    );
    assert_eq!(
        profile.stagger_window_s,
        Some(900),
        "DEC-123's 900 s window is a policy minimum"
    );
    assert_eq!(
        profile.effective_max_instruments(5),
        5,
        "the profile states no instrument ceiling of its own, so the mandate's governs"
    );
    assert_eq!(
        profile.research_cost_cap_usd_per_day, None,
        "nor a cost ceiling: DEC-120's cap is the owner's envelope field"
    );
}

/// The permissive overlay used by every hand test constrains nothing and permits both. A test that
/// started from an overlay which quietly forbade something would make its own scenario unreachable.
#[test]
fn the_permissive_overlay_constrains_nothing_and_permits_both() {
    let p = PolicyOverlay::permissive();

    assert!(
        p.research_agent_allowed && p.admission_auto_allowed,
        "both permissions granted"
    );
    assert_eq!(
        (
            p.max_instruments,
            p.max_revisions_per_lineage,
            p.stagger_window_s,
            p.research_interval_s
        ),
        (None, None, None, None),
        "no ceiling and no floor, so the mandate's own values govern every check"
    );
    assert_eq!(p.effective_max_instruments(5), 5);
    assert_eq!(p.effective_max_revisions_per_lineage(3), 3);
    assert_eq!(p.effective_stagger_window_s(900), 900);
    assert_eq!(p.effective_research_interval_s(3_600), 3_600);
    assert_eq!(p.effective_cost_cap(usd("5")), usd("5"));
    assert_eq!(
        p.effective_admission(AutonomyDecision::Auto),
        AutonomyDecision::Auto,
        "and auto stays auto"
    );
}

/// Trading spec §7.1: an instrument with a named group is in it; one without stands as its own group.
#[test]
fn a_named_group_wins_and_an_unnamed_instrument_is_its_own_group() {
    let group = GroupId::new("grp.megacap").expect("a fixture group id is not empty");
    let mut groups = BTreeMap::new();
    groups.insert(asset(INSTRUMENT_5), group.clone());

    assert_eq!(
        group_of(&asset(INSTRUMENT_5), &groups),
        InstrumentGroup::Named(group),
        "a named group is the one the claim check compares"
    );
    assert_eq!(
        group_of(&asset(INSTRUMENT_2), &groups),
        InstrumentGroup::Ungrouped(asset(INSTRUMENT_2)),
        "an instrument no policy groups stands as its own group"
    );
}

/// DEC-132 item 12: a group id that spells an asset id is a different value from that instrument's
/// own `Ungrouped` group, so it cannot claim it. This is the hole the enum closes.
#[test]
fn a_group_id_spelling_an_asset_id_is_not_that_instruments_group() {
    let spelled = GroupId::new(INSTRUMENT_5).expect("a fixture group id is not empty");
    let mut groups = BTreeMap::new();
    groups.insert(asset(INSTRUMENT_2), spelled);

    assert_ne!(
        group_of(&asset(INSTRUMENT_2), &groups),
        group_of(&asset(INSTRUMENT_5), &groups),
        "instrument 2's named group and instrument 5's own group must never compare equal"
    );
}

/// An empty group map leaves every instrument in its own group, which is the common case.
#[test]
fn an_empty_group_map_leaves_every_instrument_ungrouped() {
    let groups = BTreeMap::new();

    for id in [INSTRUMENT_2, INSTRUMENT_5] {
        assert_eq!(
            group_of(&asset(id), &groups),
            InstrumentGroup::Ungrouped(asset(id)),
            "with no policy groups, a claim on one instrument claims only that instrument"
        );
    }
}

/// Every id and value type round-trips its own text, and every `code()` returns the identifier the
/// journal carries. A table test rather than one per type, so adding a type without adding a row is a
/// compile error here rather than an untested accessor (round 2's finding 4, and the shape
/// `mandate-spec`'s vocabulary tests use).
#[test]
fn every_accessor_returns_its_own_text() {
    let digest_hex = "4444444444444444444444444444444444444444444444444444444444444444";

    assert_eq!(asset(INSTRUMENT_5).as_str(), INSTRUMENT_5, "AssetId");
    assert_eq!(
        GroupId::new("grp.megacap")
            .expect("a fixture group id is not empty")
            .as_str(),
        "grp.megacap",
        "GroupId"
    );
    assert_eq!(
        SchemaDec::from_checked_text("0.8").as_str(),
        "0.8",
        "SchemaDec keeps the field's text, unrounded"
    );
    assert_eq!(
        ThesisId::new("th-1")
            .expect("a fixture thesis id is not empty")
            .as_str(),
        "th-1",
        "ThesisId"
    );
    assert_eq!(
        LineageId::new("th-1")
            .expect("a fixture lineage id is not empty")
            .as_str(),
        "th-1",
        "LineageId"
    );
    assert_eq!(
        SourceId::new("src.filings")
            .expect("a fixture source id is not empty")
            .as_str(),
        "src.filings",
        "SourceId"
    );
    assert_eq!(
        WorkspaceId::new("ws_a")
            .expect("a fixture workspace id is not empty")
            .as_str(),
        "ws_a",
        "WorkspaceId"
    );
    assert_eq!(
        ModelId::new("llm.research_agent")
            .expect("a fixture model id is not empty")
            .as_str(),
        "llm.research_agent",
        "ModelId"
    );
    assert_eq!(ModelVersion::new("0.1.0").as_str(), "0.1.0", "ModelVersion");
    assert_eq!(
        Invalidation::new("Guidance is cut.")
            .expect("a fixture invalidation is not empty")
            .as_str(),
        "Guidance is cut.",
        "Invalidation keeps the text the approval screen shows (DEC-126)"
    );
    let digest = Digest::from_hex(digest_hex).expect("a fixture digest is 32 hex bytes");
    assert_eq!(
        ContentHash::new(digest).digest().to_hex(),
        digest_hex,
        "ContentHash carries the pinned digest unchanged (§8.1, DEC-67)"
    );
}

/// Every id type rejects the empty string, which is the whole of its validation.
#[test]
fn no_id_type_accepts_the_empty_string() {
    assert!(AssetId::new("").is_err(), "AssetId");
    assert!(GroupId::new("").is_err(), "GroupId");
    assert!(ThesisId::new("").is_err(), "ThesisId");
    assert!(LineageId::new("").is_err(), "LineageId");
    assert!(SourceId::new("").is_err(), "SourceId");
    assert!(WorkspaceId::new("").is_err(), "WorkspaceId");
    assert!(ModelId::new("").is_err(), "ModelId");
    assert!(
        Invalidation::new("   ").is_err(),
        "§8.4 asks a thesis what would end it, so whitespace is not an answer"
    );
    assert!(
        AssetId::new(INSTRUMENT_5).is_ok() && Invalidation::new("x").is_ok(),
        "and a non-empty value is accepted, so the check is not simply always failing"
    );
}

/// Every `UniverseChangeReason` and `Corroboration` code, and every `ResearchError` code: the strings
/// the journal and a reader see (ES-09, journal spec §9).
#[test]
fn every_code_is_the_identifier_the_journal_carries() {
    let reasons = [
        (UniverseChangeReason::ThesisAdmitted, "thesis_admitted"),
        (UniverseChangeReason::ThesisExpired, "thesis_expired"),
        (
            UniverseChangeReason::ThesisInvalidated,
            "thesis_invalidated",
        ),
        (UniverseChangeReason::LineageRetired, "lineage_retired"),
        (UniverseChangeReason::EligibilityLost, "eligibility_lost"),
        (UniverseChangeReason::OperatorHalt, "operator_halt"),
        (UniverseChangeReason::VersionApplied, "version_applied"),
    ];
    for (reason, code) in reasons {
        assert_eq!(reason.code(), code, "{reason:?}");
    }
    let codes: BTreeSet<&str> = reasons.iter().map(|(r, _)| r.code()).collect();
    assert_eq!(codes.len(), 7, "no two reasons share a code");

    assert_eq!(
        Corroboration::IndependentSource.code(),
        "independent_source"
    );
    assert_eq!(Corroboration::MarketData.code(), "market_data");

    let errors = [
        (ResearchError::UniverseUnavailable, "universe_unavailable"),
        (ResearchError::DuplicateInstrument, "duplicate_instrument"),
        (ResearchError::DuplicateThesisId, "duplicate_thesis_id"),
        (
            ResearchError::SessionCalendarMissing,
            "session_calendar_missing",
        ),
        (ResearchError::WindowTooLarge, "window_too_large"),
        (ResearchError::IntervalTooLarge, "interval_too_large"),
        (ResearchError::TimeOutOfRange, "time_out_of_range"),
        (ResearchError::OutOfRange, "out_of_range"),
        (ResearchError::EmptyId, "empty_id"),
        (ResearchError::EmptyInvalidation, "empty_invalidation"),
    ];
    for (error, code) in &errors {
        assert_eq!(error.code(), *code, "{error:?}");
    }
    assert_eq!(
        ResearchError::Unimplemented("admit", "E17-3").code(),
        "unimplemented",
        "the stub variant the implementation PR removes"
    );
    let unique: BTreeSet<&str> = errors.iter().map(|(e, _)| e.code()).collect();
    assert_eq!(unique.len(), errors.len(), "no two errors share a code");
}

/// `AdmissionDecision`'s three readers, over both variants: a caller must not be able to read
/// "admitted" from a refusal, a reason from an admission, or `ignored` from a check past the third.
#[test]
fn an_admission_decision_reads_the_same_way_for_both_variants() {
    let admitted = AdmissionDecision::Admitted {
        change: AdmissionChange::Admitted,
    };
    let renewed = AdmissionDecision::Admitted {
        change: AdmissionChange::Renewed,
    };
    for decision in [admitted, renewed] {
        assert!(decision.admitted(), "an admission reads as admitted");
        assert_eq!(decision.reason(), None, "and carries no refusal reason");
        assert!(!decision.ignored(), "and is not an ignored output");
    }

    for reason in RefusalReason::all() {
        let refused = AdmissionDecision::Refused { reason };
        assert!(!refused.admitted(), "{} is a refusal", reason.code());
        assert_eq!(
            refused.reason(),
            Some(reason),
            "which carries its own reason"
        );
        assert_eq!(
            refused.ignored(),
            reason.check_number() <= 3,
            "and is an ignored output exactly for checks 1 to 3: {}",
            reason.code()
        );
    }
}

/// `LineageState`'s five readers, including `held_by`, whose `==` a mutant can flip: an instrument is
/// found under the lineage that holds it and no other.
#[test]
fn lineage_state_reads_back_what_it_was_built_from() {
    let held = asset(INSTRUMENT_5);
    let other = asset(INSTRUMENT_2);
    let holder = LineageId::new("th-70").expect("a fixture lineage id is not empty");
    let bystander = LineageId::new("th-80").expect("a fixture lineage id is not empty");
    let state = LineageState::from_parts(
        [(
            holder.clone(),
            Lineage {
                revisions: 2,
                admitted: 3,
                retired: true,
            },
        )]
        .into_iter()
        .collect(),
        [(holder.clone(), held.clone())].into_iter().collect(),
    );

    let lineage = state
        .lineage(&holder)
        .expect("the lineage it was built with");
    assert_eq!(
        (lineage.revisions, lineage.admitted, lineage.retired),
        (2, 3, true),
        "every field reads back"
    );
    assert_eq!(
        state.lineage(&bystander),
        None,
        "and an unknown lineage is absent"
    );
    assert_eq!(
        state.holder_of(&holder),
        Some(&held),
        "the holder map reads back"
    );
    assert_eq!(state.holder_of(&bystander), None);
    assert_eq!(
        state.held_by(&held),
        Some(&holder),
        "and the reverse lookup finds the lineage that holds it"
    );
    assert_eq!(
        state.held_by(&other),
        None,
        "an instrument no lineage holds is held by none — the `==` this reverse lookup turns on"
    );
    assert_eq!(state.lineages().len(), 1, "the maps themselves read back");
    assert_eq!(state.holders().len(), 1);

    let empty = LineageState::default();
    assert!(
        empty.lineages().is_empty() && empty.holders().is_empty(),
        "and an empty state holds nothing, so a fold starts from nothing"
    );
    assert_eq!(empty.held_by(&held), None);
}

/// MI-18, unignored: a fold step never reports a carried-forward score, whatever else it carries.
///
/// The pending fold tests assert this too, but they cannot run until the fold exists, and a constant
/// that nothing unignored reads is a constant a mutant can flip unnoticed (round 2's finding 4).
/// `FoldStep`'s fields are public, so the assertion needs no fold at all — which is the point: **no
/// API in this crate can produce a step that carries a score**, so the claim holds by construction and
/// this test says so for every kind of step.
#[test]
fn no_fold_step_can_report_a_carried_forward_score() {
    let admitted = FoldStep {
        thesis_id: ThesisId::new("th-1").expect("a fixture thesis id is not empty"),
        decision: AdmissionDecision::Admitted {
            change: AdmissionChange::Admitted,
        },
        lineage_revisions: 0,
        lineage_retired: false,
        journal: Vec::new(),
    };
    let retired = FoldStep {
        thesis_id: ThesisId::new("th-2").expect("a fixture thesis id is not empty"),
        decision: AdmissionDecision::Refused {
            reason: RefusalReason::LineageRetired,
        },
        lineage_revisions: 3,
        lineage_retired: true,
        journal: Vec::new(),
    };

    for step in [&admitted, &retired] {
        assert!(
            !step.score_carried_forward(),
            "DEC-111 item 2: a revision starts with no track record, and there is no way to give it one"
        );
    }
}
