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
use mandate_time::{Date, UtcNanos};

use crate::condition::Condition;
use crate::{ParseError, SchemaDec};

mod parse;

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

    /// The version a journal record names (journal spec §9.2): a digest is a version only once the
    /// record has been mapped against the document stored under it, or names the document a
    /// confirmation or a provenance list is about.
    pub(crate) fn named(digest: Digest) -> Self {
        Self(digest)
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
///
/// The one private field is the canonical value the fields were read from, so [`Mandate::parse`] is
/// the only way to make one and [`Mandate::version`] hashes exactly what the owner confirmed.
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
    /// The canonical value this mandate was parsed from, which is what [`Mandate::canonical`] hashes.
    source: Value,
}

impl Mandate {
    /// Parses a canonical value into a mandate, strictly: an unknown member, a missing required
    /// member, a decimal sent as a JSON number, a value off its enum or pattern, an integer outside
    /// its bounds, or a decimal off its field's `$def` is a [`ParseError`] naming the pointer.
    ///
    /// The input is `mandate_canon::Value` and not `serde_json::Value` because a mandate reaches the
    /// platform as canonical JSON (journal spec §4); this crate carries no JSON parser of its own.
    ///
    /// Accepting exactly what `jsonschema` accepts is ES-22, and the 31 MC-S cases are the test. The
    /// schema's `date` is a pattern, not a calendar, so `2026-02-30` parses (MC-V22) and V-015 rejects
    /// it: such an `end_date` is `None` in the typed goal and its text stays in the document, which is
    /// where V-015 reads it (DEC-151).
    pub fn parse(value: &Value) -> Result<Self, ParseError> {
        parse::mandate(value)
    }

    /// The canonical JSON the version hashes: the value the document was parsed from, kept whole, so
    /// the version hash is the one the owner confirmed. Re-serialising the typed fields instead would
    /// sort a set-like array V-009 must see unsorted and drop a date V-015 must see invalid.
    ///
    /// The fields are public, so this re-parses that value and refuses with
    /// [`ParseError::Diverged`] when the fields no longer match it: a version never names anything
    /// but the mandate the gate enforces.
    pub fn canonical(&self) -> Result<Value, ParseError> {
        if Self::parse(&self.source)? == *self {
            Ok(self.source.clone())
        } else {
            Err(ParseError::Diverged)
        }
    }

    /// `to_canonical` of [`Mandate::canonical`].
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ParseError> {
        Ok(mandate_canon::to_canonical(&self.canonical()?))
    }

    /// The version: `sha256:` plus the SHA-256 of those bytes (§9.1).
    pub fn version(&self) -> Result<MandateVersion, ParseError> {
        Ok(MandateVersion(Digest::of(&self.canonical_bytes()?)))
    }

    /// The value at a pointer, for provenance (§2.1), policy reporting (§4.3), and classification
    /// (§9.2). `Ok(None)` when the pointer names nothing, which is a different answer from "this is
    /// not implemented" — hence the `Result`, so the stub cannot pass for "the field is absent".
    ///
    /// It reads the document as parsed, through [`Mandate::canonical`], so a mandate whose fields were
    /// changed afterwards is `diverged` here too rather than answering from a document it no longer is.
    pub fn at(&self, path: &Pointer) -> Result<Option<Value>, ParseError> {
        Ok(pointer(&self.canonical()?, path.as_str()).cloned())
    }
}

/// The value at an RFC 6901 pointer. Canonical keys are `[a-z][a-z0-9_]*`, so no token needs the
/// RFC's `~` escapes; an array index is digits with no leading zero, as the RFC says.
pub(crate) fn pointer<'a>(document: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(document);
    }
    path.strip_prefix('/')?
        .split('/')
        .try_fold(document, |node, token| match node {
            Value::Array(items) => {
                let canonical = token == "0" || !token.starts_with('0');
                let digits = !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit());
                (canonical && digits)
                    .then(|| token.parse::<usize>().ok())
                    .flatten()
                    .and_then(|index| items.get(index))
            }
            other => other.get(token),
        })
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
    /// The schema's `$defs/id`: one to 64 of `[A-Za-z0-9_-]`. A grammar check with no rule logic,
    /// admitted under DEC-128 item 22 for the reason [`ModelId::parse`] was: V-031 compares against a
    /// [`PreviousVersion`](crate::validate::PreviousVersion) the caller supplies, and a type nothing can
    /// construct cannot be supplied (DEC-161).
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        if parse::is_id(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(ParseError::OffPattern {
                path: Pointer::new("/connection_id"),
            })
        }
    }

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
    /// The schema's form: `fast.`, `llm.`, or `quant.` and one to 48 of `[a-z0-9_]`.
    ///
    /// A grammar check with no rule logic (DEC-128 item 22). It exists because a caller has to be able
    /// to name a registered model: V-007 compares the document's models against a registry the caller
    /// supplies, and a registry keyed by a type nothing can construct cannot be supplied at all — which
    /// is what the `mandate` harness ran into when it came to read the reference fixture's own registry.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let off_pattern = || ParseError::OffPattern {
            path: Pointer::new("/behavior/signal_models/id"),
        };
        let (kind, name) = text.split_once('.').ok_or_else(off_pattern)?;
        if !matches!(kind, "fast" | "llm" | "quant") {
            return Err(off_pattern());
        }
        if !(1..=48).contains(&name.len())
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(off_pattern());
        }
        Ok(Self(text.to_owned()))
    }

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

/// Every `LimitAction` is a `LadderAction` with the same name, which is what lets a daily-loss trigger
/// journal its action beside a rung's (§5.10). The reverse is not total: `scale_sizes` is not a daily-loss
/// action, and `LimitAction` stays two-valued so `daily_loss_action` cannot hold one (DEC-128 item 29).
impl From<LimitAction> for LadderAction {
    fn from(action: LimitAction) -> Self {
        match action {
            LimitAction::ExitsOnly => Self::ExitsOnly,
            LimitAction::FlattenAndPause => Self::FlattenAndPause,
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
    /// The review date (§6.6, DEC-188): the last risk day on which any `auto` stands. From the
    /// next risk day, §6.2 step 5b turns every `auto` an opening or an increase would get into
    /// `ask` (MI-32). `None` is no review date. Until E6-14's implementation, the parse refuses a
    /// document that sets one as unimplemented, so it fails closed rather than loading without it.
    pub review_by: Option<Date>,
    /// The owner's standing yeses (§6.5, DEC-181): each turns one kind of `ask` into `auto`, bounded
    /// and expiring; none when the member is absent. V-041 to V-043 bound them, and the order path
    /// lifts none until E8-8, which is the stricter side (DEC-420 item 7).
    pub delegations: Vec<Delegation>,
}

/// One delegation (§6.5): a bounded, expiring permission that lifts one kind of `ask` to `auto`.
/// It never touches a limit, the gate, a `deny`, a built-in decision, or the admission ceiling
/// (MI-26), and only the owner creates it, in a confirmed version (V-022).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    /// Unique among the mandate's delegations (V-041); the id the journal records.
    pub id: DelegationId,
    pub lifts: Lifts,
    /// A condition in §6.3's language, typed by V-023; the order must match it too.
    pub when: Condition,
    /// The largest single order it lifts (V-043).
    pub max_order_usd: SchemaDec,
    /// How many orders it may lift in total, 1 to 1,000.
    pub max_orders: u32,
    /// The total order value it may lift (V-043).
    pub max_total_usd: SchemaDec,
    /// The window it lifts in, `[starts_at, expires_at)`, at most 30 days (V-041). The schema's
    /// `instant` is a pattern, not a calendar, so a text it matches that names no instant parses
    /// (ES-22), is `None` here, and V-041 refuses it, as V-015 does a date (DEC-151, DEC-420).
    pub starts_at: Option<UtcNanos>,
    pub expires_at: Option<UtcNanos>,
    /// The approval request it was chosen on, or `None` when the owner created it in settings.
    pub source_approval_id: Option<ApprovalId>,
}

/// The `ask` a delegation answers (§6.5): the default, or the rule it names, whose `then` must be
/// `ask` (V-041).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lifts {
    Default,
    Rule(RuleId),
}

impl Lifts {
    /// The schema's `$defs/delegation/properties/lifts`: `default`, or `rule:` and a rule id.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        match text.strip_prefix("rule:") {
            None if text == "default" => Ok(Self::Default),
            Some(id) => RuleId::parse(id)
                .map(Self::Rule)
                .map_err(|_| ParseError::OffPattern {
                    path: Pointer::new("/autonomy/delegations/lifts"),
                }),
            None => Err(ParseError::OffPattern {
                path: Pointer::new("/autonomy/delegations/lifts"),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DelegationId(String);

impl DelegationId {
    /// The schema's `$defs/delegation/properties/id`: `^[a-z][a-z0-9_]{0,31}$`, the rule id's grammar.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        RuleId::parse(text)
            .map(|id| Self(id.as_str().to_owned()))
            .map_err(|_| ParseError::OffPattern {
                path: Pointer::new("/autonomy/delegations/id"),
            })
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An approval request's id: the schema's `$defs/uuid`, lower-case hex in 8-4-4-4-12.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApprovalId(String);

impl ApprovalId {
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let groups: Vec<&str> = text.split('-').collect();
        let shaped = groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
            && groups.iter().all(|g| {
                g.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            });
        if shaped {
            Ok(Self(text.to_owned()))
        } else {
            Err(ParseError::OffPattern {
                path: Pointer::new("/autonomy/delegations/source_approval_id"),
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
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
    /// The schema's `$defs/rule/properties/id`: `^[a-z][a-z0-9_]{0,31}$`.
    ///
    /// A grammar check with no rule logic, added by stream H's tests PR under DEC-128 item 22 for
    /// the reason that admitted [`ModelId::parse`]: §6.2's rule walk is stream H's, and an
    /// [`Autonomy`] block keyed by a type with no public constructor cannot be built at all, so
    /// every autonomy test would have to go through the mandate parser to reach one rule.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let off_pattern = || ParseError::OffPattern {
            path: Pointer::new("/autonomy/rules/id"),
        };
        let mut bytes = text.bytes();
        let first = bytes.next().ok_or_else(off_pattern)?;
        if !first.is_ascii_lowercase() || text.len() > 32 {
            return Err(off_pattern());
        }
        if !bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
            return Err(off_pattern());
        }
        Ok(Self(text.to_owned()))
    }

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
    /// The schema's approver form: `role:approver`, `role:owner`, `role:workspace_admin`, or
    /// `user:` and one to 64 of `[A-Za-z0-9_-]`. A grammar check with no rule logic, added beside
    /// [`RuleId::parse`] and for the same reason (DEC-128 item 22): an [`Approval`] cannot be built
    /// without one.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let off_pattern = || ParseError::OffPattern {
            path: Pointer::new("/autonomy/approval/approvers"),
        };
        let allowed = match text.split_once(':') {
            Some(("role", role)) => matches!(role, "approver" | "owner" | "workspace_admin"),
            Some(("user", user)) => {
                (1..=64).contains(&user.len())
                    && user
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            }
            _ => false,
        };
        if allowed {
            Ok(Self(text.to_owned()))
        } else {
            Err(off_pattern())
        }
    }

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
