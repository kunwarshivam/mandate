//! The lock judgement against journal spec §6.2 (E21-40, DEC-973): one test per refusal the
//! backlog names (no lock, governance mode only, a retention shorter than required, a rule the
//! writer's credential can remove), the boundaries of six years, R2's bucket lock rules, and a
//! property test whose oracle derives the expected refusals from the weakenings it applied.

use std::collections::BTreeSet;

use mandate_objectstore::lock::{
    BucketLock, LockMode, ProbeEvidence, R2Condition, R2ParseError, R2Rule,
    REQUIRED_RETENTION_DAYS, REQUIRED_RETENTION_SECS, REQUIRED_RETENTION_YEARS, Refusal, Refused,
    RetentionPeriod, S3Evidence, judge_r2, judge_s3, parse_r2_rules,
};
use proptest::prelude::*;

const DAY_SECS: u64 = 86_400;

/// A store §6.2 accepts: versioned, locked, a six-year compliance default, and a probe the writer
/// could not touch.
fn strong() -> S3Evidence {
    S3Evidence {
        bucket: BucketLock {
            versioning_enabled: true,
            object_lock_enabled: true,
            default_rule: Some((LockMode::Compliance, RetentionPeriod::Years(6))),
        },
        probe: ProbeEvidence {
            mode: Some(LockMode::Compliance),
            retained_secs: Some(2192 * DAY_SECS),
            shorten_refused: true,
            delete_refused: true,
            rule_write_refused: true,
        },
    }
}

fn refused(evidence: &S3Evidence) -> BTreeSet<Refusal> {
    match judge_s3(evidence) {
        Ok(()) => BTreeSet::new(),
        Err(Refused(set)) => {
            assert!(!set.is_empty(), "a refusal names at least one reason");
            set
        }
    }
}

fn set(items: &[Refusal]) -> BTreeSet<Refusal> {
    items.iter().copied().collect()
}

#[test]
fn six_years_is_counted_with_both_leap_days() {
    assert_eq!(REQUIRED_RETENTION_DAYS, 6 * 365 + 2);
    assert_eq!(REQUIRED_RETENTION_SECS, 2192 * DAY_SECS);
    assert_eq!(REQUIRED_RETENTION_YEARS, 6);
}

#[test]
fn a_compliance_store_the_writer_cannot_touch_is_accepted() {
    assert_eq!(judge_s3(&strong()), Ok(()));
}

#[test]
fn a_bucket_without_object_lock_is_refused() {
    let mut evidence = strong();
    evidence.bucket.object_lock_enabled = false;
    evidence.bucket.default_rule = None;
    assert_eq!(refused(&evidence), set(&[Refusal::NoLock]));
}

#[test]
fn a_probe_without_retention_is_refused() {
    let mut evidence = strong();
    evidence.probe.mode = None;
    evidence.probe.retained_secs = None;
    assert_eq!(refused(&evidence), set(&[Refusal::NoLock]));
}

#[test]
fn governance_mode_only_is_refused() {
    let mut evidence = strong();
    evidence.bucket.default_rule = Some((LockMode::Governance, RetentionPeriod::Years(6)));
    evidence.probe.mode = Some(LockMode::Governance);
    assert_eq!(refused(&evidence), set(&[Refusal::GovernanceOnly]));
}

#[test]
fn a_governance_default_is_refused_even_when_the_probe_is_compliance() {
    let mut evidence = strong();
    evidence.bucket.default_rule = Some((LockMode::Governance, RetentionPeriod::Years(10)));
    assert_eq!(refused(&evidence), set(&[Refusal::GovernanceOnly]));
}

#[test]
fn a_retention_shorter_than_six_years_is_refused() {
    let mut evidence = strong();
    evidence.bucket.default_rule = Some((LockMode::Compliance, RetentionPeriod::Days(2191)));
    assert_eq!(refused(&evidence), set(&[Refusal::RetentionTooShort]));

    let mut evidence = strong();
    evidence.bucket.default_rule = Some((LockMode::Compliance, RetentionPeriod::Years(5)));
    assert_eq!(refused(&evidence), set(&[Refusal::RetentionTooShort]));

    let mut evidence = strong();
    evidence.probe.retained_secs = Some(REQUIRED_RETENTION_SECS - 1);
    assert_eq!(refused(&evidence), set(&[Refusal::RetentionTooShort]));
}

#[test]
fn exactly_six_years_is_enough() {
    let mut evidence = strong();
    evidence.bucket.default_rule = Some((LockMode::Compliance, RetentionPeriod::Days(2192)));
    evidence.probe.retained_secs = Some(REQUIRED_RETENTION_SECS);
    assert_eq!(judge_s3(&evidence), Ok(()));
}

#[test]
fn a_rule_removable_by_the_writers_credential_is_refused() {
    let mut evidence = strong();
    evidence.probe.rule_write_refused = false;
    assert_eq!(refused(&evidence), set(&[Refusal::RuleRemovableByWriter]));
}

#[test]
fn a_store_that_let_the_writer_shorten_or_delete_is_refused() {
    let mut evidence = strong();
    evidence.probe.shorten_refused = false;
    assert_eq!(refused(&evidence), set(&[Refusal::RetentionShortenable]));

    let mut evidence = strong();
    evidence.probe.delete_refused = false;
    assert_eq!(refused(&evidence), set(&[Refusal::DeleteAllowed]));
}

#[test]
fn versioning_off_or_no_default_rule_is_refused() {
    let mut evidence = strong();
    evidence.bucket.versioning_enabled = false;
    assert_eq!(refused(&evidence), set(&[Refusal::VersioningOff]));

    let mut evidence = strong();
    evidence.bucket.default_rule = None;
    assert_eq!(refused(&evidence), set(&[Refusal::NoDefaultRetention]));
}

#[test]
fn every_refusal_has_its_own_code() {
    let all = [
        Refusal::NoLock,
        Refusal::VersioningOff,
        Refusal::NoDefaultRetention,
        Refusal::GovernanceOnly,
        Refusal::RetentionTooShort,
        Refusal::RetentionShortenable,
        Refusal::DeleteAllowed,
        Refusal::RuleRemovableByWriter,
    ];
    let codes: BTreeSet<&str> = all.iter().map(|refusal| refusal.code()).collect();
    assert_eq!(codes.len(), all.len());
    assert!(codes.iter().all(|code| code.starts_with("objectstore_")));
    let message = Refused(set(&[Refusal::NoLock, Refusal::DeleteAllowed])).to_string();
    assert!(
        message.contains("objectstore_no_lock, objectstore_delete_allowed"),
        "{message}"
    );
}

/// One way to weaken [`strong`], and the refusal §6.2 says it must cause.
#[derive(Debug, Clone, Copy)]
enum Weakening {
    LockOff,
    VersioningOff,
    NoDefault,
    DefaultGovernance,
    DefaultShortDays(u32),
    DefaultShortYears(u32),
    ProbeUnlocked,
    ProbeGovernance,
    ProbeShort(u64),
    Shortenable,
    Deletable,
    RuleWritable,
}

fn weakening() -> impl Strategy<Value = Weakening> {
    prop_oneof![
        Just(Weakening::LockOff),
        Just(Weakening::VersioningOff),
        Just(Weakening::NoDefault),
        Just(Weakening::DefaultGovernance),
        (0..REQUIRED_RETENTION_DAYS).prop_map(Weakening::DefaultShortDays),
        (0..REQUIRED_RETENTION_YEARS).prop_map(Weakening::DefaultShortYears),
        Just(Weakening::ProbeUnlocked),
        Just(Weakening::ProbeGovernance),
        (0..REQUIRED_RETENTION_SECS).prop_map(Weakening::ProbeShort),
        Just(Weakening::Shortenable),
        Just(Weakening::Deletable),
        Just(Weakening::RuleWritable),
    ]
}

/// The independent oracle: applies the weakenings to [`strong`] and collects, by its own walk,
/// the refusal each must cause. A bucket without object lock has no default rule to judge, so its
/// rule weakenings add nothing once `LockOff` is among them.
fn weaken(weakenings: &[Weakening]) -> (S3Evidence, BTreeSet<Refusal>) {
    let mut evidence = strong();
    let mut expected = BTreeSet::new();
    let lock_off = weakenings.iter().any(|w| matches!(w, Weakening::LockOff));
    let mut default_mode = LockMode::Compliance;
    let mut default_period = RetentionPeriod::Years(6);
    let mut no_default = false;
    for weakening in weakenings {
        match *weakening {
            Weakening::LockOff => {
                evidence.bucket.object_lock_enabled = false;
                expected.insert(Refusal::NoLock);
            }
            Weakening::VersioningOff => {
                evidence.bucket.versioning_enabled = false;
                expected.insert(Refusal::VersioningOff);
            }
            Weakening::NoDefault => no_default = true,
            Weakening::DefaultGovernance => default_mode = LockMode::Governance,
            Weakening::DefaultShortDays(days) => default_period = RetentionPeriod::Days(days),
            Weakening::DefaultShortYears(years) => default_period = RetentionPeriod::Years(years),
            Weakening::ProbeUnlocked => {
                evidence.probe.mode = None;
                evidence.probe.retained_secs = None;
                expected.insert(Refusal::NoLock);
            }
            Weakening::ProbeGovernance => {
                if evidence.probe.mode.is_some() {
                    evidence.probe.mode = Some(LockMode::Governance);
                }
            }
            Weakening::ProbeShort(secs) => {
                if evidence.probe.retained_secs.is_some() {
                    evidence.probe.retained_secs = Some(secs);
                }
            }
            Weakening::Shortenable => {
                evidence.probe.shorten_refused = false;
                expected.insert(Refusal::RetentionShortenable);
            }
            Weakening::Deletable => {
                evidence.probe.delete_refused = false;
                expected.insert(Refusal::DeleteAllowed);
            }
            Weakening::RuleWritable => {
                evidence.probe.rule_write_refused = false;
                expected.insert(Refusal::RuleRemovableByWriter);
            }
        }
    }
    if evidence.probe.mode == Some(LockMode::Governance) {
        expected.insert(Refusal::GovernanceOnly);
    }
    if evidence
        .probe
        .retained_secs
        .is_some_and(|secs| secs < REQUIRED_RETENTION_SECS)
    {
        expected.insert(Refusal::RetentionTooShort);
    }
    if no_default {
        evidence.bucket.default_rule = None;
        if !lock_off {
            expected.insert(Refusal::NoDefaultRetention);
        }
    } else {
        evidence.bucket.default_rule = Some((default_mode, default_period));
        if !lock_off {
            if default_mode == LockMode::Governance {
                expected.insert(Refusal::GovernanceOnly);
            }
            let short = match default_period {
                RetentionPeriod::Days(days) => days < 2192,
                RetentionPeriod::Years(years) => years < 6,
            };
            if short {
                expected.insert(Refusal::RetentionTooShort);
            }
        }
    }
    (evidence, expected)
}

proptest! {
    #[test]
    fn every_weakening_is_refused_with_its_own_reason(
        weakenings in proptest::collection::vec(weakening(), 0..6),
    ) {
        let (evidence, expected) = weaken(&weakenings);
        prop_assert_eq!(refused(&evidence), expected);
    }
}

fn r2(body: &str) -> Vec<R2Rule> {
    match parse_r2_rules(body.as_bytes()) {
        Ok(rules) => rules,
        Err(error) => panic!("the fixture parses: {error}"),
    }
}

const R2_INDEFINITE: &str = r#"{"success":true,"errors":[],"messages":[],"result":{"rules":[
    {"id":"all","enabled":true,"condition":{"type":"Indefinite"}}]}}"#;

#[test]
fn r2_rules_parse_in_each_condition() {
    let rules = r2(
        r#"{"success":true,"errors":[],"messages":[],"result":{"rules":[
        {"id":"a","enabled":true,"prefix":"cold/","condition":{"type":"Age","maxAgeSeconds":604800}},
        {"id":"d","enabled":false,"prefix":"","condition":{"type":"Date","date":"2033-01-01T00:00:00Z"}},
        {"id":"i","enabled":true,"condition":{"type":"Indefinite"}}]}}"#,
    );
    assert_eq!(rules.len(), 3);
    assert_eq!(
        rules.first().map(|rule| &rule.condition),
        Some(&R2Condition::Age {
            max_age_seconds: 604_800
        })
    );
    assert_eq!(
        rules.first().and_then(|rule| rule.prefix.as_deref()),
        Some("cold/")
    );
    assert_eq!(
        rules
            .get(1)
            .map(|rule| (&rule.id[..], rule.enabled, &rule.condition)),
        Some((
            "d",
            false,
            &R2Condition::Date {
                date: "2033-01-01T00:00:00Z".to_owned()
            }
        ))
    );
    assert_eq!(
        rules
            .get(2)
            .map(|rule| (&rule.condition, rule.prefix.as_deref())),
        Some((&R2Condition::Indefinite, None))
    );
}

#[test]
fn an_r2_response_that_is_not_a_successful_rules_list_is_malformed() {
    for body in [
        "",
        "{}",
        r#"{"success":false,"errors":[{"code":10006}],"messages":[],"result":null}"#,
        r#"{"success":true,"result":{}}"#,
        r#"{"success":true,"result":{"rules":[{"id":"x","enabled":true,"condition":{"type":"Forever"}}]}}"#,
    ] {
        assert_eq!(
            parse_r2_rules(body.as_bytes()),
            Err(R2ParseError::Malformed),
            "{body}"
        );
    }
}

#[test]
fn r2_bucket_lock_rules_are_refused_even_when_indefinite() {
    let verdict = judge_r2(&r2(R2_INDEFINITE), "cold/");
    assert_eq!(verdict, Err(Refused(set(&[Refusal::GovernanceOnly]))));
}

#[test]
fn r2_with_no_covering_rule_is_unlocked() {
    assert_eq!(
        judge_r2(&[], "cold/"),
        Err(Refused(set(&[Refusal::NoLock, Refusal::GovernanceOnly])))
    );
    let rules = r2(r#"{"success":true,"result":{"rules":[
        {"id":"off","enabled":false,"condition":{"type":"Indefinite"}},
        {"id":"narrow","enabled":true,"prefix":"cold/segments/","condition":{"type":"Indefinite"}},
        {"id":"other","enabled":true,"prefix":"wal/","condition":{"type":"Indefinite"}}]}}"#);
    assert_eq!(
        judge_r2(&rules, "cold/"),
        Err(Refused(set(&[Refusal::NoLock, Refusal::GovernanceOnly])))
    );
    assert_eq!(
        judge_r2(&rules, "cold/segments/s1"),
        Err(Refused(set(&[Refusal::GovernanceOnly])))
    );
}

#[test]
fn r2_age_and_date_rules_shorter_than_six_years_are_too_short() {
    let short = format!(
        r#"{{"success":true,"result":{{"rules":[{{"id":"a","enabled":true,"prefix":"cold/","condition":{{"type":"Age","maxAgeSeconds":{}}}}}]}}}}"#,
        REQUIRED_RETENTION_SECS - 1
    );
    assert_eq!(
        judge_r2(&r2(&short), "cold/"),
        Err(Refused(set(&[
            Refusal::RetentionTooShort,
            Refusal::GovernanceOnly
        ])))
    );
    let enough = short.replace(
        &(REQUIRED_RETENTION_SECS - 1).to_string(),
        &REQUIRED_RETENTION_SECS.to_string(),
    );
    assert_eq!(
        judge_r2(&r2(&enough), "cold/"),
        Err(Refused(set(&[Refusal::GovernanceOnly])))
    );
    let dated = r#"{"success":true,"result":{"rules":[
        {"id":"d","enabled":true,"condition":{"type":"Date","date":"2099-01-01T00:00:00Z"}}]}}"#;
    assert_eq!(
        judge_r2(&r2(dated), "cold/"),
        Err(Refused(set(&[
            Refusal::RetentionTooShort,
            Refusal::GovernanceOnly
        ])))
    );
}

#[test]
fn the_strictest_covering_r2_rule_decides_the_length() {
    let rules = r2(r#"{"success":true,"result":{"rules":[
        {"id":"short","enabled":true,"condition":{"type":"Age","maxAgeSeconds":60}},
        {"id":"forever","enabled":true,"prefix":"cold","condition":{"type":"Indefinite"}}]}}"#);
    assert_eq!(
        judge_r2(&rules, "cold/"),
        Err(Refused(set(&[Refusal::GovernanceOnly])))
    );
    assert_eq!(
        judge_r2(&rules, "wal/"),
        Err(Refused(set(&[
            Refusal::RetentionTooShort,
            Refusal::GovernanceOnly
        ])))
    );
}
