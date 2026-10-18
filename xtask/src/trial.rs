//! `cargo xtask trial`: a pull request's branch merged into current `main` in a scratch worktree,
//! then `cargo check --workspace --all-targets` and the nextest run of the crates the merge
//! selects (DEC-1190), so a change can be checked against the `main` it will land on before it
//! lands. `trial --main <since>` checks `main` itself, selecting what changed since `<since>`.
//!
//! The worktree is `target/trial` and its builds use `target/trial-build`, both reused between
//! runs, so only what the merge changed rebuilds. The trial never shares the checkout's own
//! `target`: cargo copies each binary to `target/debug/<name>`, so a shared directory would leave
//! the checkout running `main`'s xtask and its tests running the trial's binaries. Incremental
//! compilation is off, which keeps a trial's disk use to its artifacts. The nextest run does not
//! stop at its first failure (`MANDATE_NO_FAIL_FAST`, which the worktree's own xtask reads once
//! this lands on `main`), so a trial reports every failing test at once. Run as root, as a
//! cloud container is, `mandate-marketdata`'s
//! `a_temporary_file_that_cannot_be_created_is_an_error_not_a_retry` fails by design (it needs a
//! user that file permissions apply to); every other failure is the merge's.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::NO_FAIL_FAST_ENV;

/// Where the trial worktree is, under the checkout.
const WORKTREE: &str = "target/trial";

/// The trial's own target directory, under the checkout.
const TARGET: &str = "target/trial-build";

/// What a trial checks: a branch merged into `main`, or `main` itself since a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Trial {
    /// `origin/<branch>` merged into `origin/main`; the selection base is `main`'s tip.
    Merge(String),
    /// `origin/main` as it is; the selection base is the commit given.
    Main(String),
}

impl Trial {
    /// Reads `trial <branch>` or `trial --main <since>`.
    pub(crate) fn parse(args: &[&str]) -> Result<Self> {
        match args {
            ["--main", since] => Ok(Self::Main((*since).to_owned())),
            [branch] if !branch.starts_with('-') => Ok(Self::Merge((*branch).to_owned())),
            _ => bail!("usage: cargo xtask trial <branch> | trial --main <since>"),
        }
    }

    /// The refs to fetch from `origin` before the worktree is made.
    pub(crate) fn fetched(&self) -> Vec<String> {
        match self {
            Self::Merge(branch) => vec!["main".to_owned(), branch.clone()],
            Self::Main(_) => vec!["main".to_owned()],
        }
    }

    /// The commit the selection compares the trial against, given `main`'s tip.
    pub(crate) fn base(&self, main: &str) -> String {
        match self {
            Self::Merge(_) => main.to_owned(),
            Self::Main(since) => since.clone(),
        }
    }
}

/// One cargo invocation of a trial: its arguments and the variables it adds.
pub(crate) type Check = (Vec<&'static str>, Vec<(&'static str, String)>);

/// The two checks a trial runs in its worktree, in order: the whole workspace's every target
/// compiles, then the selected crates' nextest run against `base`, reporting every failure.
pub(crate) fn checks(base: &str) -> [Check; 2] {
    [
        (
            vec!["check", "--workspace", "--all-targets", "--locked"],
            Vec::new(),
        ),
        (
            vec!["xtask", "ci", "test-nextest"],
            vec![
                ("MANDATE_BASE_REF", base.to_owned()),
                (NO_FAIL_FAST_ENV, "1".to_owned()),
            ],
        ),
    ]
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    eprintln!("    $ git {}", args.join(" "));
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .context("starting git")?;
    if !output.status.success() {
        bail!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Runs `trial` from the checkout at `root`.
pub(crate) fn run(root: &Path, trial: &Trial) -> Result<()> {
    for reference in trial.fetched() {
        git(root, &["fetch", "--quiet", "origin", &reference])?;
    }
    let main = git(root, &["rev-parse", "origin/main"])?;
    let dir: PathBuf = root.join(WORKTREE);
    if dir.exists() {
        git(
            root,
            &["worktree", "remove", "--force", &dir.display().to_string()],
        )?;
    }
    git(
        root,
        &[
            "worktree",
            "add",
            "--detach",
            &dir.display().to_string(),
            &main,
        ],
    )?;
    if let Trial::Merge(branch) = trial {
        let merged = git(
            &dir,
            &[
                "-c",
                "user.name=xtask trial",
                "-c",
                "user.email=trial@invalid",
                "merge",
                "--no-ff",
                "--no-edit",
                &format!("origin/{branch}"),
            ],
        );
        if let Err(err) = merged {
            let _ = git(&dir, &["merge", "--abort"]);
            bail!("{branch} does not merge cleanly into main {main}: {err:#}");
        }
    }
    let base = trial.base(&main);
    let target = root.join(TARGET);
    for (args, vars) in checks(&base) {
        eprintln!("    $ cargo {}", args.join(" "));
        let status = Command::new("cargo")
            .current_dir(&dir)
            .args(&args)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_INCREMENTAL", "0")
            .envs(vars.iter().map(|(name, value)| (*name, value.as_str())))
            .status()
            .context("starting cargo")?;
        if !status.success() {
            bail!("trial: `cargo {}` failed with {status}", args.join(" "));
        }
    }
    eprintln!("    trial: passed against main {main}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trial_is_a_branch_or_main_since_a_commit() -> Result<()> {
        assert_eq!(
            Trial::parse(&["claude/x"])?,
            Trial::Merge("claude/x".to_owned())
        );
        assert_eq!(
            Trial::parse(&["--main", "abc123"])?,
            Trial::Main("abc123".to_owned())
        );
        for args in [&[][..], &["--main"], &["--x"], &["a", "b"]] {
            assert!(Trial::parse(args).is_err(), "{args:?}");
        }
        Ok(())
    }

    #[test]
    fn a_merge_selects_against_main_and_main_against_its_since() {
        let merge = Trial::Merge("b".to_owned());
        assert_eq!(merge.fetched(), ["main", "b"]);
        assert_eq!(merge.base("m"), "m");
        let main = Trial::Main("s".to_owned());
        assert_eq!(main.fetched(), ["main"]);
        assert_eq!(main.base("m"), "s");
    }

    #[test]
    fn the_checks_compile_everything_then_test_the_selection() {
        let [(check, check_vars), (test, test_vars)] = checks("base");
        assert_eq!(check, ["check", "--workspace", "--all-targets", "--locked"]);
        assert!(check_vars.is_empty());
        assert_eq!(test, ["xtask", "ci", "test-nextest"]);
        assert_eq!(
            test_vars,
            [
                ("MANDATE_BASE_REF", "base".to_owned()),
                (NO_FAIL_FAST_ENV, "1".to_owned())
            ]
        );
    }
}
