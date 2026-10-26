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

/// The milliseconds after the gate's start by which a shard of more than one mutant must be done
/// (DEC-2607 item 5): with the job's setup before xtask, inside DEC-464's ten minutes.
pub const STOP_AFTER_MS: u64 = 480_000;

/// The most mutants a shard carries (DEC-2607 item 3).
pub const MAX_PER_SHARD: usize = 16;

/// The fewest clean records a package needs before its shards carry more than one mutant.
pub const MIN_RECORDS: usize = 3;

/// The factor on a package's longest mutant when sizing its shards.
const MUTANT_MARGIN: u64 = 2;

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
    let fixed_ms = before_ms.saturating_add(baseline_ms);
    let longest_mutant_ms = if overran {
        let run_out = STOP_AFTER_MS.saturating_sub(fixed_ms) / u64::try_from(mutants.max(1))?;
        longest_mutant_ms.max(run_out)
    } else {
        longest_mutant_ms
    };
    Ok(ShardCost {
        run,
        shard,
        packages,
        mutants,
        fixed_ms,
        longest_mutant_ms,
        outcome: outcome_for(code).to_owned(),
        overran,
    })
}

/// The outcome of a record written before cargo-mutants starts, which only a shard that never
/// wrote its own record leaves behind (DEC-2367 item 2).
pub const UNFINISHED: &str = "unfinished";

/// The record a shard of `per_shard` mutants writes before cargo-mutants starts, which its real
/// record replaces (DEC-2367 item 2): it counts as having run out of time, so a shard that GitHub
/// stops at the job's limit, before xtask can write its record, still sizes its packages' next
/// plans at one mutant. `None` for a shard of one, which no stop times and no plan sizes smaller.
pub fn provisional(
    run: u64,
    shard: String,
    packages: Vec<String>,
    per_shard: usize,
) -> Option<ShardCost> {
    (per_shard > 1).then(|| ShardCost {
        run,
        shard,
        packages,
        mutants: 0,
        fixed_ms: 0,
        longest_mutant_ms: 0,
        outcome: UNFINISHED.to_owned(),
        overran: true,
    })
}

/// The histories `histories` folded into one (DEC-2367 item 1): each `mutants-costs` job folds its
/// run into the latest history it can read, so two that overlap each upload a history without the
/// other's records. Reading the recent ones together keeps both, and [`fold`] counts a record
/// held by several only once.
pub fn merged(histories: impl IntoIterator<Item = History>) -> History {
    histories
        .into_iter()
        .fold(History::default(), |kept, history| {
            let records: Vec<ShardCost> = history.by_package.into_values().flatten().collect();
            fold(kept, &records)
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

/// How many mutants a shard of one package may carry, from its kept records (DEC-2607 item 3):
/// `floor((STOP_AFTER_MS - F) / (MUTANT_MARGIN * M))` between 1 and [`MAX_PER_SHARD`], `F` its
/// largest fixed part and `M` its longest mutant. One while it has fewer than [`MIN_RECORDS`]
/// records, or any record that ran out of time or ended other than `caught` or `missed`.
fn package_per_shard(records: &[ShardCost]) -> usize {
    let clean = |record: &ShardCost| {
        !record.overran && matches!(record.outcome.as_str(), "caught" | "missed")
    };
    if records.len() < MIN_RECORDS || !records.iter().all(clean) {
        return 1;
    }
    let fixed = records
        .iter()
        .map(|r| r.fixed_ms)
        .max()
        .unwrap_or(STOP_AFTER_MS);
    let longest = records
        .iter()
        .map(|r| r.longest_mutant_ms)
        .max()
        .unwrap_or(0)
        .max(1);
    let fits = STOP_AFTER_MS.saturating_sub(fixed) / longest.saturating_mul(MUTANT_MARGIN);
    usize::try_from(fits)
        .unwrap_or(MAX_PER_SHARD)
        .clamp(1, MAX_PER_SHARD)
}

/// How many mutants a shard of a diff mutating `packages` may carry: the smallest of theirs, and
/// one for a diff with none.
pub fn per_shard<'p>(history: &History, packages: impl IntoIterator<Item = &'p String>) -> usize {
    packages
        .into_iter()
        .map(|package| {
            package_per_shard(
                history
                    .by_package
                    .get(package)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            )
        })
        .min()
        .unwrap_or(1)
}

/// The history the plan reads in CI, or an empty one, which sizes every shard at one mutant: outside
/// CI, or when the history cannot be read, as said in the log.
pub fn plan_history() -> History {
    let Ok(repo) = env::var("GITHUB_REPOSITORY") else {
        return History::default();
    };
    let scratch = Path::new("target/mutant-costs-download");
    let read = fs::create_dir_all(scratch)
        .context("creating the download directory")
        .and_then(|()| latest_history(&repo, scratch));
    read.unwrap_or_else(|err| {
        eprintln!(
            "    mutants: no cost history ({err:#}); every shard carries one mutant (DEC-2607)"
        );
        History::default()
    })
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

/// How many of the most recent `mutant-costs` histories are read together (DEC-2367 item 1).
pub const HISTORIES_READ: usize = 10;

/// The [`HISTORIES_READ`] latest unexpired `mutant-costs` histories in `repo`, [`merged`], or an
/// empty one when there is none. One that cannot be read is skipped, as said in the log
/// ([`readable`]), so the history alone never fails a job; only a listing that fails does.
pub fn latest_history(repo: &str, dir: &Path) -> Result<History> {
    let listing = gh_api(&[
        &format!(
            "repos/{repo}/actions/artifacts?name={HISTORY_ARTIFACT}&per_page={HISTORIES_READ}"
        ),
        "--jq",
        ".artifacts[] | select(.expired | not) | \"\\(.id) \\(.name)\"",
    ])?;
    let read = ids_named(&listing, HISTORY_ARTIFACT)?
        .into_iter()
        .take(HISTORIES_READ)
        .map(|id| {
            let history = artifact_text(repo, &id, dir).and_then(|text| {
                serde_json::from_str::<History>(&text).context("parsing the cost history")
            });
            (id, history)
        });
    Ok(merged(readable(read)))
}

/// The histories of `read` that could be read, by artifact id; each that could not is said and
/// skipped (DEC-2367 item 1).
fn readable(read: impl IntoIterator<Item = (String, Result<History>)>) -> Vec<History> {
    read.into_iter()
        .filter_map(|(id, history)| {
            history
                .map_err(|err| {
                    eprintln!("    mutants: skipping cost history {id} ({err:#}) (DEC-2367)");
                })
                .ok()
        })
        .collect()
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
            (0, 5_000, 475_000),
            "having run out of time with no mutant done, its one mutant took what the stop left"
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

    fn sized(fixed_ms: u64, longest_mutant_ms: u64) -> ShardCost {
        ShardCost {
            fixed_ms,
            longest_mutant_ms,
            ..cost(1, "0/1", &["a"])
        }
    }

    #[test]
    fn a_package_is_sized_from_its_largest_fixed_part_and_longest_mutant() {
        let records = [
            sized(100_000, 5_000),
            sized(140_000, 2_000),
            sized(90_000, 11_000),
        ];
        assert_eq!(
            package_per_shard(&records),
            15,
            "(480 - 140) s over twice 11 s is 15.45"
        );
        let executor = [
            sized(140_000, 26_000),
            sized(120_000, 20_000),
            sized(130_000, 1_000),
        ];
        assert_eq!(package_per_shard(&executor), 6, "(480 - 140) / 52 is 6.5");
        let quick = [sized(60_000, 100), sized(60_000, 100), sized(60_000, 100)];
        assert_eq!(
            package_per_shard(&quick),
            MAX_PER_SHARD,
            "never more than sixteen"
        );
        let full = [sized(470_000, 9_000), sized(10_000, 1), sized(10_000, 1)];
        assert_eq!(package_per_shard(&full), 1, "never fewer than one");
        let over = [sized(480_001, 1), sized(1, 1), sized(1, 1)];
        assert_eq!(
            package_per_shard(&over),
            1,
            "a fixed part past the stop leaves one"
        );
    }

    #[test]
    fn a_package_without_a_clean_history_gets_one_mutant_a_shard() {
        assert_eq!(package_per_shard(&[]), 1, "no history");
        assert_eq!(
            package_per_shard(&[sized(1_000, 1_000), sized(1_000, 1_000)]),
            1,
            "fewer than three records"
        );
        for spoil in ["timeout", "baseline-failed", "failed"] {
            let mut records = vec![sized(1_000, 1_000); 5];
            records[2].outcome = spoil.to_owned();
            assert_eq!(
                package_per_shard(&records),
                1,
                "a record that ended {spoil}"
            );
        }
        let mut records = vec![sized(1_000, 1_000); 5];
        records[4].overran = true;
        assert_eq!(
            package_per_shard(&records),
            1,
            "a record that ran out of time"
        );
        let mut missed = vec![sized(1_000, 1_000); 5];
        missed[0].outcome = "missed".to_owned();
        assert_eq!(
            package_per_shard(&missed),
            MAX_PER_SHARD,
            "a missed mutant is a cost like any other; the gate judges it, not the plan"
        );
    }

    #[test]
    fn a_diff_takes_the_smallest_of_its_packages() {
        let mut history = History::default();
        history
            .by_package
            .insert("quick".to_owned(), vec![sized(60_000, 100); 3]);
        history
            .by_package
            .insert("slow".to_owned(), vec![sized(140_000, 26_000); 3]);
        let names =
            |list: &[&str]| -> Vec<String> { list.iter().map(|p| (*p).to_owned()).collect() };
        assert_eq!(per_shard(&history, &names(&["quick"])), MAX_PER_SHARD);
        assert_eq!(per_shard(&history, &names(&["quick", "slow"])), 6);
        assert_eq!(
            per_shard(&history, &names(&["quick", "new"])),
            1,
            "a package with no records"
        );
        assert_eq!(per_shard(&history, &names(&[])), 1);
    }

    /// DEC-2367 item 1, the repeat seen on #2416: a shard of five ran out of time, but another
    /// pull request's `mutants-costs` job, which read the history before that run was folded in,
    /// uploaded last. The plan read only that newest history, so it sized the package at five
    /// again; read together, the recent histories size it at one.
    #[test]
    fn a_timed_out_shard_sizes_its_package_at_one_whatever_history_was_uploaded_last() {
        let base = fold(
            History::default(),
            &[
                sized(75_000, 30_000),
                ShardCost {
                    run: 2,
                    ..sized(75_000, 30_000)
                },
                ShardCost {
                    run: 3,
                    ..sized(75_000, 30_000)
                },
            ],
        );
        let packages = ["a".to_owned()];
        assert!(per_shard(&base, &packages) > 1, "a clean history sizes up");
        let ran_out = ShardCost {
            run: 4,
            shard: "0/2".to_owned(),
            overran: true,
            outcome: "failed".to_owned(),
            ..sized(75_000, 30_000)
        };
        let theirs = ShardCost {
            run: 5,
            ..sized(75_000, 30_000)
        };
        let with_timeout = fold(base.clone(), &[ran_out]);
        let uploaded_last = fold(base, &[theirs]);
        assert!(
            per_shard(&uploaded_last, &packages) > 1,
            "the newest history alone repeats the plan that ran out of time"
        );
        assert_eq!(
            per_shard(
                &merged([uploaded_last.clone(), with_timeout.clone()]),
                &packages
            ),
            1
        );
        assert_eq!(
            per_shard(&merged([with_timeout, uploaded_last]), &packages),
            1,
            "whichever was uploaded last"
        );
    }

    #[test]
    fn a_history_that_cannot_be_read_is_skipped() {
        let kept = fold(History::default(), &[cost(1, "0/1", &["a"])]);
        let read = readable([
            ("1".to_owned(), Err(anyhow::anyhow!("expired"))),
            ("2".to_owned(), Ok(kept.clone())),
            ("3".to_owned(), Err(anyhow::anyhow!("not JSON"))),
        ]);
        assert_eq!(read, [kept]);
    }

    #[test]
    fn histories_read_together_hold_each_record_once() {
        let one = fold(
            History::default(),
            &[cost(1, "0/1", &["a", "b"]), cost(2, "0/1", &["a"])],
        );
        let two = fold(
            History::default(),
            &[cost(2, "0/1", &["a"]), cost(3, "0/1", &["b"])],
        );
        let both = merged([one, two]);
        let runs = |p: &str| -> Vec<u64> { both.by_package[p].iter().map(|c| c.run).collect() };
        assert_eq!(runs("a"), [1, 2], "run 2's record once");
        assert_eq!(runs("b"), [1, 3]);
        assert_eq!(merged([]), History::default());
    }

    /// DEC-2367 item 2: a shard GitHub stops at the job's limit, before xtask writes its record,
    /// leaves the provisional one, which sizes its package at one; a shard of one leaves none.
    #[test]
    fn a_shard_stopped_before_its_record_sizes_its_package_at_one() {
        let left = provisional(9, "0/2".to_owned(), vec!["a".to_owned()], 5).expect("a record");
        assert!(left.overran);
        assert_eq!(left.outcome, UNFINISHED);
        assert_eq!((left.run, left.shard.as_str(), left.mutants), (9, "0/2", 0));
        let clean: Vec<ShardCost> = (1..=5)
            .map(|run| ShardCost {
                run,
                ..sized(1_000, 1_000)
            })
            .collect();
        let history = fold(History::default(), &clean);
        let packages = ["a".to_owned()];
        assert_eq!(per_shard(&history, &packages), MAX_PER_SHARD);
        assert_eq!(per_shard(&fold(history, &[left]), &packages), 1);
        assert_eq!(
            provisional(9, "0/9".to_owned(), vec!["a".to_owned()], 1),
            None
        );
        assert!(provisional(9, "0/9".to_owned(), vec![], 2).is_some());
    }

    /// DEC-2367 item 3: a shard that ran out of time counts each mutant it ran at no less than its
    /// share of what the stop left after the fixed part.
    #[test]
    fn a_shard_that_ran_out_of_time_costs_at_least_its_share_of_the_budget() -> Result<()> {
        let got = record(
            7,
            "0/2".to_owned(),
            vec![],
            79_000,
            Some(OUTCOMES),
            Some(124),
            true,
        )?;
        assert_eq!(got.fixed_ms, 140_750);
        assert_eq!(got.longest_mutant_ms, (480_000 - 140_750) / 2);
        let none = record(7, "0/2".to_owned(), vec![], 400_000, None, Some(124), true)?;
        assert_eq!(
            none.longest_mutant_ms, 80_000,
            "no mutant ran: the whole remainder"
        );
        let past = record(7, "0/2".to_owned(), vec![], 500_000, None, Some(124), true)?;
        assert_eq!(
            past.longest_mutant_ms, 0,
            "a fixed part past the stop leaves nothing to share"
        );
        let on_time = record(
            7,
            "0/2".to_owned(),
            vec![],
            79_000,
            Some(OUTCOMES),
            Some(0),
            false,
        )?;
        assert_eq!(
            on_time.longest_mutant_ms, 26_000,
            "a shard on time keeps its measure"
        );
        Ok(())
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
