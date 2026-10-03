//! The shared vocabulary. These tests are **live**, not pending: every function here is a total map
//! over the crate's own variants, small enough to be right in this PR, and a pending marker would be a
//! promise to test it later that the diff mutation gate would never collect (DEC-83's reasoning read
//! the other way round).

use std::collections::BTreeSet;

use mandate_domain::{
    AgentMode, AssetClass, AssetId, AutonomyDecision, DomainError, MarketSession, Purpose,
    ThesisRefusal, WorkingUniverse,
};

#[test]
fn asset_class_round_trips_through_the_schema_spelling() {
    for class in [AssetClass::Crypto, AssetClass::UsEquity] {
        assert_eq!(AssetClass::parse(class.as_str()), Ok(class));
    }
    assert_eq!(AssetClass::UsEquity.as_str(), "us_equity");
    assert_eq!(AssetClass::Crypto.as_str(), "crypto");
}

#[test]
fn the_vendor_spelling_is_not_an_asset_class() {
    assert_eq!(
        AssetClass::parse("us-equity"),
        Err(DomainError::UnknownAssetClass),
        "the hyphen form is a market-data path segment, not the schema's name"
    );
    assert_eq!(
        AssetClass::parse("equity"),
        Err(DomainError::UnknownAssetClass)
    );
    assert_eq!(AssetClass::parse(""), Err(DomainError::UnknownAssetClass));
}

#[test]
fn the_session_condition_form_omits_overnight() {
    for session in [
        MarketSession::PreMarket,
        MarketSession::Regular,
        MarketSession::AfterHours,
        MarketSession::Crypto,
    ] {
        assert_eq!(
            MarketSession::parse_condition_form(session.as_str()),
            Ok(session)
        );
    }
    assert_eq!(
        MarketSession::parse_condition_form("overnight"),
        Err(DomainError::UnknownSession),
        "no order may trade overnight, so no rule may name it (DEC-30, spec §6.3)"
    );
}

#[test]
fn autonomy_decisions_order_by_strictness() {
    assert!(AutonomyDecision::Auto < AutonomyDecision::Ask);
    assert!(AutonomyDecision::Ask < AutonomyDecision::Deny);
    for (a, b, expected) in [
        (
            AutonomyDecision::Auto,
            AutonomyDecision::Ask,
            AutonomyDecision::Ask,
        ),
        (
            AutonomyDecision::Ask,
            AutonomyDecision::Auto,
            AutonomyDecision::Ask,
        ),
        (
            AutonomyDecision::Deny,
            AutonomyDecision::Auto,
            AutonomyDecision::Deny,
        ),
        (
            AutonomyDecision::Ask,
            AutonomyDecision::Ask,
            AutonomyDecision::Ask,
        ),
    ] {
        assert_eq!(a.stricter(b), expected, "the stricter of {a:?} and {b:?}");
    }
}

#[test]
fn a_ceiling_only_tightens_a_decision() {
    let all = [
        AutonomyDecision::Auto,
        AutonomyDecision::Ask,
        AutonomyDecision::Deny,
    ];
    for decision in all {
        for ceiling in all {
            let result = decision.stricter(ceiling);
            assert!(
                result >= decision,
                "§6.2 step 5's ceiling may only tighten: {decision:?} under {ceiling:?} gave {result:?}"
            );
            assert!(
                result >= ceiling,
                "and the result is at least as strict as the ceiling itself"
            );
        }
    }
}

#[test]
fn agent_modes_order_by_severity() {
    assert!(AgentMode::Normal < AgentMode::ExitsOnly);
    assert!(AgentMode::ExitsOnly < AgentMode::Paused);
    assert!(AgentMode::Paused < AgentMode::Stopped);
}

#[test]
fn the_effective_mode_is_the_strictest_restriction() {
    let active = [AgentMode::Normal, AgentMode::Paused, AgentMode::ExitsOnly];
    assert_eq!(
        active.into_iter().max(),
        Some(AgentMode::Paused),
        "MI-6: the effective mode is a maximum over the severity order, not the last one applied"
    );
}

#[test]
fn only_opening_and_increasing_purposes_can_be_denied() {
    for purpose in [
        Purpose::DiscretionaryExit,
        Purpose::OwnerExit,
        Purpose::RiskExit,
        Purpose::Protective,
    ] {
        assert!(
            purpose.reduces_risk(),
            "MI-1: {purpose:?} reduces risk and is built-in AUTO (§6.2 step 3)"
        );
    }
    assert!(!Purpose::Open.reduces_risk());
    assert!(!Purpose::Increase.reduces_risk());
}

#[test]
fn an_unavailable_universe_is_not_an_empty_one() {
    let empty = WorkingUniverse::Known {
        instruments: BTreeSet::new(),
        pinned: false,
    };
    assert_ne!(
        empty,
        WorkingUniverse::Unavailable,
        "a gate that has not read the fold must be able to say so, which an empty set cannot"
    );
}

#[test]
fn every_error_has_a_distinct_stable_code() {
    let errors = [
        DomainError::UnknownAssetClass,
        DomainError::UnknownSession,
        DomainError::MalformedAssetId,
        DomainError::UnknownThesisRefusal,
        DomainError::Unimplemented,
    ];
    let codes: BTreeSet<&str> = errors.iter().map(|e| e.code()).collect();
    assert_eq!(codes.len(), errors.len(), "ES-09: one code per variant");
    assert!(codes.iter().all(|c| !c.is_empty()));
}

#[test]
fn every_display_writes_the_schema_spelling() {
    assert_eq!(AssetClass::UsEquity.to_string(), "us_equity");
    assert_eq!(AssetClass::Crypto.to_string(), "crypto");
    for session in [
        MarketSession::Overnight,
        MarketSession::PreMarket,
        MarketSession::Regular,
        MarketSession::AfterHours,
        MarketSession::Crypto,
    ] {
        assert_eq!(
            session.to_string(),
            session.as_str(),
            "a session's Display is its schema spelling, which a journal payload carries"
        );
    }
    let id = AssetId::parse("7b4a1c2e-1111-4a2b-9c3d-000000000001").expect("a uuid");
    assert_eq!(id.to_string(), "7b4a1c2e-1111-4a2b-9c3d-000000000001");
    assert_eq!(id.as_str(), "7b4a1c2e-1111-4a2b-9c3d-000000000001");
}

#[test]
fn an_asset_id_is_the_schemas_lowercase_uuid_and_nothing_else() {
    let good = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
    assert!(AssetId::parse(good).is_ok());
    for bad in [
        "",
        "7b4a1c2e",
        "7B4A1C2E-1111-4A2B-9C3D-000000000001",
        "7b4a1c2e-1111-4a2b-9c3d-00000000000",
        "7b4a1c2e-1111-4a2b-9c3d-0000000000012",
        "7b4a1c2e-1111-4a2b-9c3d-000000000001-",
        "7b4a1c2e-1111-4a2b-9c3d-00000000000g",
        "7b4a1c2e_1111_4a2b_9c3d_000000000001",
        "7b4a1c2e-1111-4a2b-9c3d000000000001-",
    ] {
        assert_eq!(
            AssetId::parse(bad),
            Err(DomainError::MalformedAssetId),
            "`{bad}` is not the schema's uuid form"
        );
    }
    assert!(
        AssetId::parse("7B4A1C2E-1111-4A2B-9C3D-000000000001").is_err(),
        "uppercase is rejected, not folded: two spellings of one id must never both be admitted"
    );
}

#[test]
fn asset_ids_order_and_deduplicate_as_text() {
    let a = AssetId::parse("7b4a1c2e-1111-4a2b-9c3d-000000000001").expect("a uuid");
    let b = AssetId::parse("7b4a1c2e-2222-4a2b-9c3d-000000000002").expect("a uuid");
    assert!(a < b);
    let universe = BTreeSet::from([a.clone(), b, a]);
    assert_eq!(
        universe.len(),
        2,
        "MI-15: a working universe holds no duplicate, which the set gives for free"
    );
}

/// Mandate spec §8.5's table, typed here from the spec rather than read from the type: each reason's
/// code in check order. A renamed, dropped, or reordered reason fails here, not silently in the
/// journal (ES-09, DEC-415).
const SPEC_8_5_REASONS: [&str; 17] = [
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
];

#[test]
fn the_thesis_refusals_are_the_spec_table_in_check_order() {
    let codes: Vec<&str> = ThesisRefusal::all().iter().map(|r| r.code()).collect();
    assert_eq!(
        codes, SPEC_8_5_REASONS,
        "`all` walks §8.5's table top to bottom"
    );
    assert_eq!(
        ThesisRefusal::CODES,
        SPEC_8_5_REASONS,
        "the closed set the journal types `reason` with is the same list in the same order"
    );
    let numbers: Vec<u8> = ThesisRefusal::all()
        .iter()
        .map(|r| r.check_number())
        .collect();
    assert_eq!(
        numbers,
        (1..=17).collect::<Vec<u8>>(),
        "a check is numbered from 1, as §8.5 numbers it"
    );
}

#[test]
fn each_thesis_refusal_names_its_own_check() {
    assert_eq!(ThesisRefusal::DirectionNotAllowed.check_number(), 1);
    assert_eq!(ThesisRefusal::ResearchDisabled.check_number(), 4);
    assert_eq!(ThesisRefusal::UniversePinned.check_number(), 5);
    assert_eq!(ThesisRefusal::AdmissionDenied.check_number(), 6);
    assert_eq!(ThesisRefusal::NotAllowedAssetClass.check_number(), 10);
    assert_eq!(ThesisRefusal::NoCorroboration.check_number(), 15);
    assert_eq!(ThesisRefusal::NoCorroboration.code(), "no_corroboration");
    assert_eq!(ThesisRefusal::LineageRetired.check_number(), 16);
    assert_eq!(ThesisRefusal::UniverseFull.check_number(), 17);
    assert_eq!(ThesisRefusal::UniverseFull.code(), "universe_full");
}

#[test]
fn a_thesis_refusal_round_trips_through_its_code() {
    for reason in ThesisRefusal::all() {
        assert_eq!(ThesisRefusal::parse(reason.code()), Ok(reason));
        assert_eq!(
            reason.to_string(),
            reason.code(),
            "a refusal's Display is the code a journal payload carries"
        );
    }
}

#[test]
fn only_the_exact_spelling_is_a_thesis_refusal() {
    for bad in [
        "",
        "Universe_full",
        "universe_full ",
        "universe-full",
        "UniverseFull",
        "unimplemented",
        "lineage_retired\n",
    ] {
        assert_eq!(
            ThesisRefusal::parse(bad),
            Err(DomainError::UnknownThesisRefusal),
            "`{bad}` is not one of §8.5's reasons"
        );
    }
    assert_eq!(
        DomainError::UnknownThesisRefusal.code(),
        "unknown_thesis_refusal"
    );
}

#[test]
fn the_ignored_outputs_are_exactly_checks_1_to_3() {
    for reason in ThesisRefusal::all() {
        assert_eq!(
            reason.is_ignored_output(),
            reason.check_number() <= 3,
            "{reason} is check {} (§8.2)",
            reason.check_number()
        );
    }
}

#[test]
fn the_first_failing_check_is_the_minimum() {
    let all = ThesisRefusal::all();
    assert!(
        all.windows(2).all(|w| matches!(w, [a, b] if a < b)),
        "`Ord` is the check order, strictly"
    );
    let failing = BTreeSet::from([
        ThesisRefusal::UniverseFull,
        ThesisRefusal::NoCorroboration,
        ThesisRefusal::UniversePinned,
    ]);
    assert_eq!(
        failing.first().copied(),
        Some(ThesisRefusal::UniversePinned),
        "§8.5: the first failure decides"
    );
}
