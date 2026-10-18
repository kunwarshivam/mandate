//! What each mutation shard cost, and the rolling history of those costs (DEC-2607 items 1 and 2).
//!
//! `cargo xtask ci mutants` writes one [`ShardCost`] a shard to [`COST_FILE`], read from
//! cargo-mutants' own `outcomes.json`: the packages its slice mutated, its mutant count, the time
//! from the gate's start, when it writes its diff, to the first mutant (the job's fixed part: the
//! pre-flight build, the enumeration, the unmutated baseline), the longest single mutant, and cargo-mutants' outcome. CI
//! uploads it as the artifact `mutant-cost-<shard>`. After the matrix, `cargo xtask ci
//! mutants-costs` folds this run's records into the latest `mutant-costs` artifact, keeping each
//! mutated package's [`KEPT_PER_PACKAGE`] most recent records, and writes [`HISTORY_FILE`] for CI
//! to upload as the new `mutant-costs`. Nothing here changes what a shard tests.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Where a shard writes its record, beside cargo-mutants' output.
pub const COST_FILE: &str = "target/mutant-cost.json";

/// Where `ci mutants-costs` writes the folded history, alone in its directory so CI uploads only it.
pub const HISTORY_FILE: &str = "target/mutant-costs/mutant-costs.json";

/// The history's artifact name.
pub const HISTORY_ARTIFACT: &str = "mutant-costs";

/// The prefix of each shard's record artifact, `mutant-cost-<shard index>`.
pub const COST_ARTIFACT_PREFIX: &str = "mutant-cost-";

/// How many records the history keeps for each mutated package, the most recent by run.
pub const KEPT_PER_PACKAGE: usize = 20;

/// One shard's cost (DEC-2607 item 1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardCost {
    /// The workflow run (`GITHUB_RUN_ID`), 0 outside CI.
    pub run: u64,
    /// The shard as `INDEX/TOTAL`, or `all` for an unsharded run.
    pub shard: String,
    /// The packages whose mutants this shard's slice holds.
    pub packages: Vec<String>,
    /// The mutants cargo-mutants tested.
    pub mutants: usize,
    /// Milliseconds from the gate's start to the first mutant: everything after it writes its diff
    /// and before cargo-mutants starts, then cargo-mutants' unmutated baseline's build and tests.
    pub fixed_ms: u64,
    /// The longest single mutant, its build and its tests, in milliseconds.
    pub longest_mutant_ms: u64,
    /// What the run ended as: `caught`, `missed`, `timeout`, `baseline-failed` or `failed`.
    pub outcome: String,
    /// Whether xtask stopped the run for running out of time (DEC-2607 item 5).
    pub overran: bool,
}

/// The records of recent shards, by the package they mutated (DEC-2607 item 2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    pub by_package: BTreeMap<String, Vec<ShardCost>>,
}

/// cargo-mutants' exit status for a run that missed mutants (cargo-mutants 27.1.0).
const MISSED: i32 = 2;
/// cargo-mutants' exit status for a run in which a mutant timed out.
const TIMED_OUT: i32 = 3;
/// cargo-mutants' exit status for a run whose unmutated baseline failed.
const BASELINE_FAILED: i32 = 4;

/// The outcome a record names for cargo-mutants' exit status.
fn outcome_for(code: Option<i32>) -> &'static str {
    match code {
        Some(0) => "caught",
        Some(MISSED) => "missed",
        Some(TIMED_OUT) => "timeout",
        Some(BASELINE_FAILED) => "baseline-failed",
        _ => "failed",
    }
}

#[derive(Deserialize)]
struct Outcomes {
    outcomes: Vec<Outcome>,
}

#[derive(Deserialize)]
struct Outcome {
    scenario: serde_json::Value,
    #[serde(default)]
    phase_results: Vec<Phase>,
}

#[derive(Deserialize)]
struct Phase {
    duration: f64,
}

/// A phase's seconds as whole milliseconds; a duration that is not a finite non-negative number
/// is refused rather than read as zero.
fn millis(seconds: f64) -> Result<u64> {
    let duration = Duration::try_from_secs_f64(seconds)
        .with_context(|| format!("a phase lasted {seconds} seconds"))?;
    u64::try_from(duration.as_millis()).context("a phase's milliseconds overflow")
}

/// The milliseconds of one scenario's phases together.
fn scenario_millis(outcome: &Outcome) -> Result<u64> {
    outcome.phase_results.iter().try_fold(0u64, |sum, phase| {
        Ok(sum.saturating_add(millis(phase.duration)?))
    })
}

/// The shard's record from cargo-mutants' `outcomes` text, the milliseconds before cargo-mutants
/// started, and its exit status. `outcomes` is `None` when cargo-mutants wrote none, as when it
/// stopped before testing anything: the record then counts no mutant and puts everything before
/// the end in the fixed part.
pub fn record(
    run: u64,
    shard: String,
    packages: Vec<String>,
    before_ms: u64,
    outcomes: Option<&str>,
    code: Option<i32>,
    overran: bool,
) -> Result<ShardCost> {
    let mut baseline_ms = 0u64;
    let mut mutants = 0usize;
    let mut longest_mutant_ms = 0u64;
    if let Some(text) = outcomes {
        let parsed: Outcomes =
            serde_json::from_str(text).context("parsing the mutants outcomes")?;
        for outcome in &parsed.outcomes {
            let spent = scenario_millis(outcome)?;
            if outcome.scenario.get("Mutant").is_some() {
                mutants = mutants.saturating_add(1);
                longest_mutant_ms = longest_mutant_ms.max(spent);
            } else {
                baseline_ms = baseline_ms.saturating_add(spent);
            }
        }
    }
    Ok(ShardCost {
        run,
        shard,
        packages,
        mutants,
        fixed_ms: before_ms.saturating_add(baseline_ms),
        longest_mutant_ms,
        outcome: outcome_for(code).to_owned(),
        overran,
    })
}

/// `history` with `records` added under each package they mutated: a record already held for the
/// same run and shard is replaced rather than counted twice, each package's records are ordered
/// by run, and only the [`KEPT_PER_PACKAGE`] most recent stay.
pub fn fold(mut history: History, records: &[ShardCost]) -> History {
    for record in records {
        for package in &record.packages {
            let kept = history.by_package.entry(package.clone()).or_default();
            kept.retain(|held| !(held.run == record.run && held.shard == record.shard));
            kept.push(record.clone());
        }
    }
    for kept in history.by_package.values_mut() {
        kept.sort_by(|a, b| a.run.cmp(&b.run).then_with(|| a.shard.cmp(&b.shard)));
        let excess = kept.len().saturating_sub(KEPT_PER_PACKAGE);
        kept.drain(..excess);
    }
    history
}

/// The workflow run's id, 0 outside CI.
pub fn run_id() -> u64 {
    env::var("GITHUB_RUN_ID")
        .ok()
        .and_then(|id| id.parse().ok())
        .unwrap_or(0)
}

/// Writes `cost` to [`COST_FILE`] under `root`.
pub fn write_record(root: &Path, cost: &ShardCost) -> Result<()> {
    let path = root.join(COST_FILE);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    fs::write(&path, serde_json::to_string_pretty(cost)?)
        .with_context(|| format!("writing {}", path.display()))
}

/// `gh api` with `args`, its standard output as bytes.
fn gh_api(args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("gh")
        .arg("api")
        .args(args)
        .output()
        .context("starting `gh api` (CI's runners have it)")?;
    if !output.status.success() {
        bail!(
            "`gh api {}` failed with {}: {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

/// The one file an artifact's zip holds, by artifact id, unpacked with `unzip -p`.
fn artifact_text(repo: &str, id: &str, dir: &Path) -> Result<String> {
    let zip = dir.join(format!("{id}.zip"));
    fs::write(
        &zip,
        gh_api(&[&format!("repos/{repo}/actions/artifacts/{id}/zip")])?,
    )
    .with_context(|| format!("writing {}", zip.display()))?;
    let output = Command::new("unzip")
        .arg("-p")
        .arg(&zip)
        .output()
        .context("starting `unzip` (CI's runners have it)")?;
    if !output.status.success() {
        bail!("`unzip -p {}` failed with {}", zip.display(), output.status);
    }
    String::from_utf8(output.stdout).context("an artifact that is not UTF-8")
}

/// The artifacts `listing` names, as `<id> <name>` lines, whose name starts with `prefix`.
fn ids_named(listing: &[u8], prefix: &str) -> Result<Vec<String>> {
    Ok(String::from_utf8(listing.to_vec())
        .context("a listing that is not UTF-8")?
        .lines()
        .filter_map(|line| line.split_once(' '))
        .filter(|(_, name)| name.starts_with(prefix))
        .map(|(id, _)| id.to_owned())
        .collect())
}

/// The latest unexpired `mutant-costs` history in `repo`, or an empty one when there is none.
pub fn latest_history(repo: &str, dir: &Path) -> Result<History> {
    let listing = gh_api(&[
        &format!("repos/{repo}/actions/artifacts?name={HISTORY_ARTIFACT}&per_page=10"),
        "--jq",
        ".artifacts[] | select(.expired | not) | \"\\(.id) \\(.name)\"",
    ])?;
    let Some(id) = ids_named(&listing, HISTORY_ARTIFACT)?.into_iter().next() else {
        return Ok(History::default());
    };
    serde_json::from_str(&artifact_text(repo, &id, dir)?).context("parsing the cost history")
}

/// `cargo xtask ci mutants-costs`: folds this run's shard records into the latest history and
/// writes [`HISTORY_FILE`] (DEC-2607 item 2).
pub fn ci() -> Result<()> {
    let repo = env::var("GITHUB_REPOSITORY").context("GITHUB_REPOSITORY is not set")?;
    let run = env::var("GITHUB_RUN_ID").context("GITHUB_RUN_ID is not set")?;
    let out = Path::new(HISTORY_FILE)
        .parent()
        .context("the history file has a directory")?;
    let scratch = Path::new("target/mutant-costs-download");
    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    fs::create_dir_all(scratch).with_context(|| format!("creating {}", scratch.display()))?;
    let history = latest_history(&repo, scratch)?;
    let listing = gh_api(&[
        "--paginate",
        &format!("repos/{repo}/actions/runs/{run}/artifacts?per_page=100"),
        "--jq",
        ".artifacts[] | \"\\(.id) \\(.name)\"",
    ])?;
    let mut records = Vec::new();
    for id in ids_named(&listing, COST_ARTIFACT_PREFIX)? {
        let text = artifact_text(&repo, &id, scratch)?;
        records.push(
            serde_json::from_str::<ShardCost>(&text)
                .with_context(|| format!("parsing artifact {id}"))?,
        );
    }
    let folded = fold(history, &records);
    eprintln!(
        "    mutants-costs: {} record(s) from this run; history of {} package(s)",
        records.len(),
        folded.by_package.len()
    );
    fs::write(HISTORY_FILE, serde_json::to_string_pretty(&folded)?)
        .with_context(|| format!("writing {HISTORY_FILE}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// cargo-mutants 27.1.0's `outcomes.json` for a baseline and two mutants, in its own shape.
    const OUTCOMES: &str = r#"{"outcomes": [
        {"scenario": "Baseline", "summary": "Success", "phase_results": [
            {"phase": "Build", "duration": 31.25, "process_status": "Success"},
            {"phase": "Test", "duration": 30.5, "process_status": "Success"}]},
        {"scenario": {"Mutant": {"name": "a"}}, "summary": "CaughtMutant", "phase_results": [
            {"phase": "Build", "duration": 4.0, "process_status": "Success"},
            {"phase": "Test", "duration": 22.0, "process_status": {"Failure": 101}}]},
        {"scenario": {"Mutant": {"name": "b"}}, "summary": "CaughtMutant", "phase_results": [
            {"phase": "Build", "duration": 3.0, "process_status": "Success"},
            {"phase": "Test", "duration": 1.5, "process_status": {"Failure": 101}}]}
    ], "total_mutants": 2, "caught": 2}"#;

    fn cost(run: u64, shard: &str, packages: &[&str]) -> ShardCost {
        ShardCost {
            run,
            shard: shard.to_owned(),
            packages: packages.iter().map(|p| (*p).to_owned()).collect(),
            mutants: 1,
            fixed_ms: 100_000,
            longest_mutant_ms: 5_000,
            outcome: "caught".to_owned(),
            overran: false,
        }
    }

    #[test]
    fn a_record_splits_the_fixed_part_from_the_longest_mutant() -> Result<()> {
        let got = record(
            7,
            "3/102".to_owned(),
            vec!["mandate-executor".to_owned()],
            79_000,
            Some(OUTCOMES),
            Some(0),
            false,
        )?;
        assert_eq!(got.mutants, 2);
        assert_eq!(
            got.fixed_ms,
            79_000 + 31_250 + 30_500,
            "the time before cargo-mutants and its baseline's build and tests"
        );
        assert_eq!(
            got.longest_mutant_ms, 26_000,
            "mutant `a`'s build and tests"
        );
        assert_eq!(got.outcome, "caught");
        Ok(())
    }

    #[test]
    fn each_exit_status_names_its_outcome() {
        for (code, outcome) in [
            (Some(0), "caught"),
            (Some(2), "missed"),
            (Some(3), "timeout"),
            (Some(4), "baseline-failed"),
            (Some(1), "failed"),
            (None, "failed"),
        ] {
            assert_eq!(outcome_for(code), outcome, "{code:?}");
        }
    }

    #[test]
    fn a_run_without_outcomes_counts_everything_as_fixed() -> Result<()> {
        let got = record(1, "all".to_owned(), vec![], 5_000, None, None, true)?;
        assert_eq!(
            (got.mutants, got.fixed_ms, got.longest_mutant_ms),
            (0, 5_000, 0)
        );
        assert!(got.overran);
        assert_eq!(got.outcome, "failed");
        Ok(())
    }

    #[test]
    fn a_phase_that_is_not_a_duration_is_refused() {
        let bad = OUTCOMES.replace("31.25", "-1.0");
        assert!(record(1, "all".to_owned(), vec![], 0, Some(&bad), Some(0), false).is_err());
    }

    #[test]
    fn the_fold_keeps_each_packages_latest_records_once() {
        let mut history = History::default();
        for run in 1..=25 {
            history = fold(history, &[cost(run, "0/1", &["a", "b"])]);
        }
        history = fold(history, &[cost(25, "0/1", &["a"]), cost(26, "0/2", &["b"])]);
        let runs = |p: &str| -> Vec<u64> { history.by_package[p].iter().map(|c| c.run).collect() };
        assert_eq!(
            runs("a"),
            (6..=25).collect::<Vec<_>>(),
            "the last twenty runs, run 25 once"
        );
        assert_eq!(runs("b"), (7..=26).collect::<Vec<_>>());
    }

    #[test]
    fn the_fold_orders_by_run_whatever_order_it_reads_them_in() {
        let history = fold(
            History::default(),
            &[
                cost(9, "1/2", &["a"]),
                cost(3, "0/2", &["a"]),
                cost(9, "0/2", &["a"]),
            ],
        );
        let order: Vec<(u64, &str)> = history.by_package["a"]
            .iter()
            .map(|c| (c.run, c.shard.as_str()))
            .collect();
        assert_eq!(order, [(3, "0/2"), (9, "0/2"), (9, "1/2")]);
    }

    #[test]
    fn the_listing_keeps_only_the_named_artifacts() -> Result<()> {
        let listing =
            b"11 mutant-cost-0\n12 playwright-traces\n13 mutant-cost-12\n14 mutant-costs\n";
        assert_eq!(
            ids_named(listing, COST_ARTIFACT_PREFIX)?,
            ["11", "13"],
            "the history is not a shard's record"
        );
        assert_eq!(ids_named(listing, HISTORY_ARTIFACT)?, ["14"]);
        Ok(())
    }
}
