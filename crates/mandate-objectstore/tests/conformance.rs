//! The conformance suite: the lock check and the locked writer against a real S3-compatible store
//! (the digest-pinned local test store in CI; any provider by configuration, DEC-973 item 7).
//! Skipped unless `MANDATE_S3_URL` is set, and required in CI (`MANDATE_S3_REQUIRED`).
//!
//! Each test makes its own buckets, so the probe and every refusal are judged on a store set up
//! exactly as the test says: compliance accepted only with a writer that cannot touch the rule,
//! and each weaker setting refused with its own reason.

mod common;

use std::collections::BTreeSet;

use jiff::{SignedDuration, Timestamp};
use mandate_objectstore::check::{
    CheckError, LockedWriter, PROBE_KEY, WRITE_RETENTION_DAYS, WriteError, check_s3, gather_s3,
};
use mandate_objectstore::lock::{
    LockMode, REQUIRED_RETENTION_SECS, Refusal, Refused, RetentionPeriod,
};
use mandate_objectstore::s3::{Outcome, S3Error};

const PREFIX: &str = "cold/";
const SIX_YEARS: Option<(LockMode, RetentionPeriod)> =
    Some((LockMode::Compliance, RetentionPeriod::Years(6)));

fn refusals(result: Result<impl std::fmt::Debug, CheckError>) -> BTreeSet<Refusal> {
    match result {
        Err(CheckError::Refused(Refused(set))) => set,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn set(items: &[Refusal]) -> BTreeSet<Refusal> {
    items.iter().copied().collect()
}

#[tokio::test]
async fn compliance_with_a_writer_that_cannot_touch_the_rule_is_accepted_and_writes_locked() {
    let Some(root) = common::root("accept") else {
        return;
    };
    common::create(&root, Some(SIX_YEARS)).await;
    let writer = common::narrowed(&root, common::WRITER_ACTIONS).await;

    let certificate = match check_s3(&writer, PREFIX).await {
        Ok(certificate) => certificate,
        Err(error) => panic!("the compliance store passes: {error:?}"),
    };
    assert_eq!(certificate.prefix(), PREFIX);

    let locked = match LockedWriter::new(writer.clone(), certificate) {
        Ok(locked) => locked,
        Err(error) => panic!("the certificate is this store's: {error}"),
    };
    let before = Timestamp::now();
    let version = match locked.put("segments/s-1.jsonl", b"{\"body\":1}\n").await {
        Ok(Some(version)) => version,
        other => panic!("the locked write lands with a version: {other:?}"),
    };
    let key = "cold/segments/s-1.jsonl";
    assert_eq!(
        writer.get_object(key, Some(&version)).await,
        Ok(Some(b"{\"body\":1}\n".to_vec()))
    );
    let retention = writer.get_object_retention(key, &version).await;
    let Ok(Some((mode, until))) = retention else {
        panic!("the written object has retention: {retention:?}");
    };
    assert_eq!(mode, LockMode::Compliance);
    let at_least = before
        .checked_add(SignedDuration::from_hours(
            i64::from(WRITE_RETENTION_DAYS) * 24 - 1,
        ))
        .unwrap_or_else(|error| panic!("in range: {error}"));
    assert!(
        until >= at_least,
        "retained until {until}, at least {at_least}"
    );

    for outside in ["../x", "../../other-bucket/x", "segments/../../x"] {
        assert_eq!(
            locked.put(outside, b"x").await,
            Err(WriteError::Store(S3Error::InvalidKey)),
            "{outside}"
        );
    }
    assert_eq!(
        locked.put("_mandate/lockcheck", b"x").await,
        Err(WriteError::ReservedKey)
    );
    assert!(matches!(locked.put("_mandatex/k", b"x").await, Ok(Some(_))));

    for client in [&writer, &root] {
        let deleted = client.delete_object_version(key, &version, true).await;
        assert!(matches!(deleted, Ok(Outcome::Rejected(_))), "{deleted:?}");
        let shortened = client
            .put_object_retention(key, &version, LockMode::Compliance, Timestamp::now())
            .await;
        assert!(
            matches!(shortened, Ok(Outcome::Rejected(_))),
            "{shortened:?}"
        );
    }
    assert_eq!(
        writer.get_object(key, Some(&version)).await,
        Ok(Some(b"{\"body\":1}\n".to_vec()))
    );
}

#[tokio::test]
async fn the_probe_is_reused_across_restarts() {
    let Some(root) = common::root("reuse") else {
        return;
    };
    common::create(&root, Some(SIX_YEARS)).await;
    let writer = common::narrowed(&root, common::WRITER_ACTIONS).await;
    let probe = format!("{PREFIX}{PROBE_KEY}");
    assert!(check_s3(&writer, PREFIX).await.is_ok());
    let first = writer.head_object(&probe, None).await;
    assert!(check_s3(&writer, PREFIX).await.is_ok());
    let second = writer.head_object(&probe, None).await;
    let Ok(Some(first)) = first else {
        panic!("the probe exists: {first:?}");
    };
    assert_eq!(Ok(Some(first.clone())), second, "no second probe version");
    let evidence = gather_s3(&writer, PREFIX).await;
    let Ok(evidence) = evidence else {
        panic!("evidence: {evidence:?}");
    };
    assert_eq!(evidence.probe.mode, Some(LockMode::Compliance));
    assert!(evidence.probe.retained_secs >= Some(REQUIRED_RETENTION_SECS));
}

#[tokio::test]
async fn a_leftover_probe_that_is_not_compliance_locked_is_replaced() {
    let Some(root) = common::root("leftover") else {
        return;
    };
    common::create(&root, Some(SIX_YEARS)).await;
    let probe = format!("{PREFIX}{PROBE_KEY}");
    let later = Timestamp::now()
        .checked_add(SignedDuration::from_hours(3000 * 24))
        .unwrap_or_else(|error| panic!("in range: {error}"));
    let planted = root
        .put_object(&probe, b"left over", Some((LockMode::Governance, later)))
        .await;
    assert!(matches!(planted, Ok(Ok(Some(_)))), "{planted:?}");
    let writer = common::narrowed(&root, common::WRITER_ACTIONS).await;
    assert!(check_s3(&writer, PREFIX).await.is_ok());
    let head = writer.head_object(&probe, None).await;
    let Ok(Some(head)) = head else {
        panic!("the probe exists: {head:?}");
    };
    assert_eq!(
        head.retention.map(|(mode, _)| mode),
        Some(LockMode::Compliance)
    );
}

#[tokio::test]
async fn a_writer_that_can_rewrite_the_rule_is_refused() {
    let Some(root) = common::root("rootwriter") else {
        return;
    };
    common::create(&root, Some(SIX_YEARS)).await;
    assert_eq!(
        refusals(check_s3(&root, PREFIX).await),
        set(&[Refusal::RuleRemovableByWriter])
    );
}

#[tokio::test]
async fn a_governance_default_is_refused() {
    let Some(root) = common::root("governance") else {
        return;
    };
    common::create(
        &root,
        Some(Some((LockMode::Governance, RetentionPeriod::Years(6)))),
    )
    .await;
    let mut actions = common::WRITER_ACTIONS.to_vec();
    actions.push("s3:BypassGovernanceRetention");
    let writer = common::narrowed(&root, &actions).await;
    assert_eq!(
        refusals(check_s3(&writer, PREFIX).await),
        set(&[Refusal::GovernanceOnly])
    );
}

#[tokio::test]
async fn a_default_shorter_than_six_years_is_refused() {
    let Some(root) = common::root("short") else {
        return;
    };
    common::create(
        &root,
        Some(Some((LockMode::Compliance, RetentionPeriod::Days(2191)))),
    )
    .await;
    let writer = common::narrowed(&root, common::WRITER_ACTIONS).await;
    assert_eq!(
        refusals(check_s3(&writer, PREFIX).await),
        set(&[Refusal::RetentionTooShort])
    );
}

#[tokio::test]
async fn object_lock_without_a_default_rule_is_refused() {
    let Some(root) = common::root("nodefault") else {
        return;
    };
    common::create(&root, Some(None)).await;
    let writer = common::narrowed(&root, common::WRITER_ACTIONS).await;
    assert_eq!(
        refusals(check_s3(&writer, PREFIX).await),
        set(&[Refusal::NoDefaultRetention])
    );
}

#[tokio::test]
async fn a_bucket_without_object_lock_is_refused() {
    let Some(root) = common::root("nolock") else {
        return;
    };
    common::create(&root, None).await;
    let writer = common::narrowed(&root, common::WRITER_ACTIONS).await;
    assert_eq!(
        refusals(check_s3(&writer, PREFIX).await),
        set(&[
            Refusal::NoLock,
            Refusal::VersioningOff,
            Refusal::DeleteAllowed
        ])
    );
}

#[tokio::test]
async fn a_writer_that_cannot_read_the_settings_is_refused() {
    let Some(root) = common::root("blind") else {
        return;
    };
    common::create(&root, Some(SIX_YEARS)).await;
    let blind: Vec<&str> = common::WRITER_ACTIONS
        .iter()
        .copied()
        .filter(|action| {
            !matches!(
                *action,
                "s3:GetBucketVersioning"
                    | "s3:GetBucketObjectLockConfiguration"
                    | "s3:GetObjectRetention"
            )
        })
        .collect();
    let writer = common::narrowed(&root, &blind).await;
    assert_eq!(
        refusals(check_s3(&writer, PREFIX).await),
        set(&[Refusal::NoLock, Refusal::VersioningOff])
    );
}

#[tokio::test]
async fn a_plain_delete_removes_an_unlocked_object_and_only_marks_a_locked_one() {
    let Some(plain) = common::root("plaindelete") else {
        return;
    };
    common::create(&plain, None).await;
    assert!(matches!(plain.put_object("k", b"v", None).await, Ok(Ok(_))));
    assert_eq!(plain.delete_object("k").await, Ok(Outcome::Done));
    assert_eq!(plain.get_object("k", None).await, Ok(None));

    let Some(locked) = common::root("lockeddelete") else {
        return;
    };
    common::create(&locked, Some(SIX_YEARS)).await;
    let put = locked.put_object("k", b"v", None).await;
    let Ok(Ok(Some(version))) = put else {
        panic!("a versioned put: {put:?}");
    };
    assert_eq!(locked.delete_object("k").await, Ok(Outcome::Done));
    assert_eq!(locked.get_object("k", None).await, Ok(None));
    assert_eq!(
        locked.get_object("k", Some(&version)).await,
        Ok(Some(b"v".to_vec())),
        "the delete marker hides the version and removes nothing"
    );
}

#[tokio::test]
async fn a_certificate_writes_only_to_its_own_store() {
    let Some(first) = common::root("certa") else {
        return;
    };
    let Some(second) = common::root("certb") else {
        return;
    };
    common::create(&first, Some(SIX_YEARS)).await;
    let writer = common::narrowed(&first, common::WRITER_ACTIONS).await;
    let certificate = match check_s3(&writer, PREFIX).await {
        Ok(certificate) => certificate,
        Err(error) => panic!("passes: {error:?}"),
    };
    assert!(matches!(
        LockedWriter::new(second, certificate),
        Err(WriteError::CertificateMismatch)
    ));
}
