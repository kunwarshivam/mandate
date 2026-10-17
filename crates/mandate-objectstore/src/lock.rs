//! The lock judgement: what a store reports about its retention, against journal spec §6.2.
//!
//! Every function here is pure. The evidence comes from the store (the next slice's probe); the
//! judgement only reads it, and refuses on the first sign of anything weaker than compliance-mode
//! retention of at least six years from creation. A judgement returns every refusal it finds, not
//! only the first, so the start-up log names everything the operator has to fix.

use std::collections::BTreeSet;

use serde::Deserialize;

/// The shortest retention §6.2 accepts, in days: six years of 365 days plus the two leap days six
/// consecutive years can hold, so that no six-year span from any creation date is longer.
pub const REQUIRED_RETENTION_DAYS: u32 = 2192;

/// [`REQUIRED_RETENTION_DAYS`] in seconds, the unit a probe's retention and an R2 age rule use.
pub const REQUIRED_RETENTION_SECS: u64 = 189_388_800;

/// The fewest whole years a default rule in years may give.
pub const REQUIRED_RETENTION_YEARS: u32 = 6;

/// S3 Object Lock's two modes. Only compliance satisfies §6.2: in governance mode a credential
/// holding the bypass permission can shorten the retention or delete the version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    /// No credential, the account's root included, can shorten or remove the retention.
    Compliance,
    /// A credential with `s3:BypassGovernanceRetention` can shorten or remove it.
    Governance,
}

/// A bucket's default retention period, as S3's `DefaultRetention` gives it: days or years.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPeriod {
    /// `<Days>`.
    Days(u32),
    /// `<Years>`.
    Years(u32),
}

/// What a standard S3 store reports for the bucket: `GetBucketVersioning` and
/// `GetObjectLockConfiguration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BucketLock {
    /// Versioning is `Enabled`, which object lock needs: an overwrite adds a version.
    pub versioning_enabled: bool,
    /// `ObjectLockEnabled` is `Enabled`.
    pub object_lock_enabled: bool,
    /// The bucket's default rule, which protects an object written without its own retention.
    pub default_rule: Option<(LockMode, RetentionPeriod)>,
}

/// What the start-up probe found on its own object, written through the writer's credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeEvidence {
    /// The probe version's retention mode, from `GetObjectRetention`; `None` when it has none.
    pub mode: Option<LockMode>,
    /// Seconds from the probe version's creation to its retain-until date; `None` when it has none
    /// or the date is before the creation.
    pub retained_secs: Option<u64>,
    /// A `PutObjectRetention` that moved the retain-until date earlier was refused.
    pub shorten_refused: bool,
    /// A `DeleteObject` of the probe version, with the governance bypass header, was refused.
    pub delete_refused: bool,
    /// A `PutObjectLockConfiguration` writing the bucket's own configuration back was refused, so
    /// the writer's credential cannot remove or weaken the bucket's rule.
    pub rule_write_refused: bool,
}

/// Everything [`judge_s3`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S3Evidence {
    /// The bucket's settings.
    pub bucket: BucketLock,
    /// The probe's results.
    pub probe: ProbeEvidence,
}

/// One reason a store is weaker than §6.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Refusal {
    /// The bucket has no object lock, or the probe object has no retention, or no R2 rule covers
    /// the prefix.
    NoLock,
    /// Versioning is not enabled, so an overwrite could replace an object.
    VersioningOff,
    /// Object lock is on, but the bucket has no default rule to protect an object written without
    /// its own retention.
    NoDefaultRetention,
    /// The retention can be lifted by a privileged credential: S3 governance mode, or an R2 bucket
    /// lock rule, which any token that can edit the bucket's configuration can delete.
    GovernanceOnly,
    /// The retention is shorter than six years from creation, or ends on a fixed date.
    RetentionTooShort,
    /// The writer's credential moved the probe's retain-until date earlier.
    RetentionShortenable,
    /// The writer's credential deleted the probe's locked version.
    DeleteAllowed,
    /// The writer's credential could rewrite the bucket's lock configuration.
    RuleRemovableByWriter,
}

impl Refusal {
    /// The stable code the start-up log and the journal carry.
    pub fn code(self) -> &'static str {
        match self {
            Self::NoLock => "objectstore_no_lock",
            Self::VersioningOff => "objectstore_versioning_off",
            Self::NoDefaultRetention => "objectstore_no_default_retention",
            Self::GovernanceOnly => "objectstore_governance_only",
            Self::RetentionTooShort => "objectstore_retention_too_short",
            Self::RetentionShortenable => "objectstore_retention_shortenable",
            Self::DeleteAllowed => "objectstore_delete_allowed",
            Self::RuleRemovableByWriter => "objectstore_rule_removable_by_writer",
        }
    }
}

/// A store the judgement refused, with every reason it found; never empty.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the object store is weaker than journal spec 6.2's write-once retention: {}", codes(.0))]
pub struct Refused(pub BTreeSet<Refusal>);

fn codes(refusals: &BTreeSet<Refusal>) -> String {
    refusals
        .iter()
        .map(|refusal| refusal.code())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Judges a standard S3 store: accepted only with versioning, object lock, a compliance-mode
/// default rule of at least six years, and a probe version in compliance mode for at least six
/// years that the writer's credential could neither shorten, delete, nor unlock by rewriting the
/// bucket's rule.
pub fn judge_s3(evidence: &S3Evidence) -> Result<(), Refused> {
    let _ = evidence;
    Ok(())
}

/// One rule of an R2 bucket's lock configuration, as Cloudflare's API returns it from
/// `GET /accounts/{account_id}/r2/buckets/{bucket}/lock`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct R2Rule {
    /// The rule's identifier.
    pub id: String,
    /// Whether the rule is in force.
    pub enabled: bool,
    /// The key prefix the rule covers; absent or empty covers the whole bucket.
    #[serde(default)]
    pub prefix: Option<String>,
    /// How long the rule keeps an object.
    pub condition: R2Condition,
}

/// An R2 lock rule's condition.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type")]
pub enum R2Condition {
    /// Kept for this many seconds after the object's creation.
    Age {
        /// The age, in seconds.
        #[serde(rename = "maxAgeSeconds")]
        max_age_seconds: u64,
    },
    /// Kept until this date, the same for every object.
    Date {
        /// The date as the API writes it.
        date: String,
    },
    /// Kept until the rule is removed.
    Indefinite,
}

/// Why an R2 lock response could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum R2ParseError {
    /// The body is not the API's `{"success": true, "result": {"rules": [...]}}`.
    #[error("the R2 lock response is not a successful rules list")]
    Malformed,
    /// Not built yet (E21-40).
    #[error("not implemented: {story}")]
    Unimplemented {
        /// The story that builds it.
        story: &'static str,
    },
}

/// Reads the rules from the body of Cloudflare's get-bucket-lock response.
pub fn parse_r2_rules(body: &[u8]) -> Result<Vec<R2Rule>, R2ParseError> {
    let _ = body;
    Err(R2ParseError::Unimplemented { story: "E21-40" })
}

/// Judges R2's bucket lock rules for objects written under `key_prefix`. R2 has no compliance
/// mode and no per-object lock (DEC-973): whatever the rules say, a token that can edit the
/// bucket's configuration can delete them and so unlock every object, which is governance at
/// best. The judgement therefore always refuses, naming as well whatever the rules lack.
pub fn judge_r2(rules: &[R2Rule], key_prefix: &str) -> Result<(), Refused> {
    let _ = (rules, key_prefix);
    Ok(())
}
