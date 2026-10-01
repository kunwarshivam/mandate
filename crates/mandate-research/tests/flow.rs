//! E17-6's correlated-flow controls: the aggregate-flow monitor's sums, shares, and DEC-123
//! thresholds, the operator halt's matching and its exits-only removal, and the two-workspace
//! isolation the acceptance clause names.
//!
//! Every expected figure is hand-computed here. DEC-123's threshold is the lower of 1% of an
//! instrument's 20-day average daily dollar volume and 1,000,000 USD: an ADV of `10000000`
//! makes the 1% arm `100000` (the arm that binds), an ADV of `1000000000` makes it
//! `10000000` (so the dollar arm `1000000` binds), an ADV of `200000000` makes it `2000000`
//! (min `1000000`, max `2000000`: exposure `1500000` alerts under the min and reads quiet under
//! the max — the discriminator), an ADV of `50000000` makes it `500000` (exposure `750000`
//! alerts under the min, quiet under the max), and an ADV of `100000000` is the crossover where
//! both arms are `1000000`. The shares are exact: `150000 ÷ 10000000 = 0.015`,
//! `600.75 ÷ 1000000000 = 0.00000060075`, and `1 ÷ 3` rounds once at 12 places to
//! `0.333333333333`. The sums are `300.5 + 200.25 + 100 = 600.75`, and the pinned agent's
//! `5000000` never enters one, because a pinned mandate's flow is not research flow.
//!
//! Every test is pending until E17-6 lands (DEC-77), and every one fails on the stubs because
//! the stubs return an error where each case expects a figure, a set, or a refusal it does not
//! make. The negative cases travel with positive ones, so a do-nothing implementation fails
//! them all: an absence is asserted beside a presence, an unchanged universe beside a removal,
//! and a refusal beside the row that proves the input was well formed.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    INSTRUMENT_1, INSTRUMENT_2, INSTRUMENT_3, INSTRUMENT_4, INSTRUMENT_5, Scenario, asset, digest,
    entry, lineage_id, thesis_id, universe_of, usd, workspace,
};
use mandate_num::{Ratio, Usd};
use mandate_research::flow::{
    AgentFlow, FlowInput, FlowReport, FlowRow, OperatorHalt, aggregate_flow, apply_operator_halts,
    halted_instruments,
};
use mandate_research::{
    AssetId, ContentHash, InstrumentRestriction, ResearchError, ResearchEvent, UniverseChange,
    UniverseChangeReason, UniverseChangedEntry, WorkingUniverse,
};

/// One research agent's content hash (DEC-67), as the halt scopes it.
const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// A second research agent's content hash, so a hash-scoped halt can miss one and match the other.
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// An exposure (or ADV) map from `(instrument, amount)` pairs.
fn exposure(pairs: &[(&str, &str)]) -> BTreeMap<AssetId, Usd> {
    pairs
        .iter()
        .map(|(id, amount)| (asset(id), usd(amount)))
        .collect()
}

/// The report of one deployment's rows, refusing nothing a well-formed input can ask for.
fn report_of<'a>(agents: &'a [AgentFlow<'a>], adv: &'a BTreeMap<AssetId, Usd>) -> FlowReport {
    aggregate_flow(&FlowInput {
        agents,
        average_daily_dollar_volume: adv,
    })
    .expect("the deployment's rows report")
}

/// The row of an instrument that holds research exposure, so a missing row fails by name.
fn row_of<'r>(report: &'r FlowReport, instrument: &str) -> &'r FlowRow {
    report
        .rows
        .iter()
        .find(|row| row.instrument == asset(instrument))
        .unwrap_or_else(|| {
            panic!("instrument {instrument} holds research exposure, so it has a row")
        })
}

/// A halted-instrument set from instrument ids.
fn halted(ids: &[&str]) -> BTreeSet<AssetId> {
    ids.iter().map(|id| asset(id)).collect()
}

/// A research agent's pinned content hash from 32 hex bytes.
fn agent_hash(hex: &str) -> ContentHash {
    ContentHash::new(digest(hex))
}

/// One operator halt: an instrument, and optionally the research agent it is scoped to.
fn halt(instrument: &str, research_agent: Option<ContentHash>) -> OperatorHalt {
    OperatorHalt {
        instrument: asset(instrument),
        research_agent,
    }
}

/// The `UniverseChanged` removal a halt journals for `instrument`'s thesis `thesis`, with the
/// working-universe size after it — the expected value the tests hand-compute.
fn removal_entry(instrument: &str, thesis: &str, size_after: usize) -> ResearchEvent {
    ResearchEvent::UniverseChanged(UniverseChangedEntry {
        instrument: asset(instrument),
        change: UniverseChange::Removed,
        reason: UniverseChangeReason::OperatorHalt,
        thesis_id: thesis_id(thesis),
        lineage_id: lineage_id(thesis),
        universe_size_after: size_after,
    })
}

/// DEC-100: the deployment sums research exposure per instrument. Three agents over two
/// workspaces — two of them in one workspace — hold `300.5 + 200.25 + 100 = 600.75` of
/// instrument 1 and `120000` of instrument 2, and the row names each contributing workspace
/// once.
#[test]
#[ignore = "pending E17-6"]
fn the_deployment_sums_research_exposure_per_instrument() {
    let ws_one = workspace("ws-one");
    let ws_two = workspace("ws-two");
    let map_one = exposure(&[(INSTRUMENT_1, "300.5"), (INSTRUMENT_2, "120000")]);
    let map_one_second_agent = exposure(&[(INSTRUMENT_1, "200.25")]);
    let map_two = exposure(&[(INSTRUMENT_1, "100")]);
    let agents = [
        AgentFlow {
            workspace: &ws_one,
            pinned: false,
            exposure: &map_one,
        },
        AgentFlow {
            workspace: &ws_one,
            pinned: false,
            exposure: &map_one_second_agent,
        },
        AgentFlow {
            workspace: &ws_two,
            pinned: false,
            exposure: &map_two,
        },
    ];
    let adv = exposure(&[(INSTRUMENT_1, "1000000000"), (INSTRUMENT_2, "10000000")]);
    let report = report_of(&agents, &adv);
    assert_eq!(
        report.rows.len(),
        2,
        "two instruments hold research exposure, so the report holds two rows"
    );
    let first = row_of(&report, INSTRUMENT_1);
    assert_eq!(
        first.exposure_usd,
        usd("600.75"),
        "300.5 + 200.25 + 100 sums across every agent and every workspace (DEC-100)"
    );
    assert!(
        !first.alerts,
        "600.75 is under the dollar arm's 1000000, so the row is quiet"
    );
    assert_eq!(
        first.contributing_workspaces,
        vec![workspace("ws-one"), workspace("ws-two")],
        "the row names each contributing workspace once, however many agents it hosts"
    );
    let second = row_of(&report, INSTRUMENT_2);
    assert_eq!(
        second.exposure_usd,
        usd("120000"),
        "instrument 2's exposure is its one agent's alone"
    );
    assert!(
        second.alerts,
        "120000 is above 1% of a 10000000 ADV, so the row alerts (DEC-123)"
    );
    assert_eq!(
        second.contributing_workspaces,
        vec![workspace("ws-one")],
        "only ws-one's agent holds instrument 2"
    );
}

/// The acceptance clause's bring-your-own-strategy sentence: a pinned agent's exposure never
/// enters the aggregate, and an instrument only pinned agents hold has no row at all.
#[test]
#[ignore = "pending E17-6"]
fn a_pinned_agent_contributes_nothing_to_the_aggregate() {
    let ws_pin = workspace("ws-pin");
    let ws_dyn = workspace("ws-dyn");
    let map_pin = exposure(&[(INSTRUMENT_1, "5000000"), (INSTRUMENT_2, "900000")]);
    let map_dyn = exposure(&[(INSTRUMENT_1, "150000")]);
    let agents = [
        AgentFlow {
            workspace: &ws_pin,
            pinned: true,
            exposure: &map_pin,
        },
        AgentFlow {
            workspace: &ws_dyn,
            pinned: false,
            exposure: &map_dyn,
        },
    ];
    let adv = exposure(&[(INSTRUMENT_1, "10000000")]);
    let report = report_of(&agents, &adv);
    assert_eq!(
        report.rows.len(),
        1,
        "only instrument 1 carries research exposure, because the pinned agent's is not research flow"
    );
    let row = row_of(&report, INSTRUMENT_1);
    assert_eq!(
        row.exposure_usd,
        usd("150000"),
        "the pinned agent's 5000000 never enters the sum"
    );
    assert!(
        row.alerts,
        "150000 is above 1% of a 10000000 ADV, on the dynamic agent's exposure alone"
    );
    assert_eq!(
        row.contributing_workspaces,
        vec![workspace("ws-dyn")],
        "the pinned agent's workspace does not contribute"
    );
}

/// DEC-123's 1% arm: an ADV of `10000000` makes the threshold `100000`, so exposure at the bar
/// is quiet and one cent above it alerts — the comparison is strict at the bar.
#[test]
#[ignore = "pending E17-6"]
fn a_thin_name_alerts_above_one_percent_of_its_adv() {
    let ws = workspace("ws-thin");
    let map = exposure(&[(INSTRUMENT_1, "100000"), (INSTRUMENT_2, "100000.01")]);
    let agents = [AgentFlow {
        workspace: &ws,
        pinned: false,
        exposure: &map,
    }];
    let adv = exposure(&[(INSTRUMENT_1, "10000000"), (INSTRUMENT_2, "10000000")]);
    let report = report_of(&agents, &adv);
    assert!(
        !row_of(&report, INSTRUMENT_1).alerts,
        "100000 is exactly 1% of a 10000000 ADV: at the bar, quiet (DEC-123)"
    );
    assert!(
        row_of(&report, INSTRUMENT_2).alerts,
        "100000.01 is one cent above 1% of a 10000000 ADV: it alerts"
    );
}

/// DEC-123's dollar arm: an ADV of `1000000000` makes 1% of it `10000000`, so the threshold is
/// the `1000000` arm — at the bar quiet, one cent above it alerts.
#[test]
#[ignore = "pending E17-6"]
fn a_megacap_alerts_above_the_dollar_threshold() {
    let ws = workspace("ws-mega");
    let map = exposure(&[(INSTRUMENT_1, "1000000"), (INSTRUMENT_2, "1000000.01")]);
    let agents = [AgentFlow {
        workspace: &ws,
        pinned: false,
        exposure: &map,
    }];
    let adv = exposure(&[(INSTRUMENT_1, "1000000000"), (INSTRUMENT_2, "1000000000")]);
    let report = report_of(&agents, &adv);
    assert!(
        !row_of(&report, INSTRUMENT_1).alerts,
        "1000000 is exactly the dollar arm: at the bar, quiet"
    );
    assert!(
        row_of(&report, INSTRUMENT_2).alerts,
        "1000000.01 is one cent above the dollar arm: it alerts"
    );
}

/// DEC-123's "whichever is lower": exposure `1500000` over an ADV of `200000000` (1% is
/// `2000000`) and exposure `750000` over an ADV of `50000000` (1% is `500000`) both alert under
/// the min and read quiet under a max — and at the crossover ADV of `100000000` both arms are
/// `1000000`, quiet at the bar and alerting one cent above it.
#[test]
#[ignore = "pending E17-6"]
fn the_threshold_is_whichever_arm_is_lower() {
    let ws = workspace("ws-min");
    let map = exposure(&[
        (INSTRUMENT_1, "1500000"),
        (INSTRUMENT_2, "750000"),
        (INSTRUMENT_3, "1000000"),
        (INSTRUMENT_4, "1000000.01"),
    ]);
    let agents = [AgentFlow {
        workspace: &ws,
        pinned: false,
        exposure: &map,
    }];
    let adv = exposure(&[
        (INSTRUMENT_1, "200000000"),
        (INSTRUMENT_2, "50000000"),
        (INSTRUMENT_3, "100000000"),
        (INSTRUMENT_4, "100000000"),
    ]);
    let report = report_of(&agents, &adv);
    assert!(
        row_of(&report, INSTRUMENT_1).alerts,
        "1500000 is under the max arm's 2000000 and above the min arm's 1000000: it alerts"
    );
    assert!(
        row_of(&report, INSTRUMENT_2).alerts,
        "750000 is under the max arm's 1000000 and above the min arm's 500000: it alerts"
    );
    assert!(
        !row_of(&report, INSTRUMENT_3).alerts,
        "at the crossover ADV both arms are 1000000: at the bar, quiet"
    );
    assert!(
        row_of(&report, INSTRUMENT_4).alerts,
        "one cent above the crossover's 1000000: it alerts"
    );
}

/// The report states each instrument's share of its ADV, one rounding of one quotient:
/// `150000 ÷ 10000000 = 0.015` exactly, `600.75 ÷ 1000000000 = 0.00000060075` exactly, and
/// `1 ÷ 3` rounds once at 12 places to `0.333333333333`.
#[test]
#[ignore = "pending E17-6"]
fn the_report_states_each_instrument_s_share_of_adv() {
    let ws = workspace("ws-share");
    let map = exposure(&[
        (INSTRUMENT_1, "150000"),
        (INSTRUMENT_2, "600.75"),
        (INSTRUMENT_3, "1"),
    ]);
    let agents = [AgentFlow {
        workspace: &ws,
        pinned: false,
        exposure: &map,
    }];
    let adv = exposure(&[
        (INSTRUMENT_1, "10000000"),
        (INSTRUMENT_2, "1000000000"),
        (INSTRUMENT_3, "3"),
    ]);
    let report = report_of(&agents, &adv);
    assert_eq!(
        row_of(&report, INSTRUMENT_1).share_of_adv,
        Some(Ratio::parse("0.015").expect("a fixture share parses")),
        "150000 ÷ 10000000 is 0.015 exactly"
    );
    assert_eq!(
        row_of(&report, INSTRUMENT_2).share_of_adv,
        Some(Ratio::parse("0.00000060075").expect("a fixture share parses")),
        "600.75 ÷ 1000000000 is 0.00000060075 exactly"
    );
    assert_eq!(
        row_of(&report, INSTRUMENT_3).share_of_adv,
        Some(Ratio::parse("0.333333333333").expect("a fixture share parses")),
        "1 ÷ 3 rounds once at the 12-place report scale to 0.333333333333"
    );
}

/// A missing ADV is undecidable, not quiet: the row alerts with the share named absent, even
/// below the dollar arm, because the 1% arm may bind lower — and the rest of the report lives,
/// so one missing figure does not blind the monitor to every other instrument.
#[test]
#[ignore = "pending E17-6"]
fn a_missing_adv_alerts_even_below_the_dollar_threshold() {
    let ws = workspace("ws-gap");
    let map = exposure(&[(INSTRUMENT_1, "500000"), (INSTRUMENT_2, "150000")]);
    let agents = [AgentFlow {
        workspace: &ws,
        pinned: false,
        exposure: &map,
    }];
    let adv = exposure(&[(INSTRUMENT_2, "10000000")]);
    let report = report_of(&agents, &adv);
    let missing = row_of(&report, INSTRUMENT_1);
    assert_eq!(
        missing.share_of_adv, None,
        "a missing ADV leaves the share named absent, never guessed"
    );
    assert!(
        missing.alerts,
        "500000 is under the dollar arm's 1000000, but the 1% arm is unknown and may bind lower: undecidable alerts (AGENTS.md rule 3)"
    );
    let present = row_of(&report, INSTRUMENT_2);
    assert_eq!(
        present.share_of_adv,
        Some(Ratio::parse("0.015").expect("a fixture share parses")),
        "the report lives on for the instruments whose ADV is present"
    );
    assert!(present.alerts, "150000 is above 1% of 10000000");
}

/// A zero ADV decides: the 1% arm is zero, so any exposure alerts; the share stays absent,
/// because nothing divides by zero.
#[test]
#[ignore = "pending E17-6"]
fn a_zero_adv_alerts_on_any_exposure() {
    let ws = workspace("ws-zero");
    let map = exposure(&[(INSTRUMENT_1, "1"), (INSTRUMENT_2, "100000")]);
    let agents = [AgentFlow {
        workspace: &ws,
        pinned: false,
        exposure: &map,
    }];
    let adv = exposure(&[(INSTRUMENT_1, "0"), (INSTRUMENT_2, "10000000")]);
    let report = report_of(&agents, &adv);
    let zero = row_of(&report, INSTRUMENT_1);
    assert!(
        zero.alerts,
        "1% of a zero ADV is zero, so any exposure is above the threshold"
    );
    assert_eq!(
        zero.share_of_adv, None,
        "a share of a zero ADV is never computed, so it is named absent"
    );
    let present = row_of(&report, INSTRUMENT_2);
    assert_eq!(
        present.share_of_adv,
        Some(Ratio::parse("0.01").expect("a fixture share parses")),
        "the zero-ADV row does not stop the report's other shares"
    );
}

/// An instrument with no research exposure — a zero row, or no row at all — is absent from the
/// report, and the instruments that do hold exposure are present beside it.
#[test]
#[ignore = "pending E17-6"]
fn an_instrument_with_no_research_exposure_is_absent_from_the_report() {
    let ws_one = workspace("ws-one");
    let ws_two = workspace("ws-two");
    let map_one = exposure(&[(INSTRUMENT_1, "150000")]);
    let map_two = exposure(&[(INSTRUMENT_2, "0")]);
    let agents = [
        AgentFlow {
            workspace: &ws_one,
            pinned: false,
            exposure: &map_one,
        },
        AgentFlow {
            workspace: &ws_two,
            pinned: false,
            exposure: &map_two,
        },
    ];
    let adv = exposure(&[
        (INSTRUMENT_1, "10000000"),
        (INSTRUMENT_2, "10000000"),
        (INSTRUMENT_3, "10000000"),
    ]);
    let report = report_of(&agents, &adv);
    assert_eq!(
        report.rows.len(),
        1,
        "only instrument 1 holds research exposure: instrument 2 holds an explicit zero and instrument 3 is never held"
    );
    assert_eq!(
        report.rows[0].instrument,
        asset(INSTRUMENT_1),
        "the one row is the instrument that holds exposure"
    );
}

/// A negative exposure row is refused: v1 is long-only, and folding a negative amount would
/// under-state the aggregate — the fail-open direction. The positive input reports its row
/// beside the refusal.
#[test]
#[ignore = "pending E17-6"]
fn a_negative_exposure_is_refused() {
    let ws = workspace("ws-sign");
    let map_ok = exposure(&[(INSTRUMENT_1, "1")]);
    let agents_ok = [AgentFlow {
        workspace: &ws,
        pinned: false,
        exposure: &map_ok,
    }];
    let adv = exposure(&[(INSTRUMENT_1, "10000000")]);
    let ok = report_of(&agents_ok, &adv);
    assert_eq!(
        ok.rows.len(),
        1,
        "the positive input reports its row, proving the refusal below is about the sign"
    );
    let map_negative = exposure(&[(INSTRUMENT_1, "-1")]);
    let agents_negative = [AgentFlow {
        workspace: &ws,
        pinned: false,
        exposure: &map_negative,
    }];
    let refused = aggregate_flow(&FlowInput {
        agents: &agents_negative,
        average_daily_dollar_volume: &adv,
    });
    assert!(
        matches!(&refused, Err(ResearchError::NegativeExposure)),
        "a research exposure is never negative (AGENTS.md rule 12: no short sales in v1): got {refused:?}"
    );
}

/// The report is ordered by instrument id, whatever order the agents' maps or the rows' sizes
/// would give.
/// The report is ordered by instrument id, whatever order the agents' maps or the rows' sizes
/// would give: instrument 2 holds the largest exposure and instrument 1 the smallest, so an
/// exposure-ordered report would read `[2, 3, 1]` against the id order's `[1, 2, 3]`.
#[test]
#[ignore = "pending E17-6"]
fn the_report_is_ordered_by_instrument_id() {
    let ws = workspace("ws-order");
    let map = exposure(&[
        (INSTRUMENT_3, "2"),
        (INSTRUMENT_1, "1"),
        (INSTRUMENT_2, "3000000"),
    ]);
    let agents = [AgentFlow {
        workspace: &ws,
        pinned: false,
        exposure: &map,
    }];
    let adv = exposure(&[
        (INSTRUMENT_1, "10000000"),
        (INSTRUMENT_2, "10000000"),
        (INSTRUMENT_3, "10000000"),
    ]);
    let report = report_of(&agents, &adv);
    assert_eq!(
        report.rows.len(),
        3,
        "three instruments hold research exposure, so the report holds three rows"
    );
    let ordered: Vec<_> = report.rows.iter().map(|row| &row.instrument).collect();
    assert_eq!(
        ordered,
        vec![
            &asset(INSTRUMENT_1),
            &asset(INSTRUMENT_2),
            &asset(INSTRUMENT_3)
        ],
        "the rows are ordered by instrument id, not by exposure (instrument 2's is the largest and instrument 1's the smallest)"
    );
}

/// DEC-100: an unscoped halt — no research-agent hash — matches every workspace, whatever its
/// admitting model, and a pinned mandate's workspace (no admitting model) is matched too,
/// because check 5 and the pinned universe are what hold a pinned mandate back, not the halt's
/// matching.
#[test]
#[ignore = "pending E17-6"]
fn an_unscoped_halt_matches_every_workspace() {
    let unscoped = halt(INSTRUMENT_1, None);
    assert_eq!(
        halted_instruments(std::slice::from_ref(&unscoped), Some(&agent_hash(HASH_A)))
            .expect("the matching is total"),
        halted(&[INSTRUMENT_1]),
        "an unscoped halt matches a workspace with an admitting model"
    );
    assert_eq!(
        halted_instruments(&[unscoped], None).expect("the matching is total"),
        halted(&[INSTRUMENT_1]),
        "an unscoped halt matches a pinned mandate's workspace too"
    );
}

/// DEC-100: a hash-scoped halt matches only the workspace whose admitting model carries the
/// named content hash — a different version is not matched, and a pinned mandate (no admitting
/// model) is matched by none of them.
#[test]
#[ignore = "pending E17-6"]
fn a_hash_scoped_halt_matches_only_its_own_research_agent() {
    let halts = [
        halt(INSTRUMENT_1, Some(agent_hash(HASH_A))),
        halt(INSTRUMENT_2, Some(agent_hash(HASH_B))),
    ];
    assert_eq!(
        halted_instruments(&halts, Some(&agent_hash(HASH_A))).expect("the matching is total"),
        halted(&[INSTRUMENT_1]),
        "the halt scoped to agent A matches agent A's workspace, and only its instrument"
    );
    assert_eq!(
        halted_instruments(&halts, Some(&agent_hash(HASH_B))).expect("the matching is total"),
        halted(&[INSTRUMENT_2]),
        "the halt scoped to agent B matches agent B's workspace, and only its instrument"
    );
    assert_eq!(
        halted_instruments(&halts, None).expect("the matching is total"),
        halted(&[]),
        "a pinned mandate has no admitting model, so no hash-scoped halt reaches it"
    );
}

/// DEC-100's halt applied: the halted instrument leaves the working universe, journaled as a
/// `UniverseChanged` removal with reason `operator_halt` citing the thesis it held, the
/// restriction is the exits-only `RemovedInstrument`, and the instrument the halt did not name
/// stays.
#[test]
#[ignore = "pending E17-6"]
fn a_halt_removes_the_instrument_from_the_working_universe() {
    let universe = universe_of(&[INSTRUMENT_1, INSTRUMENT_2]);
    let entries = [
        entry(
            INSTRUMENT_1,
            "th-1",
            "th-1",
            "2026-09-23T14:00:00.000000000Z",
            false,
        ),
        entry(
            INSTRUMENT_2,
            "th-2",
            "th-2",
            "2026-09-23T14:00:00.000000000Z",
            false,
        ),
    ];
    let outcome = apply_operator_halts(&halted(&[INSTRUMENT_1]), &entries, &universe)
        .expect("a read dynamic universe is halted");
    assert_eq!(
        outcome.removed,
        vec![asset(INSTRUMENT_1)],
        "the halted instrument is removed"
    );
    assert_eq!(
        outcome.universe,
        universe_of(&[INSTRUMENT_2]),
        "the instrument the halt did not name stays"
    );
    assert_eq!(
        outcome.instrument_restrictions,
        BTreeMap::from([(
            asset(INSTRUMENT_1),
            InstrumentRestriction::RemovedInstrument
        )]),
        "the removal earns the exits-only restriction (§2.2), never a mode change"
    );
    assert_eq!(
        outcome.journal,
        vec![removal_entry(INSTRUMENT_1, "th-1", 1)],
        "one removal is journaled, citing the thesis it held and the size after it (journal spec §9)"
    );
}

/// Two removals journal in instrument order — not the entry list's order, which the fixture
/// scrambles — with `universe_size_after` sinking by one per removal.
#[test]
#[ignore = "pending E17-6"]
fn removals_are_journalled_in_instrument_order_with_sinking_sizes() {
    let universe = universe_of(&[INSTRUMENT_1, INSTRUMENT_2, INSTRUMENT_3]);
    let entries = [
        entry(
            INSTRUMENT_3,
            "th-3",
            "th-3",
            "2026-09-23T14:00:00.000000000Z",
            false,
        ),
        entry(
            INSTRUMENT_1,
            "th-1",
            "th-1",
            "2026-09-23T14:00:00.000000000Z",
            false,
        ),
        entry(
            INSTRUMENT_2,
            "th-2",
            "th-2",
            "2026-09-23T14:00:00.000000000Z",
            false,
        ),
    ];
    let outcome = apply_operator_halts(&halted(&[INSTRUMENT_1, INSTRUMENT_3]), &entries, &universe)
        .expect("a read dynamic universe is halted");
    assert_eq!(
        outcome.removed,
        vec![asset(INSTRUMENT_1), asset(INSTRUMENT_3)],
        "the removals are ordered by instrument id, though the entries arrive scrambled"
    );
    assert_eq!(
        outcome.journal,
        vec![
            removal_entry(INSTRUMENT_1, "th-1", 2),
            removal_entry(INSTRUMENT_3, "th-3", 1),
        ],
        "each removal journals with the size after it: 2 then 1, sinking from 3"
    );
    assert_eq!(
        outcome.universe,
        universe_of(&[INSTRUMENT_2]),
        "the unheld-back instrument stays, and only it"
    );
}

/// The acceptance clause's bring-your-own-strategy sentence, on the halt's side: the same halt
/// that removes from a dynamic universe leaves a pinned universe alone — the pinned list is the
/// owner's, and a pinned mandate has no research flow to stop.
#[test]
#[ignore = "pending E17-6"]
fn the_same_halt_leaves_a_pinned_universe_alone_and_removes_from_a_dynamic_one() {
    let dynamic = universe_of(&[INSTRUMENT_1]);
    let entries = [entry(
        INSTRUMENT_1,
        "th-1",
        "th-1",
        "2026-09-23T14:00:00.000000000Z",
        false,
    )];
    let halted_set = halted(&[INSTRUMENT_1]);
    let removed = apply_operator_halts(&halted_set, &entries, &dynamic)
        .expect("a read dynamic universe is halted");
    assert_eq!(
        removed.removed,
        vec![asset(INSTRUMENT_1)],
        "in a dynamic universe the halt removes the instrument it names"
    );
    let pinned = WorkingUniverse::Known {
        instruments: halted(&[INSTRUMENT_1]),
        pinned: true,
    };
    let untouched = apply_operator_halts(&halted_set, &[], &pinned)
        .expect("a read pinned universe is answered, not halted");
    assert_eq!(
        untouched.universe, pinned,
        "a pinned universe is returned exactly as it was"
    );
    assert_eq!(
        untouched.removed,
        Vec::<AssetId>::new(),
        "a pinned universe holds no research theses, so nothing is removed"
    );
    assert!(
        untouched.journal.is_empty(),
        "a pinned universe journals nothing for a halt"
    );
}

/// An unread working universe is never halted silently: the fold cannot know what it holds, and
/// a dropped halt would be the fail-open direction, so it is an error — the same reading
/// admission takes.
#[test]
#[ignore = "pending E17-6"]
fn an_unread_universe_is_never_halted_silently() {
    let refused =
        apply_operator_halts(&halted(&[INSTRUMENT_1]), &[], &WorkingUniverse::Unavailable);
    assert!(
        matches!(&refused, Err(ResearchError::UniverseUnavailable)),
        "an unread universe is an error, never an empty halt (AGENTS.md rule 3): got {refused:?}"
    );
}

/// A halt naming an instrument the universe does not hold removes nothing and journals nothing —
/// the halt holds that instrument's admissions through check 9, which needs no removal — while
/// the instrument it does hold is removed beside it.
#[test]
#[ignore = "pending E17-6"]
fn a_halt_removes_the_held_instrument_and_leaves_one_the_universe_does_not_hold() {
    let universe = universe_of(&[INSTRUMENT_1]);
    let entries = [entry(
        INSTRUMENT_1,
        "th-1",
        "th-1",
        "2026-09-23T14:00:00.000000000Z",
        false,
    )];
    let outcome = apply_operator_halts(&halted(&[INSTRUMENT_1, INSTRUMENT_2]), &entries, &universe)
        .expect("a read dynamic universe is halted");
    assert_eq!(
        outcome.removed,
        vec![asset(INSTRUMENT_1)],
        "only the instrument the universe holds is removed"
    );
    assert_eq!(
        outcome.journal,
        vec![removal_entry(INSTRUMENT_1, "th-1", 0)],
        "one removal is journaled, and nothing for the instrument the universe does not hold"
    );
    assert_eq!(
        outcome.universe,
        universe_of(&[]),
        "the universe ends empty but known, never unavailable"
    );
}

/// DEC-100: the halt never permits anything a workspace's own limits deny. Its application
/// produces removals only — every journal entry is a `UniverseChanged` removal with reason
/// `operator_halt`, every restriction is the exits-only one, and the universe it returns is a
/// subset of the one it was handed.
#[test]
#[ignore = "pending E17-6"]
fn the_halt_only_ever_removes() {
    let universe = universe_of(&[INSTRUMENT_1, INSTRUMENT_2, INSTRUMENT_3]);
    let entries = [
        entry(
            INSTRUMENT_1,
            "th-1",
            "th-1",
            "2026-09-23T14:00:00.000000000Z",
            false,
        ),
        entry(
            INSTRUMENT_2,
            "th-2",
            "th-2",
            "2026-09-23T14:00:00.000000000Z",
            false,
        ),
        entry(
            INSTRUMENT_3,
            "th-3",
            "th-3",
            "2026-09-23T14:00:00.000000000Z",
            false,
        ),
    ];
    let outcome = apply_operator_halts(&halted(&[INSTRUMENT_1, INSTRUMENT_3]), &entries, &universe)
        .expect("a read dynamic universe is halted");
    assert_eq!(
        outcome.universe,
        universe_of(&[INSTRUMENT_2]),
        "the outcome's universe is a subset of the input's: the halt only removes"
    );
    assert!(
        outcome
            .instrument_restrictions
            .values()
            .all(|restriction| *restriction == InstrumentRestriction::RemovedInstrument),
        "every restriction the halt emits is the exits-only one"
    );
    assert!(
        outcome.journal.iter().all(|event| matches!(
            event,
            ResearchEvent::UniverseChanged(change)
                if change.change == UniverseChange::Removed
                    && change.reason == UniverseChangeReason::OperatorHalt
        )),
        "every journal entry the halt emits is an operator_halt removal — no admission, no thesis entry"
    );
    assert_eq!(
        outcome.journal.len(),
        outcome.removed.len(),
        "one entry per removal, and nothing else"
    );
}

/// The entries name the dynamic universe's instruments once each, as `expire_theses` reads
/// them; a list naming one instrument twice is refused.
#[test]
#[ignore = "pending E17-6"]
fn duplicate_entries_are_refused() {
    let universe = universe_of(&[INSTRUMENT_1]);
    let entries = [
        entry(
            INSTRUMENT_1,
            "th-1",
            "th-1",
            "2026-09-23T14:00:00.000000000Z",
            false,
        ),
        entry(
            INSTRUMENT_1,
            "th-1b",
            "th-1b",
            "2026-09-23T14:00:00.000000000Z",
            false,
        ),
    ];
    let refused = apply_operator_halts(&halted(&[INSTRUMENT_1]), &entries, &universe);
    assert!(
        matches!(&refused, Err(ResearchError::DuplicateInstrument)),
        "an entry list naming one instrument twice is refused, never silently collapsed: got {refused:?}"
    );
}

/// The acceptance clause's two-workspace test, at this crate's seams: workspace A's positions
/// are visible in the monitor's report — the only cross-workspace view there is — and workspace
/// B's admission is the same whether A exists or not, because no admission input accepts another
/// workspace's state (DEC-09). The report is a value: nothing consumes it, and it changes no
/// decision.
#[test]
#[ignore = "pending E17-6"]
fn one_workspace_s_state_never_enters_another_s_decisions() {
    let b = Scenario::admitting();
    let admission_before =
        mandate_research::admit(&b.input()).expect("workspace B's own inputs admit its own thesis");
    let ws_a = workspace("ws-a");
    let ws_b = workspace("ws-b");
    let map_a = exposure(&[(INSTRUMENT_5, "900000")]);
    let map_b = exposure(&[(INSTRUMENT_5, "150000")]);
    let agents = [
        AgentFlow {
            workspace: &ws_a,
            pinned: false,
            exposure: &map_a,
        },
        AgentFlow {
            workspace: &ws_b,
            pinned: false,
            exposure: &map_b,
        },
    ];
    let adv = exposure(&[(INSTRUMENT_5, "10000000")]);
    let report = report_of(&agents, &adv);
    let row = row_of(&report, INSTRUMENT_5);
    assert_eq!(
        row.exposure_usd,
        usd("1050000"),
        "the monitor is the one view that sees both workspaces: 900000 + 150000"
    );
    assert!(
        row.alerts,
        "1050000 is above 1% of a 10000000 ADV, so the operator is shown the concentration"
    );
    let admission_after = mandate_research::admit(&b.input())
        .expect("workspace B's own inputs admit its own thesis again");
    assert_eq!(
        admission_after.decision, admission_before.decision,
        "the report's existence changes no decision: it is a value, and no admission input accepts it"
    );
    assert_eq!(
        admission_after.universe, admission_before.universe,
        "B's working universe is its own admission's, not the aggregate's"
    );
}

/// The halt reaches a workspace only through its own resolved halt set — its own journaled
/// `PlatformOperatorAction` — never by reading another workspace's state: the same halt scoped
/// to agent A's hash removes A's instrument and leaves B's, because B derives its own set from
/// its own admitting model.
#[test]
#[ignore = "pending E17-6"]
fn a_halt_reaches_a_workspace_only_through_its_own_halt_set() {
    let scoped = halt(INSTRUMENT_5, Some(agent_hash(HASH_A)));
    let ws_a_set = halted_instruments(std::slice::from_ref(&scoped), Some(&agent_hash(HASH_A)))
        .expect("the matching is total");
    assert_eq!(
        ws_a_set,
        halted(&[INSTRUMENT_5]),
        "the halt scoped to agent A matches agent A's workspace"
    );
    let ws_b_set =
        halted_instruments(&[scoped], Some(&agent_hash(HASH_B))).expect("the matching is total");
    assert_eq!(
        ws_b_set,
        halted(&[]),
        "the same halt does not match agent B's workspace: B derives its own set"
    );
    let universe = universe_of(&[INSTRUMENT_5]);
    let entries = [entry(
        INSTRUMENT_5,
        "th-5",
        "th-5",
        "2026-09-23T14:00:00.000000000Z",
        false,
    )];
    let a = apply_operator_halts(&ws_a_set, &entries, &universe)
        .expect("a read dynamic universe is halted");
    assert_eq!(
        a.removed,
        vec![asset(INSTRUMENT_5)],
        "A's own halt set removes A's instrument"
    );
    let b = apply_operator_halts(&ws_b_set, &entries, &universe)
        .expect("a read dynamic universe is answered");
    assert_eq!(
        b.removed,
        Vec::<AssetId>::new(),
        "B's own halt set is empty, so B's universe is untouched — A's halt never reads B's way"
    );
    assert_eq!(
        b.universe, universe,
        "B's working universe is exactly what it was"
    );
}
