//! `mandate inspect`: report each dataset's coverage, raw and split-adjusted statistics, corporate
//! actions, gaps with their missing slots by class, duplicates, and problems as text, in the
//! order the directories are given, and a total.

use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use mandate_marketdata::dataset::partition_name;
use mandate_marketdata::download::describe;
use mandate_marketdata::inspect::{
    ActionsReport, ClassifiedGap, GapClass, Inspection, Problem, Values, inspect,
};
use mandate_marketdata::model::{DayRange, Kind};

#[derive(Debug, Args)]
pub struct InspectArgs {
    /// Dataset directories, e.g. `data/alpaca/sip/bars-1Min/SPY`.
    #[arg(required = true)]
    pub datasets: Vec<PathBuf>,
}

fn days(range: DayRange) -> String {
    if range.first() == range.last() {
        range.first().to_string()
    } else {
        format!("{} to {}", range.first(), range.last())
    }
}

fn runs(ranges: &[DayRange]) -> String {
    ranges
        .iter()
        .map(|r| days(*r))
        .collect::<Vec<_>>()
        .join(", ")
}

fn problem(problem: &Problem) -> String {
    match problem {
        Problem::Missing { day } => format!("{}: missing", partition_name(*day)),
        Problem::Altered { day } => format!(
            "{}: size or SHA-256 differs from the manifest",
            partition_name(*day)
        ),
        Problem::Unreadable { day, reason } => {
            format!("{}: unreadable: {reason}", partition_name(*day))
        }
        Problem::RowCount { day, listed, read } => format!(
            "{}: {read} rows, the manifest lists {listed}",
            partition_name(*day)
        ),
        Problem::OutsideDay { day, records } => {
            format!("{}: {records} records outside {day}", partition_name(*day))
        }
        Problem::Unlisted { file } => format!("{file}: not in the manifest"),
    }
}

fn span(range: DayRange) -> String {
    format!("{} to {}", range.first(), range.last())
}

fn corporate_actions(report: &ActionsReport, lines: &mut Vec<String>) {
    let (recorded, as_of) = match report {
        ActionsReport::NotApplicable => {
            lines.push("corporate actions: not applicable to crypto".to_owned());
            return;
        }
        ActionsReport::NotRecorded => {
            lines.push(
                "corporate actions: not recorded with this dataset; no split-adjusted prices"
                    .to_owned(),
            );
            return;
        }
        ActionsReport::Incomplete(recorded) => {
            lines.push(format!(
                "corporate actions: recorded for {}, which does not cover the span; {} actions, none applied",
                span(recorded.range),
                recorded.actions.ids().count()
            ));
            return;
        }
        ActionsReport::Applied { recorded, as_of } => (recorded, *as_of),
    };
    lines.push(format!(
        "corporate actions: recorded for {}, applied as of {as_of}",
        span(recorded.range)
    ));
    let actions = &recorded.actions;
    lines.extend(actions.splits.iter().map(|split| {
        let applied = if split.ex_date <= as_of {
            "applied".to_owned()
        } else {
            format!("not applied, after {as_of}")
        };
        format!(
            "  split {}:{}, ex-date {} ({}): {applied}",
            split.ratio.new_shares(),
            split.ratio.old_shares(),
            split.ex_date,
            split.id
        )
    }));
    lines.extend(actions.cash_dividends.iter().map(|dividend| {
        let special = if dividend.special { ", special" } else { "" };
        let foreign = if dividend.foreign { ", foreign" } else { "" };
        format!(
            "  cash dividend {} per share{special}{foreign}, ex-date {} ({}): not applied, cash",
            dividend.rate, dividend.ex_date, dividend.id
        )
    }));
    lines.extend(actions.other.iter().map(|action| {
        let dated = match action.ex_date {
            Some(ex_date) => format!("ex-date {ex_date}"),
            None => format!("process date {}", action.process_date),
        };
        format!(
            "  {}, {dated} ({}): not applied, not interpreted",
            action.kind, action.id
        )
    }));
}

const CLASSES: [GapClass; 4] = [
    GapClass::SessionClosure,
    GapClass::NoTrade,
    GapClass::TrueGap,
    GapClass::Unclassified,
];

fn gaps(gaps: &[ClassifiedGap], lines: &mut Vec<String>) {
    let stretches = || gaps.iter().flat_map(|g| &g.stretches);
    let totals = CLASSES
        .iter()
        .map(|class| {
            let slots = stretches()
                .filter(|s| s.class == *class)
                .fold(0_u64, |total, s| total.saturating_add(s.slots));
            format!("{slots} {}", class.as_str())
        })
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(format!("gaps: {}, missing bar slots: {totals}", gaps.len()));
    for gap in gaps {
        lines.push(format!("  {} to {}", gap.gap.previous, gap.gap.next));
        lines.extend(gap.stretches.iter().map(|s| match s.slots {
            1 => format!("    {}: 1 slot, {}", s.class.as_str(), s.first),
            n => format!(
                "    {}: {n} slots, {} to {}",
                s.class.as_str(),
                s.first,
                s.last
            ),
        }));
    }
}

/// The text report of one dataset.
pub fn render(inspection: &Inspection) -> String {
    let mut lines = vec![describe(&inspection.dataset)];
    let coverage = &inspection.coverage;
    match coverage.span {
        Some(span) => {
            lines.push(format!(
                "coverage: {} to {}, {} days listed",
                span.first(),
                span.last(),
                coverage.listed
            ));
            if !coverage.empty.is_empty() {
                lines.push(format!("  empty: {}", runs(&coverage.empty)));
            }
            if !coverage.missing.is_empty() {
                lines.push(format!("  missing: {}", runs(&coverage.missing)));
            }
        }
        None => lines.push("coverage: no days listed".to_owned()),
    }
    match &inspection.stats {
        Some(stats) => {
            lines.push(format!(
                "rows: {}, first {}, last {}",
                stats.rows, stats.first, stats.last
            ));
            lines.push(match &stats.values {
                Values::Bars {
                    low,
                    high,
                    volume,
                    trade_count,
                } => format!(
                    "bars: low {low}, high {high}, volume {volume}, trade count {trade_count}"
                ),
                Values::Trades { low, high, size } => {
                    format!("trades: low {low}, high {high}, size {size}")
                }
                Values::Quotes { .. } => "quotes: not summarized yet".to_owned(),
            });
            if let (Some(adjusted), ActionsReport::Applied { as_of, .. }) =
                (&stats.adjusted, &inspection.corporate_actions)
            {
                lines.push(format!(
                    "split-adjusted as of {as_of}: low {}, high {}",
                    adjusted.low, adjusted.high
                ));
            }
        }
        None => lines.push("rows: 0".to_owned()),
    }
    corporate_actions(&inspection.corporate_actions, &mut lines);
    match inspection.dataset.kind() {
        Kind::Trades => lines.push("gaps: not applicable to trades".to_owned()),
        Kind::Quotes => lines.push("gaps: not applicable to quotes".to_owned()),
        Kind::Bars(_) if inspection.gaps.is_empty() => lines.push("gaps: 0".to_owned()),
        Kind::Bars(_) => gaps(&inspection.gaps, &mut lines),
    }
    lines.push(format!("duplicates: {}", inspection.duplicates.len()));
    lines.extend(inspection.duplicates.iter().map(|d| {
        let trade = d
            .trade_id
            .map(|id| format!(" trade {id}"))
            .unwrap_or_default();
        let same = if d.identical {
            "identical"
        } else {
            "differing"
        };
        format!("  {}{trade} x{} {same}", d.time, d.count)
    }));
    lines.push(format!("problems: {}", inspection.problems.len()));
    lines.extend(
        inspection
            .problems
            .iter()
            .map(|p| format!("  {}", problem(p))),
    );
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Inspects every dataset of `args`, writing each report and a total to `report`, and returns the
/// number of problems found.
pub fn run(args: &InspectArgs, report: &mut impl Write) -> anyhow::Result<u64> {
    let mut problems = 0_u64;
    for dir in &args.datasets {
        let inspection = inspect(dir).with_context(|| format!("inspecting {}", dir.display()))?;
        problems = problems.saturating_add(u64::try_from(inspection.problems.len())?);
        writeln!(report, "{}", render(&inspection)).context("writing the report")?;
    }
    writeln!(
        report,
        "total: {} datasets, {problems} problems",
        args.datasets.len()
    )
    .context("writing the report")?;
    Ok(problems)
}
