//! The hashed envelope document ([mandate spec §3](../../../docs/specs/mandate.md#3-structure),
//! §9.1) and the strict parse that is "passes the JSON Schema" in Rust (ES-22).
//!
//! Everything the owner confirms is here. What the owner does **not** confirm is not: provenance
//! (§2.1) is a separate map, so the same fields hash the same however they were sourced, and the
//! strategy fields of §3.2 — the working universe, the theses, the lineage state, the day's research
//! spend — are folds of journalled events that streams G and J own and that no version can change.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Value};
use mandate_domain::{AssetClass, AssetId, AutonomyDecision, Environment};
use mandate_time::Date;

use crate::condition::Condition;
use crate::{ParseError, SchemaDec};

/// A JSON Pointer into a mandate (RFC 6901), which is also how provenance, policy violations, and
/// change classification name a field.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pointer(String);

impl Pointer {
    pub fn new(path: &str) -> Self {
        Self(path.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for Pointer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `sha256:` followed by the SHA-256 of the mandate's canonical JSON (§9.1, journal spec §4).
///
/// Provenance is not in the hash and neither is the working universe, so confirming the same fields
/// through different paths, or admitting an instrument, gives the same version (§3.2, §9.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MandateVersion(Digest);

impl MandateVersion {
    pub fn digest(self) -> Digest {
        self.0
    }
}

/// The schema version this crate reads. `mandate_schema_version` is a `const 1` in the schema.
pub const MANDATE_SCHEMA_VERSION: u32 = 1;

/// The mandate document.
///
/// Fields are public because they are plain confirmed data, as in every other safety-critical crate
/// here. The invariants live in the types that carry them instead: a [`SchemaDec`] is text already in
/// its field's grammar, and a [`ValidatedMandate`](crate::ValidatedMandate) is a document that passed
/// every rule. That keeps the one thing a caller must not be able to fake — an unvalidated mandate
/// reaching the gate — unrepresentable, without accessors that a stub could not return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mandate {
    pub mandate_schema_version: u32,
    pub name: AgentName,
    pub source_text_ref: Option<Digest>,
    pub environment: Environment,
    pub connection_id: ConnectionId,
    pub capital: Capital,
    pub goal: Goal,
    pub universe: Universe,
    pub behavior: Behavior,
    pub protection: Protection,
    pub risk: Risk,
    pub autonomy: Autonomy,
    pub notifications: Notifications,
}

impl Mandate {
    /// Parses a canonical value into a mandate, strictly: an unknown member, a missing required
    /// member, a decimal sent as a JSON number, a value off its enum or pattern, an integer outside
    /// its bounds, or a decimal off its field's `$def` is a [`ParseError`] naming the pointer.
    ///
    /// The input is `mandate_canon::Value` and not `serde_json::Value` because a mandate reaches the
    /// platform as canonical JSON (journal spec §4); this crate carries no JSON parser of its own.
    ///
    /// Accepting exactly what `jsonschema` accepts is ES-22, and the 31 MC-S cases are the test.
    pub fn parse(value: &Value) -> Result<Self, ParseError> {
        let _ = value;
        Err(ParseError::Unimplemented)
    }

    /// The canonical JSON the version hashes, byte for byte as the writer produces it for the value
    /// the document was parsed from. Round-tripping is what keeps a version hash equal to the one the
    /// owner confirmed, and it holds because every schema decimal grammar is canonical (DEC-128 item 3).
    pub fn canonical(&self) -> Result<Value, ParseError> {
        Err(ParseError::Unimplemented)
    }

    /// `to_canonical` of [`Mandate::canonical`].
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ParseError> {
        Err(ParseError::Unimplemented)
    }

    /// The version: `sha256:` plus the SHA-256 of those bytes (§9.1).
    pub fn version(&self) -> Result<MandateVersion, ParseError> {
        Err(ParseError::Unimplemented)
    }

    /// The value at a pointer, for provenance (§2.1), policy reporting (§4.3), and classification
    /// (§9.2). `None` when the pointer names nothing.
    pub fn at(&self, path: &Pointer) -> Option<Value> {
        let _ = path;
        None
    }
}

/// The agent's name: lowercase and hyphens (§3).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentName(String);

impl AgentName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The broker connection a mandate is bound to, immutable across versions (V-031).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectionId(String);

impl ConnectionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The agent's capital allocation and its lifetime loss floor (§5.1, §5.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capital {
    pub allocation_usd: SchemaDec,
    pub max_loss_from_allocation: SchemaDec,
}

/// What the agent is for, and what happens when it is done (§3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Goal {
    Continuous {
        end_date: Option<Date>,
        on_complete: OnComplete,
    },
    /// Buys one instrument only; the universe is pinned to it and admits nothing (V-003), and
    /// discretionary exits are disabled.
    Accumulate {
        instrument: AssetId,
        target_qty: SchemaDec,
        max_avg_price: Option<SchemaDec>,
        max_spend_usd: SchemaDec,
        end_date: Option<Date>,
        on_complete: OnComplete,
    },
    /// A stop condition, never a target: the level at which the agent stops (§3.1).
    ProfitStop {
        profit_level: SchemaDec,
        end_date: Option<Date>,
    },
}

impl Goal {
    pub fn end_date(&self) -> Option<&Date> {
        match self {
            Self::Continuous { end_date, .. }
            | Self::Accumulate { end_date, .. }
            | Self::ProfitStop { end_date, .. } => end_date.as_ref(),
        }
    }

    /// The `on_complete` an `accumulate` or `continuous` goal chose. A `profit_stop` has none: §3.1
    /// gives it one outcome, a discretionary exit of every position and then retirement.
    pub fn on_complete(&self) -> Option<OnComplete> {
        match self {
            Self::Continuous { on_complete, .. } | Self::Accumulate { on_complete, .. } => {
                Some(*on_complete)
            }
            Self::ProfitStop { .. } => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Continuous { .. } => "continuous",
            Self::Accumulate { .. } => "accumulate",
            Self::ProfitStop { .. } => "profit_stop",
        }
    }
}

/// What the owner chose to happen when the goal completes (§3.1). An envelope field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OnComplete {
    /// Holding: restriction `goal_complete`, every limit still armed.
    HoldProtected,
    /// Holding with the ladder and the daily loss disarmed; protection and the floor stay armed.
    DisarmLadder,
    /// Protective orders cancelled, positions become the owner's, the agent retires.
    Release,
}

impl OnComplete {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HoldProtected => "hold_protected",
            Self::DisarmLadder => "disarm_ladder",
            Self::Release => "release",
        }
    }
}

/// Which instruments the agent may hold, and how many (§2.3, §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Universe {
    /// Bring-your-own-strategy: the pinned list *is* the working universe and admission adds nothing
    /// (MI-20, V-037).
    pub pinned: bool,
    pub pinned_instruments: Vec<InstrumentRef>,
    pub max_instruments: u32,
    pub asset_classes: BTreeSet<AssetClass>,
    pub leveraged_etps_enabled: bool,
    pub leveraged_etp_disclosure_version: Option<Digest>,
}

/// A pinned instrument as the owner chose it (§3).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstrumentRef {
    pub asset_id: AssetId,
    pub symbol: String,
    pub asset_class: AssetClass,
}

/// How the agent decides: its description, its signal models, the research agent's envelope, the
/// evaluation cadence, and sizing (§3, §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Behavior {
    pub description: String,
    pub signal_models: Vec<SignalModel>,
    /// Non-null exactly when one model admits instruments (V-036).
    pub research: Option<Research>,
    pub cadence: Cadence,
    pub sizing: Sizing,
}

/// A pinned signal model: id, version, content hash, parameters, and its fixed weight (§8.1, V-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalModel {
    pub id: ModelId,
    pub version: String,
    pub content_hash: Digest,
    pub params: Vec<ModelParam>,
    pub weight: SchemaDec,
    pub max_output_age_s: u32,
    /// True for the research agent, of which there is at most one, with an `llm.` id (V-036, §8.4).
    pub admits_instruments: bool,
}

/// A registered model's id, prefixed `quant.`, `fast.`, or `llm.` (§8.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelId(String);

impl ModelId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The type prefix the policy hierarchy's `signal_model_types` constrains (§4.3).
    pub fn model_type(&self) -> Option<&str> {
        self.0.split_once('.').map(|(kind, _)| kind)
    }
}

/// One model parameter. Every parameter is set by the owner; schemas carry no defaults (§8.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelParam {
    pub key: String,
    pub value: ParamValue,
}

/// A parameter's value. The decimal branch is the only place the signed `decimal` grammar appears in
/// the whole document (DEC-128 item 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamValue {
    Decimal(SchemaDec),
    Bool(bool),
    Text(String),
}

/// The research agent's envelope fields (§8.4): how often it may propose, what it may spend in a risk
/// day, and how many revisions a lineage may carry (DEC-111, DEC-120).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Research {
    pub interval_s: u32,
    pub cost_cap_usd_per_day: SchemaDec,
    pub max_revisions_per_lineage: u32,
}

/// When the agent evaluates (§3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cadence {
    pub interval_s: u32,
    pub event_sources: BTreeSet<EventSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventSource {
    Fills,
    Filings,
    News,
    Price,
    Schedule,
}

/// The order builder's method and thresholds (§8.3). v1 has one method and no calibration (DEC-47).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sizing {
    pub method: SizingMethod,
    pub entry_threshold: SchemaDec,
    pub exit_threshold: SchemaDec,
    pub rebalance_band: SchemaDec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SizingMethod {
    ConvictionLinear,
}

/// Resting protection at the broker (§3). Distances are fractions of the entry price; the crypto
/// offset is a fraction of the stop price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protection {
    pub enabled: bool,
    pub stop_distance: Option<SchemaDec>,
    pub take_profit_distance: Option<SchemaDec>,
    pub crypto_stop_limit_offset: Option<SchemaDec>,
}

/// The limits, their timings, and what a scale rung does (§5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Risk {
    pub max_position_usd: SchemaDec,
    pub max_position_fraction: SchemaDec,
    pub max_gross_exposure_usd: SchemaDec,
    pub max_order_usd: SchemaDec,
    pub max_orders_per_day: u32,
    pub max_daily_loss: SchemaDec,
    pub daily_loss_action: LimitAction,
    pub max_drawdown: SchemaDec,
    pub drawdown_ladder: Vec<LadderRung>,
    pub hysteresis: SchemaDec,
    pub breach_confirm_s: u32,
    pub daily_breach_min_s: u32,
    pub scale_lift_after_s: u32,
    pub reentry_cooldown_s: u32,
    pub scale_action: ScaleAction,
}

/// One rung of the drawdown ladder (§5.5). `factor` is set for `scale_sizes` and null otherwise
/// (V-010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LadderRung {
    pub at: SchemaDec,
    pub action: LadderAction,
    pub factor: Option<SchemaDec>,
}

/// A rung's action, in non-decreasing severity as V-010 requires of the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LadderAction {
    ScaleSizes,
    ExitsOnly,
    FlattenAndPause,
}

impl LadderAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ScaleSizes => "scale_sizes",
            Self::ExitsOnly => "exits_only",
            Self::FlattenAndPause => "flatten_and_pause",
        }
    }
}

/// What a confirmed daily-loss breach does (§5.4). The ladder's two stricter actions share the names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LimitAction {
    ExitsOnly,
    FlattenAndPause,
}

impl LimitAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExitsOnly => "exits_only",
            Self::FlattenAndPause => "flatten_and_pause",
        }
    }
}

/// What an active `scale_sizes` rung does to positions (§5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScaleAction {
    /// Multiplies the order builder's targets and nothing else.
    LimitBuys,
    /// Also sells a position down to the scaled size as a `risk_exit`, under four guards.
    TrimToTarget,
}

/// The autonomy rules, the default, the admission ceiling, and the approval settings (§6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Autonomy {
    /// Order is significant: the first match decides (§6.2 step 4).
    pub rules: Vec<Rule>,
    pub default: AutonomyDecision,
    /// The ceiling on the first order in a newly admitted instrument (§6.2 step 5, MI-17). The
    /// platform default is `ask`.
    pub admission: AutonomyDecision,
    pub approval: Approval,
}

/// One autonomy rule. This is the crate's `Rule`, which is why a V-code is a
/// [`Violation`](crate::Violation) and not a `Rule`; stream H reads both by these names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: RuleId,
    pub when: Condition,
    pub then: AutonomyDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleId(String);

impl RuleId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How an ASKed action is approved (§6.4). `on_timeout` is always `skip`, so it is a unit rather than
/// a choice the type could get wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub timeout_s: u32,
    pub on_timeout: OnTimeout,
    pub approvers: Vec<ApproverRef>,
    pub two_approver_above_usd: Option<SchemaDec>,
}

/// The only value §6.4 allows: doing nothing skips the action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OnTimeout {
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApproverRef(String);

impl ApproverRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where approvals and alerts go, and when they are held (§3, §6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notifications {
    pub channels: BTreeSet<Channel>,
    pub quiet_hours: Option<QuietHours>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    Email,
    Phone,
    Slack,
    Sms,
    Telegram,
    WebPush,
}

/// Quiet hours in America/New_York, the only zone the schema allows. Approval requests are not
/// delivered inside them and therefore time out and are skipped; risk-limit alerts ignore them
/// (§6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuietHours {
    pub start: HourMinute,
    pub end: HourMinute,
}

/// `HH:MM`, 24 hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HourMinute {
    pub hour: u8,
    pub minute: u8,
}

/// Where each field came from and whether the owner confirmed it (§2.1).
///
/// It is **not** in the hashed document: `MandateVersionCreated` records it and `MandateConfirmed`
/// binds it to the hash, so confirming the same values through different paths gives one version.
/// A path the map does not mention is `user_entered` and confirmed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProvenanceMap(BTreeMap<Pointer, Provenance>);

impl ProvenanceMap {
    pub fn new(entries: BTreeMap<Pointer, Provenance>) -> Self {
        Self(entries)
    }

    /// The provenance of a path, or the default every unmentioned path carries.
    pub fn at(&self, path: &Pointer) -> Provenance {
        self.0.get(path).copied().unwrap_or(Provenance {
            source: Source::UserEntered,
            confirmed: true,
        })
    }

    pub fn entries(&self) -> &BTreeMap<Pointer, Provenance> {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provenance {
    pub source: Source,
    pub confirmed: bool,
}

/// How a field's value was arrived at (§2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// Extracted from the owner's own words, with the quoted span recorded.
    UserStated,
    /// Entered by the owner in the form or the editor.
    UserEntered,
    /// Present because a template included the field.
    TemplateStructure,
    /// Proposed by the compiler or a template: shown as proposed, inactive until confirmed, never
    /// allowed on a pinned universe, the environment, the connection, or any `auto` (V-022, V-038).
    PlatformProposed,
    /// Filled by the platform, allowed only on the closed list of §7 and only with the listed value.
    PlatformDefault,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserStated => "user_stated",
            Self::UserEntered => "user_entered",
            Self::TemplateStructure => "template_structure",
            Self::PlatformProposed => "platform_proposed",
            Self::PlatformDefault => "platform_default",
        }
    }

    /// The three sources V-020 accepts on an envelope field, all of which are the owner's.
    pub fn is_owner_sourced(self) -> bool {
        matches!(
            self,
            Self::UserStated | Self::UserEntered | Self::PlatformProposed
        )
    }
}
