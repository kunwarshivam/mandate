//! Inputs the evaluation tests build on: the `research_equity` reference base as the stored
//! mandate, §9.4 thesis payloads that pass `check_thesis_record` against it, close series, and the
//! reference cases' fee configuration.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::BTreeMap;

use mandate_accounting::{Config, CryptoFees, EquityFees, FeeRounding, TafCapBasis};
use mandate_canon::{Digest, Int, Key, Object, Value};
use mandate_evaluation::{
    Baselines, Closes, CostModel, EvaluationReport, EvaluationRequest, ThesisRecord,
};
use mandate_num::{Bps, FeeCap, FeePerShare, FeeRate, Price, Ratio, ShareIncrement, Usd};
use mandate_research::AssetId;
use mandate_research::score::{EvaluationDecision, EvaluationWindow, ObservedClose};
use mandate_spec::Mandate;
use mandate_time::{Date, TradingCalendar, UtcNanos};

pub const RESEARCH_HASH: &str = "4444444444444444444444444444444444444444444444444444444444444444";
pub const NEXT_HASH: &str = "5555555555555555555555555555555555555555555555555555555555555555";
pub const THESIS_ASSET: &str = "7b4a1c2e-5555-4a2b-9c3d-000000000005";
pub const BASKET_ASSET: &str = "7b4a1c2e-6666-4a2b-9c3d-000000000006";
pub const INDEX_ASSET: &str = "7b4a1c2e-7777-4a2b-9c3d-000000000007";

pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap_or_else(|e| panic!("`{text}` is an instant: {e}"))
}

pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("`{text}` is a price: {e}"))
}

pub fn ratio(text: &str) -> Ratio {
    Ratio::parse(text).unwrap_or_else(|e| panic!("`{text}` is a ratio: {e}"))
}

pub fn asset(text: &str) -> AssetId {
    AssetId::new(text).unwrap_or_else(|e| panic!("`{text}` is an asset id: {e}"))
}

/// The `research_equity` base as the store holds it: its canonical document, under its version.
pub fn stored_research() -> (Digest, Value) {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../fixtures/refcases/mandate.json"))
            .unwrap_or_else(|e| panic!("the fixture parses: {e}"));
    let document = fixture
        .pointer("/bases/research_equity/mandate")
        .unwrap_or_else(|| panic!("the base exists"));
    let encoded = serde_json::to_vec(document).unwrap_or_else(|e| panic!("{e}"));
    let canonical = mandate_canon::parse(&encoded).unwrap_or_else(|e| panic!("{e}"));
    let mandate = Mandate::parse(&canonical).unwrap_or_else(|e| panic!("{e}"));
    let version = mandate.version().unwrap_or_else(|e| panic!("{e}")).digest();
    (
        version,
        mandate.canonical().unwrap_or_else(|e| panic!("{e}")),
    )
}

/// What one test thesis says.
#[derive(Debug, Clone)]
pub struct Thesis {
    pub thesis_id: String,
    pub model_version: &'static str,
    pub content_hash: &'static str,
    pub instrument: &'static str,
    pub as_of: &'static str,
    pub expires_at: &'static str,
    pub admitted: bool,
}

pub fn thesis(id: &str, as_of: &'static str, expires_at: &'static str) -> Thesis {
    Thesis {
        thesis_id: id.to_owned(),
        model_version: "0.1.0",
        content_hash: RESEARCH_HASH,
        instrument: THESIS_ASSET,
        as_of,
        expires_at,
        admitted: true,
    }
}

/// A §9.4 payload with every member: admitted, or refused at check 14 (`source_not_allowlisted`), which the
/// mandate alone never decides, so `check_thesis_record` accepts either verdict.
pub fn payload(thesis: &Thesis) -> Value {
    let text = |v: &str| Value::Str(v.to_owned());
    let int = |n: u64| Value::Int(Int::new(n).unwrap_or_else(|| panic!("an integer")));
    let digest = |c: char| text(&format!("sha256:{}", c.to_string().repeat(64)));
    let horizon = at(thesis.expires_at).secs() - at(thesis.as_of).secs();
    let members = [
        ("model_id", text("llm.research_agent")),
        ("model_version", text(thesis.model_version)),
        (
            "content_hash",
            text(&format!("sha256:{}", thesis.content_hash)),
        ),
        ("thesis_id", text(&thesis.thesis_id)),
        ("lineage_id", text(&format!("ln_{}", thesis.thesis_id))),
        ("revision", int(0)),
        ("predecessor_thesis_id", Value::Null),
        ("autopsy_ref", Value::Null),
        ("instrument_id", text(thesis.instrument)),
        ("asset_class", text("us_equity")),
        ("direction", text("long")),
        ("as_of", text(&at(thesis.as_of).to_string())),
        ("expires_at", text(&at(thesis.expires_at).to_string())),
        (
            "horizon_s",
            int(u64::try_from(horizon).unwrap_or_else(|e| panic!("{e}"))),
        ),
        ("conviction", text("0.7")),
        ("confidence", text("0.8")),
        ("evidence_ref", Value::Null),
        ("evidence_sources", Value::Array(vec![text("src.filings")])),
        ("corroboration", text("market_data")),
        ("invalidation", text("Guidance is cut.")),
        ("allowlist_version", int(1)),
        ("prompt_ref", digest('a')),
        ("response_ref", digest('b')),
        ("admitted", Value::Bool(thesis.admitted)),
        (
            "reason",
            if thesis.admitted {
                Value::Null
            } else {
                text("source_not_allowlisted")
            },
        ),
    ];
    let mut out = Object::new();
    for (name, value) in members {
        out.insert(Key::new(name).unwrap_or_else(|e| panic!("{e}")), value);
    }
    Value::Object(out)
}

/// The theses as records of one agent stream, `seq` from 10 up.
pub fn records(version: Digest, theses: &[Thesis]) -> Vec<ThesisRecord> {
    theses
        .iter()
        .enumerate()
        .map(|(index, thesis)| ThesisRecord {
            stream_id: "agent:ws_internal:agt_research".to_owned(),
            seq: 10 + u64::try_from(index).unwrap_or_else(|e| panic!("{e}")),
            hash: Digest::of(thesis.thesis_id.as_bytes()),
            mandate_version: version,
            payload: payload(thesis),
        })
        .collect()
}

fn closes_of(points: &[(&str, &str)]) -> Vec<ObservedClose> {
    points
        .iter()
        .map(|(when, value)| ObservedClose {
            at: at(when),
            price: price(value),
        })
        .collect()
}

/// The thesis instrument closes at 100 on Monday 2026-09-21 and 110 on Wednesday 2026-09-23, the
/// basket member at 100 then 105, and the index at 100 then 102; each closes once more on Thursday,
/// after the common horizon, so a thesis ending Wednesday has resolved.
pub fn closes() -> Closes {
    BTreeMap::from([
        (
            asset(THESIS_ASSET),
            closes_of(&[
                ("2026-09-21T20:00:00Z", "100"),
                ("2026-09-23T20:00:00Z", "110"),
                ("2026-09-24T20:00:00Z", "111"),
            ]),
        ),
        (
            asset(BASKET_ASSET),
            closes_of(&[
                ("2026-09-21T20:00:00Z", "100"),
                ("2026-09-23T20:00:00Z", "105"),
                ("2026-09-24T20:00:00Z", "106"),
            ]),
        ),
        (
            asset(INDEX_ASSET),
            closes_of(&[
                ("2026-09-21T20:00:00Z", "100"),
                ("2026-09-23T20:00:00Z", "102"),
                ("2026-09-24T20:00:00Z", "103"),
            ]),
        ),
    ])
}

pub fn us_2026() -> TradingCalendar {
    let d = |text: &str| Date::parse(text).unwrap_or_else(|e| panic!("{e}"));
    TradingCalendar::new(
        d("2026-09-01"),
        d("2026-12-31"),
        [d("2026-11-26"), d("2026-12-25")],
        [d("2026-10-12"), d("2026-11-11")],
    )
    .unwrap_or_else(|e| panic!("{}", e.code()))
}

/// The reference cases' fees, 3 bps of slippage a leg, and a 10,000 USD reference notional.
pub fn cost() -> CostModel {
    CostModel {
        slippage_per_leg: Bps::parse("3").unwrap_or_else(|e| panic!("{e}")),
        fees: Config {
            equities: EquityFees {
                sec_rate: FeeRate::parse("0.00003").unwrap_or_else(|e| panic!("{e}")),
                taf_per_share: FeePerShare::parse("0.0002").unwrap_or_else(|e| panic!("{e}")),
                taf_cap: FeeCap::parse("9.79").unwrap_or_else(|e| panic!("{e}")),
                fee_rounding: FeeRounding::DailyTotal,
                taf_cap_basis: TafCapBasis::PerExecution,
                cat_per_share: FeePerShare::parse("0.00001").unwrap_or_else(|e| panic!("{e}")),
            },
            crypto: CryptoFees {
                maker: Bps::parse("15").unwrap_or_else(|e| panic!("{e}")),
                taker: Bps::parse("25").unwrap_or_else(|e| panic!("{e}")),
            },
            calendar: us_2026(),
        },
        reference_notional: Usd::parse("10000").unwrap_or_else(|e| panic!("{e}")),
        increment: ShareIncrement::Whole,
    }
}

/// DEC-122's shape with a September-to-December window, a minimum of `minimum`, and `z` 1.645.
pub fn decision(minimum: u32) -> EvaluationDecision {
    EvaluationDecision {
        window: EvaluationWindow {
            from: at("2026-09-01T00:00:00Z"),
            to: at("2026-12-31T00:00:00Z"),
        },
        minimum_scoreable: minimum,
        z: ratio("1.645"),
    }
}

/// Evaluates `records` at `cut_off` against the store, the closes, and the baselines.
pub fn evaluate(
    records: &[ThesisRecord],
    stored: &(Digest, Value),
    decision: &EvaluationDecision,
    cut_off: &str,
) -> Result<EvaluationReport, mandate_evaluation::EvaluationError> {
    evaluate_with(records, stored, decision, cut_off, &closes())
}

/// [`evaluate`] over the given closes.
pub fn evaluate_with(
    records: &[ThesisRecord],
    stored: &(Digest, Value),
    decision: &EvaluationDecision,
    cut_off: &str,
    closes: &Closes,
) -> Result<EvaluationReport, mandate_evaluation::EvaluationError> {
    let (version, document) = stored.clone();
    let documents = move |digest: &Digest| (*digest == version).then(|| document.clone());
    let basket = [asset(BASKET_ASSET)];
    let index = asset(INDEX_ASSET);
    let cost = cost();
    mandate_evaluation::evaluate(&EvaluationRequest {
        decision_id: "DEC-122",
        decision,
        cut_off: at(cut_off),
        records,
        documents: &documents,
        closes,
        baselines: Baselines {
            basket: &basket,
            index: &index,
        },
        cost: &cost,
    })
}
