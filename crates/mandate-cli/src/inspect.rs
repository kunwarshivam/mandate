//! `mandate inspect`: report each dataset's coverage, statistics, gaps, duplicates, and problems
//! as text, in the order the directories are given, and a total.

use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use mandate_marketdata::dataset::partition_name;
use mandate_marketdata::download::describe;
use mandate_marketdata::inspect::{Inspection, Problem, Values, inspect};
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
            });
        }
        None => lines.push("rows: 0".to_owned()),
    }
    match inspection.dataset.kind() {
        Kind::Trades => lines.push("gaps: not applicable to trades".to_owned()),
        Kind::Bars(_) if inspection.gaps.is_empty() => lines.push("gaps: 0".to_owned()),
        Kind::Bars(_) => {
            lines.push(format!(
                "gaps: {} (the starts of the bars on either side; not yet classified by market session)",
                inspection.gaps.len()
            ));
            lines.extend(
                inspection
                    .gaps
                    .iter()
                    .map(|g| format!("  {} to {}", g.previous, g.next)),
            );
        }
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
