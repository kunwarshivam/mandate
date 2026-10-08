//! Reading `schemas/policy.schema.json` documents and the `policy_set` object that holds them
//! (journal spec §9, DEC-484 item 4; E7-19 slice 4 remainder, the brief's slice D4). A registered
//! policy set is validated before its levels fold into §4.3's overlay, so nothing outside the
//! schema reaches `check`.

use mandate_canon::Value;
use mandate_spec::policy::{LevelName, PolicyKey, PolicyLevel, PolicyValue, parse_policy_set};
use mandate_spec::{DecGrammar, ParseError, Pointer, SchemaDec, SpecError};

fn json(text: &str) -> Value {
    mandate_canon::parse(text.as_bytes()).unwrap()
}

fn dec(text: &str, grammar: DecGrammar) -> PolicyValue {
    PolicyValue::Decimal(SchemaDec::parse(text, grammar).unwrap())
}

fn set(items: &[&str]) -> PolicyValue {
    PolicyValue::Set(items.iter().map(|item| (*item).to_owned()).collect())
}

/// Every key the schema lists: its name, a value in its schema as JSON text, and what it reads as.
fn every_key() -> Vec<(PolicyKey, &'static str, PolicyValue)> {
    use DecGrammar::{Fraction, OpenFraction, PositiveDecimal, UnitPositive};
    use PolicyKey as K;
    use PolicyValue::{Flag, Integer};
    vec![
        (K::AdmissionAutoAllowed, "false", Flag(false)),
        (K::AllocationUsd, r#""5000""#, dec("5000", PositiveDecimal)),
        (K::ApprovalTimeoutS, "30", Integer(30)),
        (K::AssetClasses, r#"["us_equity"]"#, set(&["us_equity"])),
        (K::AutoAllowed, "true", Flag(true)),
        (K::BreachConfirmS, "300", Integer(300)),
        (K::CadenceIntervalS, "60", Integer(60)),
        (
            K::Channels,
            r#"["email","web_push"]"#,
            set(&["email", "web_push"]),
        ),
        (K::DailyBreachMinS, "0", Integer(0)),
        (K::EntryThreshold, r#""1""#, dec("1", UnitPositive)),
        (K::Environments, r#"["paper"]"#, set(&["paper"])),
        (K::ExitThreshold, r#""0.25""#, dec("0.25", UnitPositive)),
        (K::ExitsOnlyAtMax, r#""0.3""#, dec("0.3", OpenFraction)),
        (K::GoalTypes, r#"["accumulate"]"#, set(&["accumulate"])),
        (K::Hysteresis, r#""0.05""#, dec("0.05", OpenFraction)),
        (K::IndependentApprovalRequired, "true", Flag(true)),
        (K::LeveragedEtpsAllowed, "false", Flag(false)),
        (K::MaxDailyLoss, r#""0.02""#, dec("0.02", OpenFraction)),
        (K::MaxDrawdown, r#""0.1""#, dec("0.1", OpenFraction)),
        (
            K::MaxGrossExposureUsd,
            r#""2500.5""#,
            dec("2500.5", PositiveDecimal),
        ),
        (K::MaxInstruments, "20", Integer(20)),
        (
            K::MaxLossFromAllocation,
            r#""0.5""#,
            dec("0.5", OpenFraction),
        ),
        (K::MaxOrderUsd, r#""0.01""#, dec("0.01", PositiveDecimal)),
        (K::MaxOrdersPerDay, "10000", Integer(10000)),
        (K::MaxOutputAgeS, "86400", Integer(86400)),
        (K::MaxPositionFraction, r#""0.2""#, dec("0.2", UnitPositive)),
        (K::MaxPositionUsd, r#""1000""#, dec("1000", PositiveDecimal)),
        (K::MaxRevisionsPerLineage, "0", Integer(0)),
        (K::ProtectionRequired, "true", Flag(true)),
        (K::RebalanceBand, r#""0""#, dec("0", Fraction)),
        (K::ReentryCooldownS, "604800", Integer(604800)),
        (K::ResearchAgentAllowed, "false", Flag(false)),
        (
            K::ResearchCostCapUsdPerDay,
            r#""3""#,
            dec("3", PositiveDecimal),
        ),
        (K::ResearchIntervalS, "300", Integer(300)),
        (K::ResearchWeight, r#""0.5""#, dec("0.5", UnitPositive)),
        (K::ScaleLiftAfterS, "3600", Integer(3600)),
        (K::SignalModelTypes, r#"["quant"]"#, set(&["quant"])),
        (K::StaggerWindowS, "3600", Integer(3600)),
        (K::StopDistanceMax, r#""0.08""#, dec("0.08", OpenFraction)),
        (
            K::TwoApproverAboveUsd,
            r#""250""#,
            dec("250", PositiveDecimal),
        ),
    ]
}

/// A workspace document holding `values`, given as the members' JSON text.
fn document(values: &str) -> String {
    format!(
        r#"{{"level":"workspace","policy_schema_version":1,"profile":null,"values":{{{values}}}}}"#
    )
}

fn refused(document: &str) -> Option<SpecError> {
    PolicyLevel::parse(&json(document)).err()
}

fn at(path: &str) -> Pointer {
    Pointer::new(path)
}

/// Every key the schema lists reads as its value, in one document, and the level is named; every
/// `PolicyKey` is among them.
#[test]
#[ignore = "pending E7-19"]
fn every_key_the_schema_lists_reads_as_its_value() {
    let rows = every_key();
    let members: Vec<String> = rows
        .iter()
        .map(|(key, text, _)| format!(r#""{}":{text}"#, key.as_str()))
        .collect();
    let level = PolicyLevel::parse(&json(&document(&members.join(",")))).unwrap();
    assert_eq!(level.name, LevelName::Workspace);
    let expected = rows
        .into_iter()
        .map(|(key, _, value)| (key, value))
        .collect();
    assert_eq!(level.values, expected);
    assert_eq!(level.values.len(), 40, "every PolicyKey");
    let empty = PolicyLevel::parse(&json(&document(""))).unwrap();
    assert!(empty.values.is_empty(), "every key is optional");
}

/// The document's own members: `policy_schema_version` is 1, `level` one of the three ancestors,
/// `profile` one of two or `null`, and nothing else.
#[test]
#[ignore = "pending E7-19"]
fn the_documents_own_members_are_exactly_the_schemas() {
    let base = document("");
    let rows = [
        (
            base.replace(
                r#""policy_schema_version":1"#,
                r#""policy_schema_version":2"#,
            ),
            ParseError::NotInEnum {
                path: at("/policy_schema_version"),
            },
        ),
        (
            base.replace(r#""workspace""#, r#""mandate""#),
            ParseError::NotInEnum { path: at("/level") },
        ),
        (
            base.replace("null", r#""vip""#),
            ParseError::NotInEnum {
                path: at("/profile"),
            },
        ),
        (
            base.replace(r#""profile":null,"#, r#""owner":"x","profile":null,"#),
            ParseError::UnknownMember { path: at("/owner") },
        ),
        (
            base.replace(r#","values":{}"#, ""),
            ParseError::MissingMember {
                path: at("/values"),
            },
        ),
        (
            base.replace(r#""level":"workspace","#, ""),
            ParseError::MissingMember { path: at("/level") },
        ),
    ];
    for (text, error) in rows {
        assert_eq!(refused(&text), Some(SpecError::Parse(error)), "{text}");
    }
    let without_profile = base.replace(r#""profile":null,"#, "");
    assert!(
        PolicyLevel::parse(&json(&without_profile)).is_ok(),
        "profile is optional"
    );
}

/// Each value outside its schema is refused at its pointer: an unknown key, a decimal as a number
/// or off its grammar, an integer as a string or out of bounds, a flag of another type, a set item
/// off its enum, repeated, or a set that is not an array.
#[test]
#[ignore = "pending E7-19"]
fn a_value_outside_its_schema_is_refused_at_its_pointer() {
    let path = |key: &str| at(&format!("/values/{key}"));
    let grammar = |key: &str, grammar| ParseError::OffGrammar {
        path: path(key),
        grammar,
    };
    let rows = [
        (
            r#""max_loss":"0.1""#,
            ParseError::UnknownMember {
                path: path("max_loss"),
            },
        ),
        (
            r#""max_order_usd":5"#,
            ParseError::DecimalAsNumber {
                path: path("max_order_usd"),
            },
        ),
        (
            r#""max_order_usd":"0""#,
            grammar("max_order_usd", DecGrammar::PositiveDecimal),
        ),
        (
            r#""max_drawdown":"1""#,
            grammar("max_drawdown", DecGrammar::OpenFraction),
        ),
        (
            r#""entry_threshold":"1.5""#,
            grammar("entry_threshold", DecGrammar::UnitPositive),
        ),
        (
            r#""rebalance_band":"1.0""#,
            grammar("rebalance_band", DecGrammar::Fraction),
        ),
        (
            r#""max_instruments":"5""#,
            ParseError::WrongType {
                path: path("max_instruments"),
            },
        ),
        (
            r#""max_instruments":21"#,
            ParseError::OutOfBounds {
                path: path("max_instruments"),
            },
        ),
        (
            r#""approval_timeout_s":29"#,
            ParseError::OutOfBounds {
                path: path("approval_timeout_s"),
            },
        ),
        (
            r#""breach_confirm_s":301"#,
            ParseError::OutOfBounds {
                path: path("breach_confirm_s"),
            },
        ),
        (
            r#""auto_allowed":"true""#,
            ParseError::WrongType {
                path: path("auto_allowed"),
            },
        ),
        (
            r#""environments":["sandbox"]"#,
            ParseError::NotInEnum {
                path: at("/values/environments/0"),
            },
        ),
        (
            r#""channels":["sms","sms"]"#,
            ParseError::NotUnique {
                path: at("/values/channels/1"),
            },
        ),
        (
            r#""asset_classes":"us_equity""#,
            ParseError::WrongType {
                path: path("asset_classes"),
            },
        ),
    ];
    for (member, error) in rows {
        let text = document(member);
        assert_eq!(refused(&text), Some(SpecError::Parse(error)), "{text}");
    }
}

/// A `policy_set` object is exactly its three members; its levels read outermost first, in the
/// platform, organization, workspace order with at most one of each, and an empty set holds none.
#[test]
#[ignore = "pending E7-19"]
fn a_policy_set_reads_its_levels_in_order() {
    let level = |name: &str, values: &str| {
        format!(r#"{{"level":"{name}","policy_schema_version":1,"values":{{{values}}}}}"#)
    };
    let platform = level("platform", r#""max_instruments":20"#);
    let workspace = level("workspace", r#""max_instruments":5"#);
    let object = |levels: &[&str]| {
        format!(
            r#"{{"kind":"policy_set","levels":[{}],"policy_set_version":1}}"#,
            levels.join(",")
        )
    };
    let levels = parse_policy_set(&json(&object(&[&platform, &workspace]))).unwrap();
    let names: Vec<LevelName> = levels.iter().map(|level| level.name).collect();
    assert_eq!(names, [LevelName::Platform, LevelName::Workspace]);
    let max = levels[1].values.get(&PolicyKey::MaxInstruments);
    assert_eq!(max, Some(&PolicyValue::Integer(5)));
    assert_eq!(
        parse_policy_set(&json(&object(&[]))).map(|l| l.len()),
        Ok(0)
    );
    let reversed = parse_policy_set(&json(&object(&[&workspace, &platform]))).err();
    let twice = parse_policy_set(&json(&object(&[&platform, &platform]))).err();
    for refusal in [reversed, twice] {
        assert_eq!(refusal.map(|error| error.code()), Some("invalid_input"));
    }
}

/// The object's own members, and a level that breaks the schema, refuse with the pointer from the
/// object's root.
#[test]
#[ignore = "pending E7-19"]
fn a_policy_set_outside_its_shape_is_refused() {
    let good = r#"{"kind":"policy_set","levels":[],"policy_set_version":1}"#;
    let bad_level =
        r#"[{"level":"platform","policy_schema_version":1,"values":{"max_instruments":0}}]"#;
    let rows = [
        (
            good.replace(r#""policy_set""#, r#""model_registry""#),
            ParseError::NotInEnum { path: at("/kind") },
        ),
        (
            good.replace(r#""policy_set_version":1"#, r#""policy_set_version":2"#),
            ParseError::NotInEnum {
                path: at("/policy_set_version"),
            },
        ),
        (
            good.replace("[]", "{}"),
            ParseError::WrongType {
                path: at("/levels"),
            },
        ),
        (
            good.replace(r#""levels":[],"#, ""),
            ParseError::MissingMember {
                path: at("/levels"),
            },
        ),
        (
            good.replace(r#""kind""#, r#""extra":1,"kind""#),
            ParseError::UnknownMember { path: at("/extra") },
        ),
        (
            good.replace("[]", bad_level),
            ParseError::OutOfBounds {
                path: at("/levels/0/values/max_instruments"),
            },
        ),
    ];
    for (text, error) in rows {
        let refusal = parse_policy_set(&json(&text)).err();
        assert_eq!(refusal, Some(SpecError::Parse(error)), "{text}");
    }
}
