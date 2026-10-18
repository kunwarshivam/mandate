//! `cargo xtask trial`: a pull request, by number or by branch, merged into current `main` in a
//! scratch worktree,
//! then `cargo check --workspace --all-targets` and the nextest run of the crates the merge
//! selects (DEC-1190), so a change can be checked against the `main` it will land on before it
//! lands. `trial --main <since>` checks `main` itself, selecting what changed since `<since>`.
//!
//! The worktree is `target/trial` and its builds use `target/trial-build`, both reused between
//! runs. The worktree is reset and checked out at `main`'s tip rather than made anew, so files that
//! did not change keep their timestamps and only what the merge changed rebuilds.
//!
//! The trial never shares the checkout's own `target`: cargo copies each binary to
//! `target/debug/<name>`, so a shared directory would leave the checkout running `main`'s xtask
//! and its tests running the trial's binaries. Incremental compilation is off, which keeps a
//! trial's disk use to its artifacts.
//!
//! The nextest run does not stop at its first failure (`MANDATE_NO_FAIL_FAST`, which the
//! worktree's own xtask reads once this lands on `main`), so a trial reports every failing test at
//! once. Run as root, as a cloud container is, `mandate-marketdata`'s
//! `a_temporary_file_that_cannot_be_created_is_an_error_not_a_retry` fails by design (it needs a
//! user that file permissions apply to); every other failure is the merge's.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::NO_FAIL_FAST_ENV;

/// Where the trial worktree is, under the checkout.
const WORKTREE: &str = "target/trial";

/// The trial's own target directory, under the checkout.
const TARGET: &str = "target/trial-build";

/// What a trial merges into `main`: a pull request by number, fetched from `origin`'s
/// `refs/pull/<n>/head`, so a pull request is tried without its branch's name (a fork's included),
/// or a branch by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    PullRequest(u64),
    Branch(String),
}

impl Change {
    /// Digits name a pull request (no leading zero); anything else must be a plain branch name,
    /// which cannot reach git as an option, a revision expression or a range.
    pub(crate) fn parse(argument: &str) -> Result<Self> {
        if !argument.is_empty() && argument.bytes().all(|b| b.is_ascii_digit()) {
            return match argument.parse::<u64>() {
                Ok(number) if number > 0 && !argument.starts_with('0') => {
                    Ok(Self::PullRequest(number))
                }
                _ => bail!("`{argument}` is not a pull request number"),
            };
        }
        let plain = argument
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b));
        if argument.is_empty()
            || !plain
            || argument.starts_with(['-', '/', '.'])
            || argument.ends_with(['/', '.'])
            || argument.contains("..")
            || argument.contains("//")
            || argument.ends_with(".lock")
        {
            bail!("`{argument}` is neither a pull request number nor a plain branch name");
        }
        Ok(Self::Branch(argument.to_owned()))
    }

    /// What `git fetch origin` takes to bring the change into `origin/...`.
    fn refspec(&self) -> String {
        match self {
            Self::PullRequest(number) => {
                format!("+refs/pull/{number}/head:refs/remotes/origin/pr/{number}")
            }
            Self::Branch(name) => name.clone(),
        }
    }

    /// The remote-tracking ref the fetch leaves the change at, which the trial merges.
    fn merged(&self) -> String {
        match self {
            Self::PullRequest(number) => format!("origin/pr/{number}"),
            Self::Branch(name) => format!("origin/{name}"),
        }
    }
}

/// What a trial checks: a change merged into `main`, or `main` itself since a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Trial {
    /// The change merged into `origin/main`; the selection base is `main`'s tip.
    Merge(Change),
    /// `origin/main` as it is; the selection base is the commit given.
    Main(String),
}

impl Trial {
    /// Reads `trial <pull request number | branch>` or `trial --main <since>`, where `<since>` is
    /// a commit's hex name, so neither reaches git as an option or an expression.
    pub(crate) fn parse(args: &[&str]) -> Result<Self> {
        match args {
            ["--main", since]
                if (4..=40).contains(&since.len())
                    && since.bytes().all(|b| b.is_ascii_hexdigit()) =>
            {
                Ok(Self::Main((*since).to_owned()))
            }
            [change] => Ok(Self::Merge(Change::parse(change)?)),
            _ => bail!(
                "usage: cargo xtask trial <pull request number | branch> | trial --main <commit>"
            ),
        }
    }

    /// The refspecs to fetch from `origin` before the worktree is made.
    pub(crate) fn fetched(&self) -> Vec<String> {
        match self {
            Self::Merge(change) => vec!["main".to_owned(), change.refspec()],
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
    let (dir, main) = prepare(root, trial)?;
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

/// Fetches what `trial` needs from `origin`, puts the worktree at `main`'s tip and, for a merge,
/// merges the change there: the worktree and `main`'s tip, or the conflicting paths.
fn prepare(root: &Path, trial: &Trial) -> Result<(PathBuf, String)> {
    for reference in trial.fetched() {
        git(root, &["fetch", "--quiet", "origin", &reference])?;
    }
    let main = git(root, &["rev-parse", "origin/main"])?;
    let dir: PathBuf = root.join(WORKTREE);
    let reused = dir.join(".git").exists() && {
        let _ = git(&dir, &["merge", "--abort"]);
        git(&dir, &["reset", "--quiet", "--hard"])
            .and_then(|_| git(&dir, &["clean", "--quiet", "-fdx"]))
            .and_then(|_| git(&dir, &["checkout", "--quiet", "--detach", &main]))
            .is_ok()
    };
    if !reused {
        let _ = git(
            root,
            &["worktree", "remove", "--force", &dir.display().to_string()],
        );
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .with_context(|| format!("removing the stale trial worktree {}", dir.display()))?;
        }
        git(root, &["worktree", "prune"])?;
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
    }
    if let Trial::Merge(change) = trial {
        let merged = git(
            &dir,
            &[
                "-c",
                "user.name=xtask trial",
                "-c",
                "user.email=trial@invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "merge",
                "--no-ff",
                "--no-edit",
                &change.merged(),
            ],
        );
        if merged.is_err() {
            let paths = git(&dir, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
            let _ = git(&dir, &["merge", "--abort"]);
            bail!(
                "{} does not merge cleanly into main {main}; conflicts in: {}",
                change.merged(),
                paths.lines().collect::<Vec<_>>().join(", ")
            );
        }
    }
    Ok((dir, main))
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn a_trial_is_a_branch_or_main_since_a_commit() -> Result<()> {
        assert_eq!(
            Trial::parse(&["claude/x"])?,
            Trial::Merge(Change::Branch("claude/x".to_owned()))
        );
        assert_eq!(
            Trial::parse(&["1714"])?,
            Trial::Merge(Change::PullRequest(1714))
        );
        assert_eq!(
            Trial::parse(&["--main", "abc123"])?,
            Trial::Main("abc123".to_owned())
        );
        for args in [
            &[][..],
            &["--main"],
            &["--x"],
            &["a", "b"],
            &["--main", "main~3"],
            &["--main", "--all"],
            &["--main", "abc"],
        ] {
            assert!(Trial::parse(args).is_err(), "{args:?}");
        }
        Ok(())
    }

    #[test]
    fn a_change_is_a_pull_request_number_or_a_plain_branch_name() -> Result<()> {
        assert_eq!(Change::parse("1714")?.merged(), "origin/pr/1714");
        assert_eq!(
            Change::parse("1714")?.refspec(),
            "+refs/pull/1714/head:refs/remotes/origin/pr/1714"
        );
        assert_eq!(Change::parse("a/b-c_1.2")?.merged(), "origin/a/b-c_1.2");
        for bad in [
            "",
            "0",
            "012",
            "-x",
            "--upload-pack=x",
            "a..b",
            "main~1",
            "a b",
            "/a",
            "a/",
            ".a",
            "a.",
            "a//b",
            "a.lock",
            "a:b",
            "a^",
            "@{1}",
            "+a",
        ] {
            assert!(Change::parse(bad).is_err(), "{bad:?} is refused");
        }
        Ok(())
    }

    /// An `origin` with `main`, a branch `clean` that adds a file, a branch `clash` that changes
    /// the line `main` changed after both branched, and pull request 7's head at `clean`; and a
    /// clone of it to run trials from.
    struct Origin {
        origin: PathBuf,
        checkout: PathBuf,
    }

    impl Origin {
        fn new(name: &str) -> Result<Self> {
            let top = env::temp_dir().join(format!("mandate-trial-{name}-{}", std::process::id()));
            if top.exists() {
                fs::remove_dir_all(&top)?;
            }
            let origin = top.join("origin");
            fs::create_dir_all(&origin)?;
            let commit = |path: &str, text: &str, message: &str| -> Result<()> {
                fs::write(origin.join(path), text)?;
                git(&origin, &["add", path])?;
                git(
                    &origin,
                    &[
                        "-c",
                        "user.name=t",
                        "-c",
                        "user.email=t@invalid",
                        "-c",
                        "commit.gpgsign=false",
                        "commit",
                        "--quiet",
                        "-m",
                        message,
                    ],
                )?;
                Ok(())
            };
            git(&origin, &["init", "--quiet", "--initial-branch=main"])?;
            commit("shared.txt", "one\n", "base")?;
            git(&origin, &["branch", "clean"])?;
            git(&origin, &["branch", "clash"])?;
            commit("shared.txt", "two\n", "main moves")?;
            git(&origin, &["checkout", "--quiet", "clean"])?;
            commit("added.txt", "new\n", "clean change")?;
            git(&origin, &["update-ref", "refs/pull/7/head", "clean"])?;
            git(&origin, &["checkout", "--quiet", "clash"])?;
            commit("shared.txt", "three\n", "clashing change")?;
            git(&origin, &["checkout", "--quiet", "main"])?;
            let checkout = top.join("checkout");
            git(
                &top,
                &[
                    "clone",
                    "--quiet",
                    &origin.display().to_string(),
                    "checkout",
                ],
            )?;
            Ok(Self { origin, checkout })
        }
    }

    impl Drop for Origin {
        fn drop(&mut self) {
            if let Some(top) = self.origin.parent() {
                fs::remove_dir_all(top).ok();
            }
        }
    }

    #[test]
    fn a_pull_request_or_branch_merges_onto_mains_tip() -> Result<()> {
        let fx = Origin::new("clean")?;
        let main = git(&fx.origin, &["rev-parse", "main"])?;
        let head = git(&fx.origin, &["rev-parse", "clean"])?;
        for argument in ["7", "clean"] {
            let (dir, at) = prepare(&fx.checkout, &Trial::parse(&[argument])?)?;
            assert_eq!(at, main, "{argument}: main's tip");
            assert_eq!(
                git(&dir, &["rev-parse", "HEAD^1", "HEAD^2"])?,
                format!("{main}\n{head}"),
                "{argument}: the change merged onto main's tip"
            );
            assert_eq!(fs::read_to_string(dir.join("shared.txt"))?, "two\n");
            assert_eq!(fs::read_to_string(dir.join("added.txt"))?, "new\n");
        }
        Ok(())
    }

    #[test]
    fn a_conflicting_change_names_its_paths_and_leaves_the_worktree_reusable() -> Result<()> {
        let fx = Origin::new("clash")?;
        let err = prepare(&fx.checkout, &Trial::parse(&["clash"])?)
            .err()
            .context("a change to the line main changed conflicts")?;
        assert!(
            err.to_string().contains("conflicts in: shared.txt"),
            "{err:#}"
        );
        let (dir, _) = prepare(&fx.checkout, &Trial::parse(&["clean"])?)?;
        assert_eq!(
            fs::read_to_string(dir.join("added.txt"))?,
            "new\n",
            "the next trial reuses the worktree the conflict left"
        );
        Ok(())
    }

    #[test]
    fn a_worktree_left_by_a_killed_trial_is_replaced() -> Result<()> {
        let fx = Origin::new("stale")?;
        let (dir, _) = prepare(&fx.checkout, &Trial::parse(&["clean"])?)?;
        fs::remove_file(dir.join(".git"))?;
        fs::write(dir.join("left-over.txt"), "x")?;
        let (again, _) = prepare(&fx.checkout, &Trial::parse(&["7"])?)?;
        assert_eq!(again, dir);
        assert!(!again.join("left-over.txt").exists(), "a fresh worktree");
        assert_eq!(fs::read_to_string(again.join("added.txt"))?, "new\n");
        Ok(())
    }

    #[test]
    fn a_change_origin_does_not_hold_fails_at_the_fetch() -> Result<()> {
        let fx = Origin::new("missing")?;
        assert!(prepare(&fx.checkout, &Trial::parse(&["nope"])?).is_err());
        assert!(prepare(&fx.checkout, &Trial::parse(&["8"])?).is_err());
        Ok(())
    }

    #[test]
    fn a_merge_selects_against_main_and_main_against_its_since() {
        let merge = Trial::Merge(Change::Branch("b".to_owned()));
        assert_eq!(merge.fetched(), ["main", "b"]);
        assert_eq!(
            Trial::Merge(Change::PullRequest(3)).fetched(),
            ["main", "+refs/pull/3/head:refs/remotes/origin/pr/3"]
        );
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
