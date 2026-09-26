//! The small total functions this PR implements rather than stubs, and their invariants.
//!
//! These are **live**. They exist because the diff mutation gate skips a crate while it carries
//! pending tests (DEC-83), so anything real that lands in a tests PR would otherwise reach `main`
//! with no gate having looked at it. Each function below is a total map over this crate's own enums,
//! and each has a test here that a mutant would fail.

use std::collections::{BTreeMap, BTreeSet};

use mandate_domain::AgentMode;
use mandate_spec::condition::{
    Condition, ConditionField, ConditionValue, FieldKind, MAX_CONDITION_DEPTH, Operator,
};
use mandate_spec::document::{OnComplete, Pointer, Provenance, ProvenanceMap, Source};
use mandate_spec::policy::{KeyKind, PolicyKey};
use mandate_spec::risk::{LimitKey, Restriction};
use mandate_spec::{DecGrammar, SchemaDec};

#[test]
fn a_path_provenance_does_not_mention_is_entered_and_confirmed() {
    let map = ProvenanceMap::new(BTreeMap::from([(
        Pointer::new("/risk/max_drawdown"),
        Provenance {
            source: Source::PlatformProposed,
            confirmed: false,
        },
    )]));
    let stated = map.at(&Pointer::new("/risk/max_drawdown"));
    assert_eq!(stated.source, Source::PlatformProposed);
    assert!(!stated.confirmed);
    let unmentioned = map.at(&Pointer::new("/capital/allocation_usd"));
    assert_eq!(
        unmentioned.source,
        Source::UserEntered,
        "the fixture's rule: an absent path is user_entered and confirmed"
    );
    assert!(unmentioned.confirmed);
}

#[test]
fn only_the_three_owner_sources_satisfy_v020() {
    for source in [
        Source::UserStated,
        Source::UserEntered,
        Source::PlatformProposed,
    ] {
        assert!(
            source.is_owner_sourced(),
            "{source:?} is one of the three V-020 accepts"
        );
    }
    for source in [Source::TemplateStructure, Source::PlatformDefault] {
        assert!(
            !source.is_owner_sourced(),
            "{source:?} is not the owner's, so V-020 needs the §7 list for it"
        );
    }
    let names: BTreeSet<&str> = [
        Source::UserStated,
        Source::UserEntered,
        Source::TemplateStructure,
        Source::PlatformProposed,
        Source::PlatformDefault,
    ]
    .iter()
    .map(|s| s.as_str())
    .collect();
    assert_eq!(names.len(), 5, "§2.1 names five sources, each once");
}

#[test]
fn a_restrictions_mode_is_the_one_its_section_gives_it() {
    for (restriction, expected) in [
        (Restriction::DailyLoss, AgentMode::ExitsOnly),
        (Restriction::DrawdownExitsOnly, AgentMode::ExitsOnly),
        (Restriction::HardBreach, AgentMode::ExitsOnly),
        (Restriction::GoalComplete, AgentMode::ExitsOnly),
        (Restriction::DrawdownFlatten, AgentMode::Paused),
        (Restriction::LifetimeFloor, AgentMode::Paused),
        (Restriction::Retired, AgentMode::Stopped),
    ] {
        assert_eq!(
            restriction.mode(),
            expected,
            "`{}` asks for {expected:?}",
            restriction.as_str()
        );
    }
}

#[test]
fn the_effective_mode_is_the_strictest_of_the_active_restrictions() {
    let active = [
        Restriction::GoalComplete,
        Restriction::LifetimeFloor,
        Restriction::DailyLoss,
    ];
    assert_eq!(
        active.iter().map(|r| r.mode()).max(),
        Some(AgentMode::Paused),
        "MI-6: a maximum over the severity order, not the last restriction applied"
    );
    let none: [Restriction; 0] = [];
    assert_eq!(
        none.iter()
            .map(|r| r.mode())
            .max()
            .unwrap_or(AgentMode::Normal),
        AgentMode::Normal,
        "and nothing active is `normal`"
    );
}

#[test]
fn a_limit_journals_under_the_name_section_five_ten_gives_it() {
    assert_eq!(LimitKey::MaxDailyLoss.journal_name(), "max_daily_loss");
    assert_eq!(LimitKey::LifetimeFloor.journal_name(), "lifetime_floor");
    assert_eq!(
        LimitKey::DrawdownRung(0).journal_name(),
        "drawdown_ladder[0]",
        "the reference cases compare this string, so the brackets and the index are load-bearing"
    );
    assert_eq!(
        LimitKey::DrawdownRung(2).journal_name(),
        "drawdown_ladder[2]"
    );
    let names: BTreeSet<String> = [
        LimitKey::MaxDailyLoss,
        LimitKey::DrawdownRung(0),
        LimitKey::DrawdownRung(1),
        LimitKey::LifetimeFloor,
        LimitKey::ProfitStop,
    ]
    .iter()
    .map(|k| k.journal_name())
    .collect();
    assert_eq!(names.len(), 5, "no two limits journal under one name");
}

#[test]
fn a_conditions_comparisons_carry_their_nesting_depth() {
    let leaf = || Condition::Compare {
        field: ConditionField::OrderUsd,
        op: Operator::Gt,
        value: ConditionValue::Decimal(
            SchemaDec::parse("900", DecGrammar::PositiveDecimal).expect("a decimal"),
        ),
    };
    assert_eq!(
        leaf().comparisons().len(),
        1,
        "a bare comparison is one comparison"
    );
    assert_eq!(
        leaf().comparisons().first().map(|(_, depth)| *depth),
        Some(1),
        "the outermost condition is depth 1, so V-017's limit of four counts from there"
    );
    let nested = Condition::All(vec![leaf(), Condition::Not(Box::new(leaf()))]);
    let depths: Vec<u8> = nested.comparisons().iter().map(|(_, d)| *d).collect();
    assert_eq!(depths, vec![2, 3], "one level per combinator");
    let wide = Condition::Any(vec![leaf(), leaf(), leaf()]);
    assert_eq!(
        wide.comparisons().len(),
        3,
        "every comparison is collected, not just the first"
    );
    assert_eq!(MAX_CONDITION_DEPTH, 4, "V-017");
}

#[test]
fn every_condition_field_has_one_kind_and_the_reserved_one_is_named() {
    let rows = [
        (ConditionField::Purpose, FieldKind::Enum),
        (ConditionField::AssetClass, FieldKind::Enum),
        (ConditionField::Session, FieldKind::Enum),
        (ConditionField::Instrument, FieldKind::Text),
        (ConditionField::OrderUsd, FieldKind::Decimal),
        (ConditionField::CombinedScore, FieldKind::Decimal),
        (ConditionField::ThesisConfidence, FieldKind::Decimal),
        (ConditionField::FirstTradeInInstrument, FieldKind::Bool),
        (ConditionField::NewInstrument, FieldKind::Bool),
        (ConditionField::UnusualInput, FieldKind::Bool),
    ];
    for (field, kind) in rows {
        assert_eq!(field.kind(), kind, "`{}`", field.as_str());
    }
    assert!(
        ConditionField::UnusualInput.is_reserved(),
        "V-018: no rule may use it until the drift detector ships"
    );
    for field in [
        ConditionField::Purpose,
        ConditionField::OrderUsd,
        ConditionField::NewInstrument,
    ] {
        assert!(!field.is_reserved());
    }
    for field in [ConditionField::CombinedScore, ConditionField::Drawdown] {
        assert!(
            field.is_unit_bounded(),
            "§6.3 bounds `{}` to [0, 1], which V-023 checks on the value too",
            field.as_str()
        );
    }
    assert!(!ConditionField::OrderUsd.is_unit_bounded());
}

#[test]
fn operators_split_into_the_groups_v023_needs() {
    for op in [Operator::In, Operator::NotIn] {
        assert!(op.takes_list(), "`{}` takes an array", op.as_str());
        assert!(!op.is_ordering());
    }
    for op in [Operator::Gt, Operator::Gte, Operator::Lt, Operator::Lte] {
        assert!(op.is_ordering(), "`{}` orders its operands", op.as_str());
        assert!(!op.takes_list());
    }
    for op in [Operator::Eq, Operator::Ne] {
        assert!(!op.is_ordering() && !op.takes_list());
    }
    let names: BTreeSet<&str> = [
        Operator::Eq,
        Operator::Ne,
        Operator::Gt,
        Operator::Gte,
        Operator::Lt,
        Operator::Lte,
        Operator::In,
        Operator::NotIn,
    ]
    .iter()
    .map(|o| o.as_str())
    .collect();
    assert_eq!(names.len(), 8, "§6.3 names eight operators");
}

#[test]
fn the_two_keys_where_absence_is_the_violation_are_the_only_two() {
    let all = [
        PolicyKey::AllocationUsd,
        PolicyKey::MaxLossFromAllocation,
        PolicyKey::MaxPositionUsd,
        PolicyKey::MaxOrderUsd,
        PolicyKey::TwoApproverAboveUsd,
        PolicyKey::StopDistanceMax,
        PolicyKey::MaxInstruments,
        PolicyKey::EntryThreshold,
        PolicyKey::AutoAllowed,
        PolicyKey::ProtectionRequired,
        PolicyKey::AssetClasses,
    ];
    let violating: Vec<&str> = all
        .iter()
        .filter(|k| k.absence_violates())
        .map(|k| k.as_str())
        .collect();
    assert_eq!(
        violating,
        vec!["two_approver_above_usd", "stop_distance_max"],
        "§4.3: only where \"no limit\" is looser than any limit"
    );
    assert_eq!(PolicyKey::MaxInstruments.kind(), KeyKind::Maximum);
    assert_eq!(PolicyKey::StaggerWindowS.kind(), KeyKind::Minimum);
}

#[test]
fn a_profit_stop_has_no_on_complete_to_choose() {
    use mandate_spec::document::Goal;
    let continuous = Goal::Continuous {
        end_date: None,
        on_complete: OnComplete::HoldProtected,
    };
    assert_eq!(continuous.on_complete(), Some(OnComplete::HoldProtected));
    assert_eq!(continuous.type_name(), "continuous");
    assert_eq!(continuous.end_date(), None);
    let profit_stop = Goal::ProfitStop {
        profit_level: SchemaDec::parse("0.2", DecGrammar::PositiveDecimal).expect("a level"),
        end_date: None,
    };
    assert_eq!(
        profit_stop.on_complete(),
        None,
        "§3.1 gives a profit_stop one outcome, so there is nothing for the owner to choose"
    );
    assert_eq!(profit_stop.type_name(), "profit_stop");
    let names: BTreeSet<&str> = [
        OnComplete::HoldProtected,
        OnComplete::DisarmLadder,
        OnComplete::Release,
    ]
    .iter()
    .map(|c| c.as_str())
    .collect();
    assert_eq!(names.len(), 3);
}

/// ES-09's stable codes, asserted per variant. A report or a log line carries these, so an empty or
/// wrong code is a defect a reader cannot see and a script cannot key on.
#[test]
fn every_error_variant_has_its_own_stable_code() {
    use mandate_num::NumError;
    use mandate_spec::{ParseError, SpecError};
    let path = || Pointer::new("/risk/max_drawdown");
    let parse_errors = [
        (ParseError::UnknownMember { path: path() }, "unknown_member"),
        (ParseError::MissingMember { path: path() }, "missing_member"),
        (
            ParseError::DecimalAsNumber { path: path() },
            "decimal_as_number",
        ),
        (ParseError::WrongType { path: path() }, "wrong_type"),
        (ParseError::NotInEnum { path: path() }, "not_in_enum"),
        (ParseError::OffPattern { path: path() }, "off_pattern"),
        (ParseError::OutOfBounds { path: path() }, "out_of_bounds"),
        (
            ParseError::OffGrammar {
                path: path(),
                grammar: DecGrammar::Fraction,
            },
            "off_grammar",
        ),
        (ParseError::TooDeep { path: path() }, "too_deep"),
        (ParseError::Unimplemented, "unimplemented"),
    ];
    for (error, code) in &parse_errors {
        assert_eq!(error.code(), *code, "{error:?}");
    }
    let codes: BTreeSet<&str> = parse_errors.iter().map(|(e, _)| e.code()).collect();
    assert_eq!(codes.len(), parse_errors.len(), "one code per variant");

    let spec_errors = [
        (
            SpecError::OutOfRange {
                path: path(),
                cause: NumError::TooPrecise,
            },
            "out_of_range",
        ),
        (SpecError::ClockWentBackwards, "clock_went_backwards"),
        (SpecError::UnknownRestriction, "unknown_restriction"),
        (SpecError::Unimplemented, "unimplemented"),
    ];
    for (error, code) in &spec_errors {
        assert_eq!(error.code(), *code, "{error:?}");
    }
    assert_eq!(
        SpecError::from(ParseError::Unimplemented).code(),
        "unimplemented",
        "a wrapped error keeps the inner code, so a caller sees one vocabulary"
    );
    assert_eq!(
        SpecError::from(NumError::TooPrecise).code(),
        NumError::TooPrecise.code(),
        "and the numeric error's own code, not a new one"
    );
}

/// Every text form a journal payload, a report, or a policy violation carries.
#[test]
fn every_enum_writes_the_text_its_section_gives_it() {
    use mandate_spec::change::ChangeClass;
    use mandate_spec::document::{LadderAction, LimitAction};
    for (class, text) in [
        (ChangeClass::RiskIncreasing, "risk_increasing"),
        (ChangeClass::RiskReducing, "risk_reducing"),
        (ChangeClass::Neutral, "neutral"),
        (ChangeClass::Invalid, "invalid"),
    ] {
        assert_eq!(class.as_str(), text);
    }
    for (action, text) in [
        (LadderAction::ScaleSizes, "scale_sizes"),
        (LadderAction::ExitsOnly, "exits_only"),
        (LadderAction::FlattenAndPause, "flatten_and_pause"),
    ] {
        assert_eq!(action.as_str(), text);
    }
    for (action, text) in [
        (LimitAction::ExitsOnly, "exits_only"),
        (LimitAction::FlattenAndPause, "flatten_and_pause"),
    ] {
        assert_eq!(action.as_str(), text);
    }
    for (field, text) in [
        (ConditionField::Purpose, "purpose"),
        (ConditionField::OrderUsd, "order_usd"),
        (ConditionField::CombinedScore, "combined_score"),
        (ConditionField::NewInstrument, "new_instrument"),
        (ConditionField::ThesisConfidence, "thesis_confidence"),
        (ConditionField::UnusualInput, "unusual_input"),
        (ConditionField::PositionPnlFraction, "position_pnl_fraction"),
    ] {
        assert_eq!(field.as_str(), text, "§6.3 names it {text}");
    }
    for (restriction, text) in [
        (Restriction::DailyLoss, "daily_loss"),
        (Restriction::DrawdownExitsOnly, "drawdown_exits_only"),
        (Restriction::DrawdownFlatten, "drawdown_flatten"),
        (Restriction::LifetimeFloor, "lifetime_floor"),
        (Restriction::HardBreach, "hard_breach"),
        (Restriction::GoalComplete, "goal_complete"),
        (Restriction::Retired, "retired"),
    ] {
        assert_eq!(restriction.as_str(), text, "§5.9 names it {text}");
    }
}

/// A pointer is what every rejection, provenance entry, and policy violation names a field with.
#[test]
fn a_pointer_keeps_the_path_it_was_given() {
    let path = "/behavior/signal_models/0/max_output_age_s";
    let pointer = Pointer::new(path);
    assert_eq!(pointer.as_str(), path);
    assert_eq!(pointer.to_string(), path);
    assert_eq!(Pointer::new("/a"), Pointer::new("/a"));
    assert_ne!(Pointer::new("/a"), Pointer::new("/b"));
    assert!(
        Pointer::new("/a") < Pointer::new("/b"),
        "pointers order, so a report's findings come out in one order"
    );
}

/// An `end_date` is read back, not dropped: it is the last risk day of the goal (§3.1, §5.4).
#[test]
fn a_goals_end_date_is_the_one_it_was_given() {
    use mandate_spec::document::Goal;
    let date = mandate_time::Date::parse("2026-12-31").expect("a date");
    let goal = Goal::Continuous {
        end_date: Some(date),
        on_complete: OnComplete::Release,
    };
    assert_eq!(goal.end_date(), Some(&date));
    let profit_stop = Goal::ProfitStop {
        profit_level: SchemaDec::parse("0.2", DecGrammar::PositiveDecimal).expect("a level"),
        end_date: Some(date),
    };
    assert_eq!(
        profit_stop.end_date(),
        Some(&date),
        "every goal type carries one, which is why the accessor exists"
    );
}

/// The remaining text forms and accessors that are reachable without the parse.
///
/// What is *not* reachable is named in the PR body: an accessor on a type only the parse constructs
/// (`AgentName`, `ConnectionId`, `ModelId`, `RuleId`, `ApproverRef`), and anything inside or downstream
/// of a stub. Those mutants are collected by the implementation PR's gate, which touches the same
/// files; `dec.rs` is the one fully real module, and it was run through the gate by hand at zero.
#[test]
fn the_reachable_accessors_return_what_they_were_given() {
    use mandate_spec::policy::LevelName;
    use mandate_spec::risk::{Confirmation, InstrumentRestriction, Rejection};

    for (level, text) in [
        (LevelName::Platform, "platform"),
        (LevelName::Organization, "organization"),
        (LevelName::Workspace, "workspace"),
        (LevelName::Mandate, "mandate"),
    ] {
        assert_eq!(level.as_str(), text, "§4.3 names the levels");
    }
    assert_eq!(InstrumentRestriction::StaleMark.as_str(), "stale_mark");
    assert_eq!(
        InstrumentRestriction::RemovedInstrument.as_str(),
        "removed_instrument"
    );

    let rejections = [
        (
            Rejection::IncreaseBlockedWhileLatched,
            "increase_blocked_while_latched",
        ),
        (Rejection::EquityBelowExposure, "equity_below_exposure"),
        (Rejection::WouldTriggerLimit, "would_trigger_limit"),
        (Rejection::NotAcknowledgeable, "not_acknowledgeable"),
        (Rejection::NothingToAcknowledge, "nothing_to_acknowledge"),
        (Rejection::FlattenInProgress, "flatten_in_progress"),
        (Rejection::NotLoosening, "not_loosening"),
        (Rejection::WaitingPeriod, "waiting_period"),
        (Rejection::StillBelowNewFloor, "still_below_new_floor"),
    ];
    for (rejection, code) in &rejections {
        assert_eq!(rejection.code(), *code, "the cases compare this string");
    }
    let codes: BTreeSet<&str> = rejections.iter().map(|(r, _)| r.code()).collect();
    assert_eq!(codes.len(), rejections.len(), "one code per refusal");

    let fresh = Confirmation::default();
    assert!(
        !fresh.is_pending(),
        "nothing is confirming before any input has been credited"
    );
    assert_eq!(fresh.accumulated_s(), 0);

    let empty = ProvenanceMap::default();
    assert!(
        empty.entries().is_empty(),
        "an empty map really is empty, and every path then takes the default"
    );
    let map = ProvenanceMap::new(BTreeMap::from([(
        Pointer::new("/name"),
        Provenance {
            source: Source::PlatformDefault,
            confirmed: true,
        },
    )]));
    assert_eq!(map.entries().len(), 1);
    assert_eq!(
        map.entries().keys().next(),
        Some(&Pointer::new("/name")),
        "the entry that was put in is the entry that comes out"
    );
}
