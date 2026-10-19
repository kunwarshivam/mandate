//! `cargo xtask leak-guard`, part of `lint` (DEC-2032): the repository is public, so a CI log or
//! artifact is too. Each rule is a text match over `.github/workflows` and `.github/scripts` that
//! refuses the shapes its rules name, and a line it refuses stays refused until a person lists it,
//! file and trimmed line byte for byte, in [`ALLOWED`], in a change a reviewer sees. It is defence
//! in depth: a text match is never complete, so the closed world is L9, a secret step that runs
//! only a prebuilt command, together with no secret in a pull request's jobs.
//!
//! - **L1, environment dumps:** `printenv`, `declare`, `typeset`, a bare `export` or `export -p`,
//!   a bare `set`, an `env` with no command (only options, assignments or a redirect),
//!   `compgen -e` or `-v`, and any `/proc/*/environ`.
//! - **L2, trace and debug output:** `set` with any flag holding `x` or `v`, or `-o xtrace` or
//!   `-o verbose`; a shell run with any of those or `--xtrace`/`--verbose`, its `-c` program read as
//!   a command of its own; a shebang with `-x` or `-v`; `PS4=`, `SHELLOPTS`, `BASH_XTRACEFD`,
//!   `BASH_ENV`, the runner's and `gh`'s debug switches, `TF_LOG`, `RUST_LOG` at `trace`,
//!   `helm --debug`, `kubectl` above `--v=5`, `doctl -v` and `cosign -d`.
//! - **L3, contexts:** read in every GitHub expression whole (each `${{ … }}` joined across lines,
//!   a quoted `}}` not ending it, and each `if:` value) and without regard to case: `toJSON(` whose
//!   argument names a context, however nested; the whole `secrets` context anywhere; `secrets[`;
//!   and `secrets.` anywhere but an `env:` block, so a secret reaches a step only as a variable and
//!   is never pasted into its script; and `secrets: inherit` in any case and spacing.
//! - **L5, artifacts:** each `upload-artifact` or `upload-pages-artifact` step names its paths,
//!   in any style and before or after its `uses:`, and each is one of [`ARTIFACT_PATHS`]; none names
//!   deploy state, a plan, a key, a secret, a token, a kubeconfig, an `.env` or auth; none uploads
//!   hidden files; and nothing runs `gh release upload`.
//! - **L6, caches:** no cache is restored or saved in a secret job (one with an `environment:`, a
//!   `secrets.` or `secrets[` read, `id-token: write`, or a writable `github.token`), by any caching
//!   action or input, so nothing a pull request wrote runs beside a secret.
//! - **L7, curl and tools:** no verbose, traced or header-dumping `curl`, no `-H` or `-u` value but
//!   an `@file`, each read inside combined short options and with attached values, no
//!   `--aws-sigv4`, no `docker login -p`, and no `--password` but `--password-stdin`.
//! - **L8, runner files:** a secret job, and a script it runs, writes no `GITHUB_ENV`,
//!   `GITHUB_PATH`, `GITHUB_OUTPUT`, `GITHUB_STEP_SUMMARY` or `GITHUB_STATE` and issues no `::`
//!   workflow command, in any case and after an escaped line break too; and nothing anywhere issues
//!   `::add-mask::`, in any case, but a reviewed line that escapes its value first (`%` to `%25`,
//!   then CR to `%0D` and LF to `%0A`).
//! - **L9, the closed world:** a step a secret reaches, from the step, its job or its workflow,
//!   and every step after it in its job, runs only one reviewed line of [`SECRET_RUNS`] or one of
//!   xtask's pinned `READ_ACTIONS`, sets no `shell:`, and a secret job runs no local action; and
//!   every `uses:` anywhere is pinned to a 40-character commit.
//! - **L12, triggers:** `pull_request_target` and `workflow_run` only in `merge.yml`, which names no
//!   `head.sha`, `head.ref` or `head_ref`; no trigger anyone can raise (a comment, an issue, a
//!   review, a discussion, a dispatch); a workflow holding an `environment:` triggers only on
//!   `workflow_dispatch` or a push to exactly `main`, and each job with one has an `if:` that
//!   holds `github.ref == 'refs/heads/main'` with no `||`, `!=` or negation.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// The directories the guard reads, under the repository root.
const GUARDED_DIRS: [&str; 2] = [".github/workflows", ".github/scripts"];

/// The lines a person has read and found to leak nothing, by rule, file, and trimmed line, matched
/// byte for byte:
/// - `no-credentials.sh` lists the environment's names, never its values, and prints only the
///   names that look like a credential's (DEC-964).
/// - `release.yml`'s build jobs upload `images/test-key/cosign.pub`, the public half of an
///   ephemeral test key made for the run's drill (E21-35), which signs nothing that ships.
/// - `merge.yml`'s `RUN_PRS` is the JSON list of the pull request numbers a `workflow_run` names,
///   which the event holds in public.
/// - `merge.yml`'s merge job, whose `github.token` may write, writes the fixed `merged=true` as
///   its output, so `ci-on-main` knows to dispatch `ci`.
/// - `release.yml`'s two `publish` jobs, which hold `id-token: write`, write the build's printed
///   digest as their output, the public digest `compare` checks.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "L1",
        ".github/scripts/no-credentials.sh",
        "found=$(compgen -e | grep -E -i 'KEY|SECRET|TOKEN|PASSWORD|PASSWD|CREDENTIAL|ALPACA|APCA|ROBINHOOD' || true)",
    ),
    (
        "L5",
        ".github/workflows/release.yml",
        "images/test-key/cosign.pub",
    ),
    (
        "L3",
        ".github/workflows/merge.yml",
        "RUN_PRS: ${{ toJSON(github.event.workflow_run.pull_requests.*.number) }}",
    ),
    (
        "L8",
        ".github/workflows/merge.yml",
        "echo \"merged=true\" >> \"$GITHUB_OUTPUT\"",
    ),
    (
        "L8",
        ".github/workflows/release.yml",
        "run: cargo xtask release build dist | tee -a \"$GITHUB_OUTPUT\"",
    ),
];

/// Every `run:` a step that a secret reaches may hold, by workflow and line, matched exactly, and
/// so every `run:` a step after one may hold (L9's closed world). Each runs a binary or script
/// built or checked out before any secret is in reach, never a build:
/// - `infra.yml`'s two steps run the xtask binary its secret-free step built (DEC-2014, #1703);
/// - `r2-lockcheck.yml`'s step runs the example its secret-free step built;
/// - `release.yml`'s signing step runs the reviewed `release-sign.sh` (DEC-2024);
/// - `web-deploy.yml`'s step runs the deploy tool its earlier `npm ci --ignore-scripts` installed
///   from the lockfile, its job's shape pinned by `web/scripts/workers.mjs` (#1700).
const SECRET_RUNS: &[(&str, &str)] = &[
    (".github/workflows/infra.yml", "target/debug/xtask infra"),
    (
        ".github/workflows/infra.yml",
        "target/debug/xtask infra close",
    ),
    (
        ".github/workflows/r2-lockcheck.yml",
        "target/debug/examples/r2_finding",
    ),
    (
        ".github/workflows/release.yml",
        ".github/scripts/release-sign.sh",
    ),
    (".github/workflows/web-deploy.yml", "npx wrangler deploy"),
];

/// Every path an `upload-artifact` step may upload, by workflow and path, matched exactly (L5):
/// the mutation cost reports, the release build's binaries, SBOMs, images and their test-key
/// signatures, what each publish wrote, the release to sign and its signatures, and the
/// Playwright traces of a failed end-to-end run, uploaded before the sign-in suite runs, so they
/// never hold a sign-in session's cookies.
const ARTIFACT_PATHS: &[(&str, &str)] = &[
    (".github/workflows/ci.yml", "target/mutant-cost.json"),
    (
        ".github/workflows/ci.yml",
        "target/mutant-costs/mutant-costs.json",
    ),
    (".github/workflows/release.yml", "dist/"),
    (".github/workflows/release.yml", "images/IMAGES"),
    (".github/workflows/release.yml", "images/images/"),
    (".github/workflows/release.yml", "images/signatures/"),
    (
        ".github/workflows/release.yml",
        "images/test-key/cosign.pub",
    ),
    (".github/workflows/release.yml", "published/PUBLISHED"),
    (".github/workflows/release.yml", "published/IMAGES"),
    (".github/workflows/release.yml", "published/CHARTS"),
    (".github/workflows/release.yml", "dist/SHA256SUMS"),
    (".github/workflows/release.yml", "dist/bin/"),
    (".github/workflows/release.yml", "signatures/"),
    (".github/workflows/web-e2e.yml", "web/test-results/"),
];

/// The words no artifact path may hold (L5).
const ARTIFACT_WORDS: [&str; 11] = [
    "deploy/tofu",
    ".terraform",
    "plan",
    "state",
    "key",
    "secret",
    "token",
    "kubeconfig",
    ".env",
    "auth",
    "credential",
];

/// One file the guard reads: its path from the root, and its text.
pub(crate) struct Source {
    pub(crate) path: String,
    pub(crate) text: String,
}

/// One refused line: the rule, the file and its 1-based line, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub(crate) rule: &'static str,
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) why: &'static str,
}

/// Every workflow (`.yml`, `.yaml`) and every script under [`GUARDED_DIRS`] of `root`.
pub(crate) fn sources(root: &Path) -> Result<Vec<Source>> {
    let mut found = Vec::new();
    for dir in GUARDED_DIRS {
        let Ok(entries) = fs::read_dir(root.join(dir)) else {
            continue;
        };
        let mut paths: Vec<_> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_file())
            .collect();
        paths.sort();
        for path in paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .context("a file name")?;
            if dir.ends_with("workflows") && !(name.ends_with(".yml") || name.ends_with(".yaml")) {
                continue;
            }
            let text =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            found.push(Source {
                path: format!("{dir}/{name}"),
                text,
            });
        }
    }
    Ok(found)
}

/// Runs the guard over the repository at `root`.
pub(crate) fn guard_in(root: &Path) -> Result<()> {
    eprintln!(
        "    leak-guard: checking workflows and scripts for what could leak a secret (DEC-2032)"
    );
    let sources = sources(root)?;
    let found = findings(&sources);
    if found.is_empty() {
        return Ok(());
    }
    let listed: Vec<String> = found
        .iter()
        .map(|f| format!("  {}:{}: {}: {}", f.path, f.line, f.rule, f.why))
        .collect();
    bail!(
        "leak-guard refuses {} line(s) (DEC-2032); change each, or list it in xtask/src/leak.rs's \
         ALLOWED once a reviewer has read it:\n{}",
        found.len(),
        listed.join("\n")
    )
}

/// Every refused line of `sources`, less the [`ALLOWED`] ones.
pub(crate) fn findings(sources: &[Source]) -> Vec<Finding> {
    let mut found = Vec::new();
    let secret_scripts: Vec<String> = sources
        .iter()
        .filter(|source| source.path.contains("/workflows/"))
        .flat_map(|source| {
            jobs(&source.text)
                .into_iter()
                .filter(|job| job.secret)
                .flat_map(|job| {
                    job.lines
                        .iter()
                        .flat_map(|(_, line)| script_paths(line))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let mut push = |rule: &'static str, path: &str, line: usize, text: &str, why: &'static str| {
        if !ALLOWED.contains(&(rule, path, text.trim())) {
            found.push(Finding {
                rule,
                path: path.to_owned(),
                line,
                why,
            });
        }
    };
    for source in sources {
        if source.path.contains("/workflows/") {
            let all = job_findings(&source.path, &source.text)
                .into_iter()
                .chain(workflow_findings(&source.path, &source.text));
            for (rule, number, text, why) in all {
                push(rule, &source.path, number, &text, why);
            }
        } else if secret_scripts.contains(&source.path) {
            for (index, line) in source.text.lines().enumerate() {
                if let Some(why) = runner_file(line) {
                    push("L8", &source.path, index.saturating_add(1), line, why);
                }
            }
        }
        let mut env_block: Option<usize> = None;
        let mut env_lines: Vec<bool> = Vec::new();
        for (index, raw) in source.text.lines().enumerate() {
            let trimmed = raw.trim();
            let indent = raw.len().saturating_sub(raw.trim_start().len());
            if env_block.is_some_and(|key| !trimmed.is_empty() && indent <= key) {
                env_block = None;
            }
            let opens_env = trimmed == "env:" || trimmed == "- env:";
            let inline_env = trimmed.starts_with("env:") || trimmed.starts_with("- env:");
            env_lines.push(env_block.is_some() || inline_env);
            let mut hits: Vec<(&'static str, &'static str)> = Vec::new();
            if let Some(why) = shebang_trace(trimmed) {
                hits.push(("L2", why));
            }
            if !trimmed.starts_with('#') {
                for words in segments(trimmed) {
                    let inner = shell_program(&words);
                    for words in std::iter::once(words).chain(inner) {
                        hits.extend(dump(&words).map(|why| ("L1", why)));
                        hits.extend(trace(&words).map(|why| ("L2", why)));
                        hits.extend(tool(&words).map(|why| ("L7", why)));
                        hits.extend(release_upload(&words).map(|why| ("L5", why)));
                    }
                }
                if trimmed.to_ascii_lowercase().contains("::add-mask::") {
                    hits.push((
                        "L8",
                        "`::add-mask::` must escape its value first (`%` to `%25`, CR, LF) and be \
                         a reviewed line",
                    ));
                }
                hits.extend(line_dump(trimmed).map(|why| ("L1", why)));
                hits.extend(line_trace(trimmed).map(|why| ("L2", why)));
                hits.extend(inherits(trimmed).map(|why| ("L3", why)));
            }
            for (rule, why) in hits {
                push(rule, &source.path, index.saturating_add(1), trimmed, why);
            }
            if opens_env {
                env_block = Some(indent);
            }
        }
        let lines: Vec<&str> = source.text.lines().collect();
        for (index, expression) in expressions(&source.text) {
            let in_env = env_lines.get(index).copied().unwrap_or(false);
            let trimmed = lines.get(index).map(|line| line.trim()).unwrap_or_default();
            if let Some(why) = expression_rule(&expression, in_env) {
                push("L3", &source.path, index.saturating_add(1), trimmed, why);
            }
        }
    }
    found
}

/// The commands of one line: split at `;`, `|`, `&`, `(`, `)`, `{`, `}`, `!` and backticks, then
/// into words, with a leading YAML key or list dash, shell keywords (`if`, `then`, `do`, …) and
/// variable assignments (`A=1`) dropped, a leading `\` taken off, and wrappers (`sudo`, `exec`,
/// `command`, `nohup`, `time`, `env`, `timeout`, `xargs`) stepped over with their options, so each
/// command's own name comes first. Quotes are dropped, not parsed: a refused word inside a string
/// is refused too.
fn segments(line: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for piece in line.split([';', '|', '&', '(', ')', '`', '{', '}', '!']) {
        let mut words: Vec<String> = piece
            .split_whitespace()
            .map(|word| word.trim_matches(['"', '\'']).to_owned())
            .filter(|word| !word.is_empty())
            .collect();
        loop {
            while words.first().is_some_and(|word| {
                word == "-"
                    || word.ends_with(':')
                    || word == "$"
                    || assignment(word)
                    || KEYWORDS.contains(&word.as_str())
            }) {
                words.remove(0);
            }
            if let Some(first) = words.first_mut()
                && let Some(bare) = first.strip_prefix('\\')
            {
                *first = bare.to_owned();
            }
            match wrapped(&words) {
                Some(at) => {
                    words.drain(..at);
                }
                None => break,
            }
        }
        if !words.is_empty() {
            out.push(words);
        }
    }
    out
}

/// The shell keywords that come before a command and are not one.
const KEYWORDS: [&str; 12] = [
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
];

/// A shell variable assignment, `NAME=value`.
fn assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Each wrapper, with the options that take the next word as their value.
const WRAPPERS: &[(&str, &[&str])] = &[
    (
        "sudo",
        &["-u", "-g", "-C", "-D", "-h", "-p", "-r", "-t", "-U"],
    ),
    ("exec", &["-a"]),
    ("command", &[]),
    ("nohup", &[]),
    ("time", &[]),
    ("env", &["-u", "--unset", "-C", "--chdir"]),
    ("timeout", &["-s", "--signal", "-k", "--kill-after"]),
    ("xargs", &["-a", "-d", "-E", "-I", "-L", "-n", "-P", "-s"]),
];

/// Where the command a wrapper runs starts in `words`, when `words` starts with a wrapper that
/// runs one: past its options, the values they take, any assignments, and `timeout`'s duration.
/// `None` for a wrapper with no command after it, a redirect not being one, so a bare `env` stays
/// a bare `env`.
fn wrapped(words: &[String]) -> Option<usize> {
    let name = program(words);
    let (_, valued) = WRAPPERS.iter().find(|(wrapper, _)| *wrapper == name)?;
    let mut at = 1;
    while let Some(word) = words.get(at) {
        if valued.contains(&word.as_str()) {
            at = at.saturating_add(2);
        } else if word.starts_with('-') || (name == "env" && assignment(word)) {
            at = at.saturating_add(1);
        } else {
            break;
        }
    }
    if name == "timeout" {
        at = at.saturating_add(1);
    }
    words
        .get(at)
        .is_some_and(|word| !word.contains(['>', '<']))
        .then_some(at)
}

/// The program a command names, by its last path segment.
fn program(words: &[String]) -> &str {
    words
        .first()
        .map(|word| word.rsplit('/').next().unwrap_or(word))
        .unwrap_or_default()
}

/// A single-dash flag word (`-x`, `-eux`), not `--long` and not a lone `-`.
fn short_flag(word: &str) -> bool {
    word.len() > 1 && word.starts_with('-') && !word.starts_with("--")
}

/// The words of a command up to its first redirect (`>`, `>>`, `2>&1`, `<`): a redirect and what
/// follows it are not arguments, so `env > out.txt` is still a bare `env`.
fn arguments(words: &[String]) -> Vec<&str> {
    words
        .get(1..)
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .take_while(|word| !word.contains(['>', '<']))
        .collect()
}

/// L1 for one command.
fn dump(words: &[String]) -> Option<&'static str> {
    let args = arguments(words);
    match program(words) {
        "printenv" => Some("`printenv` prints the environment"),
        "declare" | "typeset" => Some("`declare` and `typeset` can print every variable"),
        "export" if args.is_empty() || args.contains(&"-p") => {
            Some("a bare `export` or `export -p` prints the environment")
        }
        "set" if args.is_empty() => Some("a bare `set` prints every variable"),
        "env" if wrapped(words).is_none() => {
            Some("an `env` with no command, only options or assignments, prints the environment")
        }
        "compgen"
            if args
                .iter()
                .any(|a| short_flag(a) && (a.contains('e') || a.contains('v'))) =>
        {
            Some("`compgen -e` or `-v` lists the environment")
        }
        _ => None,
    }
}

/// L1 for a whole line.
fn line_dump(line: &str) -> Option<&'static str> {
    (line.contains("/proc/") && line.contains("/environ"))
        .then_some("`/proc/*/environ` holds a process's environment")
}

/// The shells whose `-x`, `-v`, `-o xtrace`, `-o verbose`, `--xtrace` and `--verbose` L2
/// refuses, and whose `-c` program L1 and L2 read as a command of its own.
const SHELLS: [&str; 5] = ["bash", "sh", "zsh", "dash", "ksh"];

/// Whether shell or `set` options turn on tracing or verbose echo: a single-dash flag holding `x`
/// or `v`, `-o xtrace`, `-o verbose`, `--xtrace` or `--verbose`, among the leading options.
fn traces(args: &[&str], only_leading: bool) -> bool {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        let named = |name: &str| matches!(name, "xtrace" | "verbose");
        if *arg == "-o" {
            if args
                .get(at.saturating_add(1))
                .is_some_and(|name| named(name))
            {
                return true;
            }
            at = at.saturating_add(2);
            continue;
        }
        if arg.strip_prefix("--").is_some_and(named) || named(arg) {
            return true;
        }
        if short_flag(arg) && arg.contains(['x', 'v']) {
            return true;
        }
        if only_leading && !arg.starts_with(['-', '+']) {
            return false;
        }
        at = at.saturating_add(1);
    }
    false
}

/// The program a shell's `-c` runs, as a command of its own, when `words` runs one.
fn shell_program(words: &[String]) -> Option<Vec<String>> {
    if !SHELLS.contains(&program(words)) {
        return None;
    }
    let at = words
        .iter()
        .skip(1)
        .position(|word| short_flag(word) && word.contains('c'))?;
    let inner: Vec<String> = words.get(at.saturating_add(2)..)?.to_vec();
    (!inner.is_empty()).then_some(inner)
}

/// L2 for one command.
fn trace(words: &[String]) -> Option<&'static str> {
    let all = words.get(1..).unwrap_or_default();
    let args = arguments(words);
    let has = |flag: &str| args.contains(&flag);
    match program(words) {
        "set" if traces(&args, false) => {
            Some("`set -x` or `-v` traces every command, `-x` with its expanded values")
        }
        name if SHELLS.contains(&name) && traces(&args, true) => Some(
            "a shell run with `-x` or `-v` traces every command, `-x` with its expanded values",
        ),
        "helm" if has("--debug") => Some("`helm --debug` prints rendered values"),
        "kubectl" if kubectl_verbosity(all) > 5 => {
            Some("`kubectl` above `--v=5` prints request headers")
        }
        "doctl" if has("-v") || has("--verbose") => Some("`doctl -v` prints request details"),
        "cosign" if has("-d") || has("--debug") => Some("`cosign -d` prints request details"),
        _ => None,
    }
}

/// The highest `-v`/`--v` level among `args`, `0` when none is given.
fn kubectl_verbosity(args: &[String]) -> u32 {
    let mut level = 0;
    for (at, arg) in args.iter().enumerate() {
        let value = if let Some(value) = arg.strip_prefix("--v=").or(arg.strip_prefix("-v=")) {
            Some(value.to_owned())
        } else if arg == "--v" || arg == "-v" {
            args.get(at.saturating_add(1)).cloned()
        } else {
            None
        };
        if let Some(parsed) = value.and_then(|value| value.parse::<u32>().ok()) {
            level = level.max(parsed);
        }
    }
    level
}

/// L2 for a whole line: the switches that turn on debug output wherever they are named.
fn line_trace(line: &str) -> Option<&'static str> {
    if line.contains("PS4=") {
        return Some("`PS4=` shapes `set -x` output");
    }
    if ["ACTIONS_STEP_DEBUG", "ACTIONS_RUNNER_DEBUG", "GH_DEBUG"]
        .iter()
        .any(|name| line.contains(name))
    {
        return Some("the runner's and `gh`'s debug switches print requests and variables");
    }
    if ["SHELLOPTS", "BASH_XTRACEFD", "BASH_ENV"]
        .iter()
        .any(|name| line.contains(name))
    {
        return Some(
            "`SHELLOPTS`, `BASH_XTRACEFD` and `BASH_ENV` turn on tracing, or run a file, in every \
             shell the step starts",
        );
    }
    if line.contains("TF_LOG") {
        return Some("`TF_LOG` prints provider requests, credentials among them");
    }
    if line.contains("RUST_LOG") && line.to_ascii_lowercase().contains("trace") {
        return Some("`RUST_LOG` at `trace` prints what libraries log about requests");
    }
    None
}

/// L2 for a shebang that runs its shell with `-x` or `-v`.
fn shebang_trace(line: &str) -> Option<&'static str> {
    let rest = line.strip_prefix("#!")?;
    rest.split_whitespace()
        .skip(1)
        .any(|word| short_flag(word) && word.contains(['x', 'v']))
        .then_some("a shebang with `-x` or `-v` traces every command")
}

/// The contexts whose whole value `toJSON(` must never print.
const CONTEXTS: [&str; 11] = [
    "secrets", "env", "github", "vars", "inputs", "steps", "needs", "matrix", "runner", "job",
    "strategy",
];

/// L3's line rule: `secrets: inherit`, in any case and spacing.
fn inherits(line: &str) -> Option<&'static str> {
    let compact: String = line
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    compact
        .contains("secrets:inherit")
        .then_some("`secrets: inherit` hands every secret to a called workflow")
}

/// Every GitHub expression of `text` with the 0-based line it starts on: each `${{ … }}`, joined
/// across lines, and each `if:` value written without one. GitHub reads expressions without regard
/// to case, so each is lowercased and its whitespace dropped before L3 reads it.
fn expressions(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = trimmed.trim_start_matches("- ").strip_prefix("if:")
            && !value.contains("${{")
        {
            out.push((index, compact(value)));
        }
    }
    let mut starts: Vec<usize> = Vec::new();
    let mut offset = 0;
    for line in &lines {
        starts.push(offset);
        offset = offset.saturating_add(line.len()).saturating_add(1);
    }
    let line_of = |at: usize| {
        starts
            .partition_point(|start| *start <= at)
            .saturating_sub(1)
    };
    let mut rest = 0;
    while let Some(open) = text.get(rest..).and_then(|tail| tail.find("${{")) {
        let begin = rest.saturating_add(open);
        let body_at = begin.saturating_add(3);
        let end = close(text, body_at);
        let start_line = line_of(begin);
        let commented = lines
            .get(start_line)
            .is_some_and(|line| line.trim_start().starts_with('#'));
        if !commented {
            out.push((
                start_line,
                compact(text.get(body_at..end).unwrap_or_default()),
            ));
        }
        rest = end.saturating_add(2).min(text.len());
        if end >= text.len() {
            break;
        }
    }
    out
}

/// Where the expression whose body starts at `body_at` ends: the first `}}` outside a quoted
/// string, since `'}}'` inside one does not end it; the end of `text` when none does.
fn close(text: &str, body_at: usize) -> usize {
    let mut quoted = false;
    let tail = text.get(body_at..).unwrap_or_default();
    for (at, c) in tail.char_indices() {
        if c == '\'' {
            quoted = !quoted;
        } else if !quoted && tail.get(at..).is_some_and(|rest| rest.starts_with("}}")) {
            return body_at.saturating_add(at);
        }
    }
    text.len()
}

/// An expression lowercased, with its whitespace dropped.
fn compact(expression: &str) -> String {
    expression
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Each place `name` stands as a word of its own in `expression`, outside a quoted string's
/// opening quote and not as a property (`x.name`), with the character after it.
fn context_words<'a>(
    expression: &'a str,
    name: &'a str,
) -> impl Iterator<Item = Option<char>> + 'a {
    let word =
        |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    expression
        .match_indices(name)
        .filter_map(move |(found, _)| {
            let before = expression
                .get(..found)
                .and_then(|head| head.chars().next_back());
            let after = expression
                .get(found.saturating_add(name.len())..)
                .and_then(|tail| tail.chars().next());
            (!word(before) && !word(after) && !matches!(before, Some('\'' | '.'))).then_some(after)
        })
}

/// L3 for one [`expressions`] entry, `in_env` when its line sits in an `env:` block: `toJSON(`
/// whose argument names a whole context anywhere in it, however nested; the whole `secrets`
/// context anywhere (`secrets` alone, in `join(` or `format(`, or `secrets.*`); `secrets[`; and
/// `secrets.` outside an `env:` block.
fn expression_rule(expression: &str, in_env: bool) -> Option<&'static str> {
    if let Some(at) = expression.find("tojson(") {
        let argument = expression.get(at..).unwrap_or_default();
        if CONTEXTS
            .iter()
            .any(|name| context_words(argument, name).next().is_some())
        {
            return Some("`toJSON(` of a context prints every value in it");
        }
    }
    if expression.contains("secrets[") {
        return Some("`secrets[` reads a secret by a computed name");
    }
    if expression.contains("secrets.*")
        || context_words(expression, "secrets").any(|after| after != Some('.'))
    {
        return Some("the whole `secrets` context is never read, only one secret by its name");
    }
    if expression.contains("secrets.") && !in_env {
        return Some("a secret is read only in an `env:` block, never pasted into a step");
    }
    None
}

/// One job of a workflow: its lines with their 1-based numbers, and whether it holds a secret:
/// an `environment:`, a `secrets.` or `secrets[` read in any case, `id-token: write`, or
/// `github.token` in a job that grants it a write permission. A read-only token in a pull
/// request's job gives nothing the pull request's own code does not already hold.
struct Job {
    lines: Vec<(usize, String)>,
    secret: bool,
}

/// Whether a workflow line gives its job a secret by itself.
fn holds_secret(trimmed: &str) -> bool {
    let lower = compact(trimmed);
    trimmed.starts_with("environment:")
        || lower.contains("secrets.")
        || lower.contains("secrets[")
        || lower.contains("id-token:write")
}

/// Whether a job's lines read `github.token` and grant a write permission (`x: write`,
/// `write-all`), which makes that token a secret.
fn writes_with_token(lines: &[(usize, String)]) -> bool {
    let compacted: Vec<String> = lines
        .iter()
        .map(|(_, raw)| compact(uncommented(raw.trim())))
        .collect();
    compacted.iter().any(|line| line.contains("github.token"))
        && compacted
            .iter()
            .any(|line| line.ends_with(":write") || line.contains("write-all"))
}

/// A line's text before a trailing ` #` comment.
fn uncommented(trimmed: &str) -> &str {
    trimmed
        .find(" #")
        .map_or(trimmed, |at| trimmed.get(..at).unwrap_or(trimmed))
        .trim_end()
}

/// The jobs of a workflow's text: each key two spaces in under the top-level `jobs:`, a trailing
/// comment aside, up to the next such key or the next top-level line.
fn jobs(text: &str) -> Vec<Job> {
    let mut out: Vec<Job> = Vec::new();
    let mut in_jobs = false;
    for (index, raw) in text.lines().enumerate() {
        let trimmed = raw.trim();
        let indent = raw.len().saturating_sub(raw.trim_start().len());
        if trimmed.is_empty() || trimmed.starts_with('#') {
            if let Some(job) = out.last_mut().filter(|_| in_jobs) {
                job.lines.push((index.saturating_add(1), raw.to_owned()));
            }
            continue;
        }
        if indent == 0 {
            in_jobs = uncommented(trimmed) == "jobs:";
            continue;
        }
        if !in_jobs {
            continue;
        }
        if indent == 2 && uncommented(trimmed).ends_with(':') {
            out.push(Job {
                lines: Vec::new(),
                secret: false,
            });
        }
        if let Some(job) = out.last_mut() {
            job.secret |= holds_secret(trimmed);
            job.lines.push((index.saturating_add(1), raw.to_owned()));
        }
    }
    for job in &mut out {
        job.secret |= writes_with_token(&job.lines);
    }
    out
}

/// The steps of a job, each its lines: every `- ` item at the indentation of the first one after
/// `steps:`, so a step's `with:` is read with its `uses:` in whichever order they come.
fn step_lines(job: &Job) -> Vec<Vec<(usize, String)>> {
    let mut out: Vec<Vec<(usize, String)>> = Vec::new();
    let mut steps_at: Option<usize> = None;
    let mut item: Option<usize> = None;
    for (number, raw) in &job.lines {
        let trimmed = raw.trim();
        let indent = raw.len().saturating_sub(raw.trim_start().len());
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if uncommented(trimmed) == "steps:" {
            steps_at = Some(indent);
            item = None;
            continue;
        }
        let Some(at) = steps_at else {
            continue;
        };
        if indent <= at && !trimmed.starts_with("- ") {
            steps_at = None;
            continue;
        }
        if trimmed.starts_with("- ") && item.is_none_or(|step| indent == step) {
            item = Some(indent);
            out.push(Vec::new());
        }
        if let Some(step) = out.last_mut() {
            step.push((*number, raw.to_owned()));
        }
    }
    out
}

/// The `.github/scripts/` files a line runs.
fn script_paths(line: &str) -> Vec<String> {
    line.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ';' | '|' | '&'))
        .filter_map(|word| {
            let at = word.find(".github/scripts/")?;
            word.get(at..).map(str::to_owned)
        })
        .collect()
}

/// The runner's files that later steps, and the run's summary, read.
const RUNNER_FILES: [&str; 5] = [
    "GITHUB_ENV",
    "GITHUB_PATH",
    "GITHUB_OUTPUT",
    "GITHUB_STEP_SUMMARY",
    "GITHUB_STATE",
];

/// L8 for a line of a secret job or of a script one runs.
fn runner_file(line: &str) -> Option<&'static str> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return None;
    }
    if RUNNER_FILES.iter().any(|name| trimmed.contains(name)) {
        return Some(
            "a secret job writes none of the runner's files, which later steps and logs read",
        );
    }
    workflow_command(trimmed)
        .filter(|name| name != "add-mask")
        .map(|_| "a secret job issues no `::` workflow command but `::add-mask::`")
}

/// The name of the first `::name ...::` workflow command in a line, lowercased, since the runner
/// reads names without regard to case: one that starts the line, a word, a quoted string, or a
/// line an escaped `\n` or `\r` begins inside a `printf` or `echo -e` string, so a Rust path such
/// as `std::env::var` is not one.
fn workflow_command(line: &str) -> Option<String> {
    let mut rest = line;
    while let Some(at) = rest.find("::") {
        let head = rest.get(..at).unwrap_or_default();
        let before = head.chars().next_back();
        let after = rest.get(at.saturating_add(2)..).unwrap_or_default();
        let escaped_break = head.ends_with("\\n") || head.ends_with("\\r");
        if before.is_some_and(|c| !(c.is_whitespace() || c == '"' || c == '\'')) && !escaped_break {
            rest = after;
            continue;
        }
        let name_len = after
            .find(|c: char| !(c.is_ascii_alphabetic() || c == '-'))
            .unwrap_or(after.len());
        let name = after.get(..name_len).unwrap_or_default();
        let tail = after.get(name_len..).unwrap_or_default();
        if !name.is_empty()
            && (tail.starts_with("::") || (tail.starts_with(' ') && tail.contains("::")))
        {
            return Some(name.to_ascii_lowercase());
        }
        rest = after;
    }
    None
}

/// The actions that cache whatever their inputs say (L6). `actions/setup-go` caches unless a step
/// sets `cache: false`.
const CACHE_ACTIONS: [&str; 4] = [
    "actions/cache",
    "rust-cache",
    "sccache-action",
    "actions/setup-go",
];

/// The step inputs that turn a cache on, at any value but `false` (L6).
const CACHE_KEYS: [&str; 6] = [
    "cache",
    "cache-dependency-path",
    "enable-cache",
    "cache-from",
    "cache-to",
    "bundler-cache",
];

/// A line's `key: value` pairs, as [`pairs`] reads them.
type Pairs = Vec<(String, String)>;

/// Each `key: value` of a line, lowercased, in block style (`key: value`) or flow style
/// (`with: { key: value, … }`), the value unquoted.
fn pairs(trimmed: &str) -> Pairs {
    let lower = uncommented(trimmed).to_ascii_lowercase();
    let body = lower.trim_start_matches("- ");
    let body = body
        .split_once('{')
        .map_or(body, |(_, inner)| inner.trim_end_matches('}'));
    body.split(',')
        .filter_map(|pair| {
            let (key, value) = pair.split_once(':')?;
            Some((
                key.trim().to_owned(),
                value.trim().trim_matches(['"', '\'']).to_owned(),
            ))
        })
        .collect()
}

/// L6 for one step of a secret job: a caching action, unless it is `setup-go` with
/// `cache: false`, and any cache input set to anything but `false`.
fn cache_findings(step: &[(usize, String)]) -> Vec<(&'static str, usize, String, &'static str)> {
    let why = "a secret job restores or saves no cache, which a pull request could have written";
    let pairs_of: Vec<(usize, &str, Pairs)> = step
        .iter()
        .map(|(number, raw)| (*number, raw.trim(), pairs(raw.trim())))
        .collect();
    let cache_off = pairs_of
        .iter()
        .any(|(_, _, pairs)| pairs.iter().any(|(k, v)| k == "cache" && v == "false"));
    let mut out = Vec::new();
    for (number, trimmed, pairs) in &pairs_of {
        let lower = trimmed.to_ascii_lowercase();
        let action = CACHE_ACTIONS.iter().find(|name| lower.contains(*name));
        if action.is_some_and(|name| *name != "actions/setup-go" || !cache_off)
            || pairs
                .iter()
                .any(|(k, v)| CACHE_KEYS.contains(&k.as_str()) && v != "false")
            || lower.contains("--cache-from")
            || lower.contains("--cache-to")
        {
            out.push(("L6", *number, (*trimmed).to_owned(), why));
        }
    }
    out
}

/// The actions that upload an artifact (L5).
const UPLOADS: [&str; 2] = ["actions/upload-artifact", "actions/upload-pages-artifact"];

/// L5 for one step: every `path:` entry of an upload step, in block, `|` block or flow style and
/// before or after its `uses:`, and `include-hidden-files` at any value but `false`, in any case
/// and quoting. An upload step with no `path:` uploads its action's default, which no reviewer has
/// read, so it is refused too.
fn upload_findings(
    path: &str,
    step: &[(usize, String)],
) -> Vec<(&'static str, usize, String, &'static str)> {
    let mut out = Vec::new();
    let Some((first, _)) = step.first() else {
        return out;
    };
    if !step.iter().any(|(_, raw)| {
        UPLOADS
            .iter()
            .any(|name| raw.to_ascii_lowercase().contains(name))
    }) {
        return out;
    }
    let mut paths_from: Option<usize> = None;
    let mut found_path = false;
    for (number, raw) in step {
        let trimmed = raw.trim();
        let indent = raw.len().saturating_sub(raw.trim_start().len());
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(from) = paths_from {
            if indent > from {
                out.extend(artifact_path(path, *number, trimmed));
                continue;
            }
            paths_from = None;
        }
        for (key, value) in pairs(trimmed) {
            if key == "include-hidden-files" && value != "false" {
                out.push((
                    "L5",
                    *number,
                    trimmed.to_owned(),
                    "an artifact uploads no hidden files",
                ));
            }
        }
        let flow = trimmed.contains('{');
        let original = uncommented(trimmed).trim_start_matches("- ");
        let entries: Vec<&str> = if flow {
            original
                .split_once('{')
                .map(|(_, inner)| inner.trim_end_matches('}'))
                .unwrap_or_default()
                .split(',')
                .filter_map(|pair| pair.trim().strip_prefix("path:"))
                .collect()
        } else {
            original.strip_prefix("path:").into_iter().collect()
        };
        for value in entries {
            found_path = true;
            let value = value.trim();
            if value == "|" || value == ">" || value.is_empty() {
                paths_from = Some(indent);
            } else {
                out.extend(artifact_path(
                    path,
                    *number,
                    value.trim_matches(['"', '\'']),
                ));
            }
        }
    }
    if !found_path {
        out.push((
            "L5",
            *first,
            step.first()
                .map(|(_, raw)| raw.trim().to_owned())
                .unwrap_or_default(),
            "an upload step names its paths; its action's default path is not reviewed",
        ));
    }
    out
}

/// L5, L6 and L8 for a workflow: what its jobs and steps hold, by rule, line number, text and why.
fn job_findings(path: &str, text: &str) -> Vec<(&'static str, usize, String, &'static str)> {
    let mut out = Vec::new();
    let workflow_secret = workflow_level_secret(text);
    for job in jobs(text) {
        out.extend(step_shape(path, &job, workflow_secret));
        for step in step_lines(&job) {
            out.extend(upload_findings(path, &step));
            if job.secret {
                out.extend(cache_findings(&step));
            }
        }
        if job.secret {
            for (number, raw) in &job.lines {
                if let Some(why) = runner_file(raw) {
                    out.push(("L8", *number, raw.trim().to_owned(), why));
                }
            }
        }
    }
    out
}

/// One step of a job: its lines, and whether a secret reaches it from its own `env:`, `with:` or
/// `run:` (`secrets.` or `secrets[`, in any case).
struct Step {
    lines: Vec<(usize, String)>,
    secret: bool,
}

/// Whether a line reads a secret: `secrets.` or `secrets[`, in any case and spacing.
fn reads_secret(line: &str) -> bool {
    let lower = compact(line);
    lower.contains("secrets.") || lower.contains("secrets[")
}

/// The steps of a job, as [`step_lines`] finds them, each with whether it reads a secret itself.
fn steps(job: &Job) -> Vec<Step> {
    step_lines(job)
        .into_iter()
        .map(|lines| Step {
            secret: lines.iter().any(|(_, raw)| reads_secret(raw)),
            lines,
        })
        .collect()
}

/// Whether a secret reaches every step of a job from outside them: a job line no step holds, its
/// own `env:` or a reusable workflow's `with:` or `secrets:`, reads one.
fn job_level_secret(job: &Job) -> bool {
    let in_steps: Vec<usize> = step_lines(job)
        .iter()
        .flatten()
        .map(|(number, _)| *number)
        .collect();
    job.lines
        .iter()
        .filter(|(number, _)| !in_steps.contains(number))
        .any(|(_, raw)| !raw.trim().starts_with('#') && reads_secret(raw))
}

/// Whether the workflow's top-level `env:`, which every step of every job inherits, reads a secret.
fn workflow_level_secret(text: &str) -> bool {
    let mut in_env = false;
    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !raw.starts_with(' ') {
            in_env = uncommented(trimmed) == "env:" || trimmed.starts_with("env:");
            if in_env && reads_secret(trimmed) {
                return true;
            }
            continue;
        }
        if in_env && reads_secret(trimmed) {
            return true;
        }
    }
    false
}

/// The value of a step's `key:`, its first line's text after the key, wherever in the step it is.
fn step_value<'a>(step: &'a Step, key: &str) -> Option<(usize, &'a str)> {
    step.lines.iter().find_map(|(number, raw)| {
        uncommented(raw.trim())
            .trim_start_matches("- ")
            .strip_prefix(key)
            .and_then(|rest| rest.strip_prefix(':'))
            .map(|value| (*number, value.trim()))
    })
}

/// Why a step that a secret reaches, or that follows one in its job, is outside L9's closed world,
/// or `None` when it is inside it. Its `run:` is one line of [`SECRET_RUNS`] for its workflow,
/// exactly, never a block; its `uses:` is one of xtask's reviewed pins, `READ_ACTIONS`, never a
/// local action; and it sets no `shell:`.
fn outside_closed_world(path: &str, step: &Step) -> Option<&'static str> {
    if step_value(step, "shell").is_some() {
        return Some("a step a secret reaches sets no `shell:`");
    }
    if let Some((_, run)) = step_value(step, "run")
        && !SECRET_RUNS.contains(&(path, run))
    {
        return Some(
            "a step a secret reaches, or that follows one, runs only one line of xtask/src/leak.rs's \
             SECRET_RUNS",
        );
    }
    if let Some((_, uses)) = step_value(step, "uses") {
        let action = uses
            .trim_matches(['"', '\''])
            .split_whitespace()
            .next()
            .unwrap_or_default();
        let reviewed = crate::READ_ACTIONS
            .iter()
            .any(|(name, only)| *name == action && only.is_none_or(|only| only == path));
        if !reviewed {
            return Some(
                "a step a secret reaches, or that follows one, uses only an action xtask's \
                 READ_ACTIONS pins",
            );
        }
    }
    None
}

/// L9 for a job: every step a secret reaches, from the step, the job or the workflow, and every
/// step after the first such step, is inside the closed world; and a secret job runs no local
/// action, whose steps no reviewer has read here.
fn step_shape(
    path: &str,
    job: &Job,
    workflow_secret: bool,
) -> Vec<(&'static str, usize, String, &'static str)> {
    let mut out = Vec::new();
    let job_secret = workflow_secret || job_level_secret(job);
    let mut after_secret = false;
    for step in steps(job) {
        let Some((number, first)) = step.lines.first() else {
            continue;
        };
        if job.secret
            && step_value(&step, "uses")
                .is_some_and(|(_, uses)| uses.trim_matches(['"', '\'']).starts_with("./"))
        {
            out.push((
                "L9",
                *number,
                first.trim().to_owned(),
                "a secret job runs no local action",
            ));
        }
        let secret = step.secret || job_secret;
        if (secret || after_secret)
            && let Some(why) = outside_closed_world(path, &step)
        {
            out.push(("L9", *number, first.trim().to_owned(), why));
        }
        after_secret |= secret;
    }
    out
}

/// L9's pins and L12's triggers for a whole workflow.
fn workflow_findings(path: &str, text: &str) -> Vec<(&'static str, usize, String, &'static str)> {
    let mut out = Vec::new();
    let mut triggers: Vec<(usize, String, String)> = Vec::new();
    let mut in_on = false;
    let mut trigger: Option<String> = None;
    for (index, raw) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        let trimmed = raw.trim();
        let indent = raw.len().saturating_sub(raw.trim_start().len());
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = trimmed.trim_start_matches("- ").strip_prefix("uses:") {
            let value = value.trim().trim_matches(['"', '\'']);
            if !(value.starts_with("./") || value.starts_with("docker://") || pinned(value)) {
                out.push((
                    "L9",
                    number,
                    trimmed.to_owned(),
                    "every `uses:` is pinned to a 40-character commit",
                ));
            }
        }
        if indent == 0 {
            let on = ["on:", "\"on\":", "'on':", "true:"]
                .iter()
                .find_map(|key| trimmed.strip_prefix(key));
            in_on = on.is_some();
            trigger = None;
            if let Some(inline) = on.map(|rest| uncommented(rest.trim())) {
                for name in inline
                    .trim_matches(['[', ']'])
                    .split(',')
                    .map(|name| name.trim().trim_matches(['"', '\'']))
                    .filter(|name| !name.is_empty())
                {
                    triggers.push((number, name.to_owned(), String::new()));
                }
            }
            continue;
        }
        if in_on {
            if indent == 2 {
                let name = trimmed
                    .split(':')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .trim_matches(['"', '\''])
                    .to_owned();
                triggers.push((number, name.clone(), String::new()));
                trigger = Some(name);
            } else if trigger.is_some()
                && let Some(last) = triggers.last_mut()
            {
                last.2.push_str(trimmed);
                last.2.push('\n');
            }
        }
    }
    let merge = path.ends_with("/merge.yml");
    for (number, name, _) in &triggers {
        if !merge && matches!(name.as_str(), "pull_request_target" | "workflow_run") {
            out.push((
                "L12",
                *number,
                format!("{name}:"),
                "`pull_request_target` and `workflow_run` run only in `merge.yml`",
            ));
        }
        if OUTSIDE_TRIGGERS.contains(&name.as_str()) {
            out.push((
                "L12",
                *number,
                format!("{name}:"),
                "no workflow runs on an event anyone can raise with the default branch's secrets",
            ));
        }
    }
    if merge {
        for (index, raw) in text.lines().enumerate() {
            let trimmed = raw.trim();
            if !trimmed.starts_with('#')
                && ["head.sha", "head.ref", "head_ref"]
                    .iter()
                    .any(|name| trimmed.contains(name))
            {
                out.push((
                    "L12",
                    index.saturating_add(1),
                    trimmed.to_owned(),
                    "`merge.yml` names no pull request head, so it never checks one out",
                ));
            }
        }
    }
    let environment_jobs: Vec<Job> = jobs(text)
        .into_iter()
        .filter(|job| {
            job.lines
                .iter()
                .any(|(_, line)| line.trim().starts_with("environment:"))
        })
        .collect();
    if !environment_jobs.is_empty() {
        for (number, name, body) in &triggers {
            let main_push = name == "push" && pushes_only_main(body);
            if !(name == "workflow_dispatch" || main_push) {
                out.push((
                    "L12",
                    *number,
                    format!("{name}:"),
                    "a workflow with an `environment:` triggers only on `workflow_dispatch` or a \
                     push to `main`",
                ));
            }
        }
        for job in &environment_jobs {
            let gated = job.lines.iter().any(|(_, line)| {
                let indent = line.len().saturating_sub(line.trim_start().len());
                indent == 4
                    && line
                        .trim()
                        .strip_prefix("if:")
                        .is_some_and(|value| gates_on_main(uncommented(value.trim())))
            });
            if !gated && let Some((number, header)) = job.lines.first() {
                out.push((
                    "L12",
                    *number,
                    header.trim().to_owned(),
                    "a job with an `environment:` has an `if:` naming `refs/heads/main`",
                ));
            }
        }
    }
    out
}

/// The triggers that run a workflow, with the default branch's secrets, on an event anyone can
/// raise: a comment, an issue, a review, a discussion, or an API dispatch (L12).
const OUTSIDE_TRIGGERS: [&str; 7] = [
    "issue_comment",
    "issues",
    "pull_request_review",
    "pull_request_review_comment",
    "discussion",
    "discussion_comment",
    "repository_dispatch",
];

/// Whether a `push` trigger's body names exactly one branch, `main`, in either list style, and no
/// tags or ignored branches. A body with no `branches:` pushes on every branch.
fn pushes_only_main(body: &str) -> bool {
    let mut key = String::new();
    let mut branches: Vec<String> = Vec::new();
    for line in body.lines().map(|line| uncommented(line.trim())) {
        if let Some(item) = line.strip_prefix("- ") {
            if key == "branches" {
                branches.push(item.trim().trim_matches(['"', '\'']).to_owned());
            }
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        key = name.trim().trim_matches(['"', '\'']).to_owned();
        if matches!(key.as_str(), "tags" | "tags-ignore" | "branches-ignore") {
            return false;
        }
        let value = value.trim();
        if key == "branches" && !value.is_empty() {
            branches.extend(
                value
                    .trim_matches(['[', ']'])
                    .split(',')
                    .map(|name| name.trim().trim_matches(['"', '\'']).to_owned())
                    .filter(|name| !name.is_empty()),
            );
        }
    }
    branches == ["main"]
}

/// Whether a job's `if:` admits only `main`: `github.ref == 'refs/heads/main'` alone or joined to
/// other terms by `&&`, with no `||`, no `!=` and no negation anywhere, so no other term can widen it.
fn gates_on_main(value: &str) -> bool {
    let expression = compact(value);
    let expression = expression
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .unwrap_or(&expression);
    if expression.contains("||") || expression.contains('!') {
        return false;
    }
    expression.split("&&").any(|term| {
        matches!(
            term.trim_matches(['(', ')']),
            "github.ref=='refs/heads/main'" | "'refs/heads/main'==github.ref"
        )
    })
}

/// A `uses:` value pinned to a 40-character lowercase hex commit.
fn pinned(value: &str) -> bool {
    value
        .split_once('@')
        .map(|(_, rest)| rest.split_whitespace().next().unwrap_or_default())
        .is_some_and(|sha| {
            sha.len() == 40
                && sha
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

/// L5 for one artifact path entry.
fn artifact_path(
    path: &str,
    number: usize,
    entry: &str,
) -> Vec<(&'static str, usize, String, &'static str)> {
    let mut out = Vec::new();
    let lower = entry.to_ascii_lowercase();
    if ARTIFACT_WORDS.iter().any(|word| lower.contains(word)) {
        out.push((
            "L5",
            number,
            entry.to_owned(),
            "an artifact path names deploy state, a plan, a key, a secret, a token, a kubeconfig, \
             an `.env` or auth",
        ));
    }
    if !ARTIFACT_PATHS.contains(&(path, entry)) {
        out.push((
            "L5",
            number,
            entry.to_owned(),
            "an artifact path is not one of xtask/src/leak.rs's ARTIFACT_PATHS",
        ));
    }
    out
}

/// L5 for one command: `gh release upload`, which publishes files outside any reviewed artifact
/// path.
fn release_upload(words: &[String]) -> Option<&'static str> {
    (program(words) == "gh"
        && words.get(1).is_some_and(|a| a == "release")
        && words.get(2).is_some_and(|a| a == "upload"))
    .then_some("`gh release upload` publishes files no reviewer has listed")
}

/// `curl`'s short options that take a value, attached (`-uname`) or as the next word.
const CURL_VALUED: [char; 26] = [
    'u', 'H', 'D', 'o', 'd', 'X', 'A', 'e', 'b', 'c', 'F', 'T', 'w', 'x', 'U', 'E', 'K', 'r', 'm',
    'Y', 'y', 'z', 'C', 't', 'Q', 'P',
];

/// L7 for `curl`'s arguments: no verbose, traced, header-dumping or signed request, and no header
/// or user written inline, each read inside combined short options (`-sSfLv`, `-sSu name`) and
/// with an attached value (`-uname`, `-D-`) as well as in long form.
fn curl(args: &[String]) -> Option<&'static str> {
    let loud = "`curl` writes no verbose output, trace, headers or signed request";
    let inline = "a `curl` header or user is read from an `@file`, never written inline";
    for (at, arg) in args.iter().enumerate() {
        let next = args.get(at.saturating_add(1)).map(String::as_str);
        if matches!(arg.as_str(), "--verbose" | "--include")
            || arg.starts_with("--trace")
            || arg.starts_with("--dump-header")
            || arg.starts_with("--aws-sigv4")
        {
            return Some(loud);
        }
        let long_value = match arg.as_str() {
            "--header" | "--user" => next,
            other => other
                .strip_prefix("--header=")
                .or(other.strip_prefix("--user=")),
        };
        if long_value.is_some_and(|value| !value.starts_with('@')) {
            return Some(inline);
        }
        if !short_flag(arg) {
            continue;
        }
        for (offset, flag) in arg.char_indices().skip(1) {
            if matches!(flag, 'v' | 'i' | 'D') {
                return Some(loud);
            }
            if CURL_VALUED.contains(&flag) {
                let attached = arg.get(offset.saturating_add(1)..).unwrap_or_default();
                let value = if attached.is_empty() {
                    next
                } else {
                    Some(attached)
                };
                if matches!(flag, 'H' | 'u') && value.is_none_or(|value| !value.starts_with('@')) {
                    return Some(inline);
                }
                break;
            }
        }
    }
    None
}

/// L7 for one command.
fn tool(words: &[String]) -> Option<&'static str> {
    let args = words.get(1..).unwrap_or_default();
    if args
        .iter()
        .any(|a| a.starts_with("--password") && a != "--password-stdin")
    {
        return Some("a password is given only on standard input (`--password-stdin`)");
    }
    match program(words) {
        "curl" => curl(args),
        "docker" if args.iter().any(|a| a == "login") && args.iter().any(|a| a == "-p") => {
            Some("`docker login -p` puts the password in the command line")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo_root;

    fn one(path: &str, text: &str) -> Vec<Finding> {
        findings(&[Source {
            path: path.to_owned(),
            text: text.to_owned(),
        }])
    }

    fn rules(path: &str, text: &str) -> Vec<&'static str> {
        one(path, text).into_iter().map(|f| f.rule).collect()
    }

    const WORKFLOW: &str = ".github/workflows/x.yml";
    const SCRIPT: &str = ".github/scripts/x.sh";

    /// Each rule's fixture that must fail, with the rule it must name.
    const FAILING: &[(&str, &str)] = &[
        ("L1", "      - run: printenv"),
        ("L1", "        run: env | sort"),
        ("L1", "declare -p"),
        ("L1", "typeset -x"),
        ("L1", "export -p > env.txt"),
        ("L1", "compgen -v"),
        ("L1", "cat /proc/self/environ"),
        ("L1", "sudo env"),
        ("L2", "set -euxo pipefail"),
        ("L2", "set -o xtrace"),
        ("L2", "bash -x ./build.sh"),
        ("L2", "#!/usr/bin/env bash -x"),
        ("L2", "export PS4='+ $BASH_SOURCE'"),
        ("L2", "          ACTIONS_STEP_DEBUG: true"),
        ("L2", "          GH_DEBUG: api"),
        ("L2", "TF_LOG=DEBUG tofu plan"),
        ("L2", "          RUST_LOG: trace"),
        ("L2", "helm upgrade --install x . --debug"),
        ("L2", "kubectl get pods --v=8"),
        ("L2", "kubectl get pods -v 9"),
        ("L2", "doctl compute droplet list -v"),
        ("L2", "cosign sign -d image"),
        ("L3", "          ALL: ${{ toJSON(secrets) }}"),
        ("L3", "        run: echo '${{ toJSON(github) }}'"),
        ("L3", "          X: ${{ secrets[matrix.name] }}"),
        ("L3", "    secrets: inherit"),
        ("L3", "        run: deploy --token ${{ secrets.TOKEN }}"),
        ("L3", "        if: secrets.TOKEN != ''"),
        ("L7", "curl -v https://example.com"),
        ("L7", "curl -sSfLv https://example.com"),
        ("L7", "curl --trace-ascii log https://example.com"),
        ("L7", "curl -i https://example.com"),
        ("L7", "curl -D headers.txt https://example.com"),
        (
            "L7",
            "curl -H \"Authorization: Bearer $TOKEN\" https://example.com",
        ),
        ("L7", "curl -u \"user:$PASSWORD\" https://example.com"),
        ("L7", "curl --aws-sigv4 aws:amz https://example.com"),
        ("L7", "docker login -u me -p \"$TOKEN\" ghcr.io"),
        ("L7", "helm registry login --password \"$TOKEN\" ghcr.io"),
        ("L8", "echo \"::add-mask::$TOKEN\""),
    ];

    /// Lines each rule must let through.
    const PASSING: &[&str] = &[
        "set -euo pipefail",
        "set +x",
        "env -i PATH=/usr/bin make",
        "env FOO=1 cargo test",
        "        env:",
        "      - env: { TOKEN: ${{ secrets.TOKEN }} }",
        "bash ./build.sh",
        "#!/usr/bin/env bash",
        "kubectl get pods --v=4",
        "helm template x .",
        "cosign sign --yes image",
        "          RUST_LOG: info",
        "          X: ${{ toJSON(fromJSON('[1, 2]')) }}",
        "          X: ${{ toJSON(githubby) }}",
        "# never run printenv or set -x here",
        "echo 'the secrets are in the vault'",
        "export PATH=\"$HOME/bin:$PATH\"",
        "curl -sSfL --retry 5 -o x.tgz https://example.com/x.tgz",
        "curl -H @headers.txt https://example.com",
        "echo \"$TOKEN\" | docker login -u me --password-stdin ghcr.io",
        "run: cargo xtask ci select >> \"$GITHUB_OUTPUT\"",
    ];

    #[test]
    fn each_rule_refuses_its_failing_fixtures() {
        for (rule, line) in FAILING {
            let path = if line.starts_with("#!") || !line.starts_with(' ') {
                SCRIPT
            } else {
                WORKFLOW
            };
            assert!(
                rules(path, line).contains(rule),
                "{rule} refuses {line:?}: {:?}",
                one(path, line)
            );
        }
    }

    #[test]
    fn each_rule_passes_its_passing_fixtures() {
        for line in PASSING {
            assert_eq!(one(WORKFLOW, line), Vec::new(), "{line:?}");
        }
    }

    /// An `env:` block holds secrets until the indentation leaves it; a secret after it, in a
    /// step's script, is refused.
    #[test]
    fn a_secret_is_read_only_inside_an_env_block() {
        let text = "      - name: Deploy\n        env:\n          TOKEN: ${{ secrets.TOKEN }}\n\
                    \n          OTHER: ${{ secrets.OTHER }}\n        run: |\n          \
                    deploy ${{ secrets.TOKEN }}\n";
        let found = one(WORKFLOW, text);
        assert_eq!(
            found.iter().map(|f| (f.rule, f.line)).collect::<Vec<_>>(),
            [("L3", 7)],
            "{found:?}"
        );
    }

    /// A workflow with a job that holds a secret and one that does not.
    fn workflow(secret_steps: &str, open_steps: &str) -> String {
        format!(
            "on: workflow_dispatch\njobs:\n  open:\n    runs-on: ubuntu-24.04\n    steps:\n\
             {open_steps}\n  deploy:\n    if: github.ref == 'refs/heads/main'\n\
             \x20   environment: production\n    steps:\n{secret_steps}\n"
        )
    }

    /// L6 and L8: a cache or a runner-file write is refused in the secret job and let through in
    /// the open one.
    #[test]
    fn a_secret_job_restores_no_cache_and_writes_no_runner_file() {
        let steps = "      - uses: Swatinem/rust-cache@0123456789012345678901234567890123456789\n\
                     \x20     - uses: actions/setup-node@0123456789012345678901234567890123456789\n\
                     \x20       with:\n          cache: npm\n\
                     \x20     - run: echo \"x=1\" >> \"$GITHUB_OUTPUT\"\n\
                     \x20     - run: echo \"::warning ::done\"";
        let found = rules(WORKFLOW, &workflow(steps, steps));
        assert_eq!(found, ["L6", "L6", "L8", "L8"], "{found:?}");
    }

    /// L8 reaches a script a secret job runs, and only such a script.
    #[test]
    fn a_script_a_secret_job_runs_writes_no_runner_file() {
        let flow = workflow(
            "      - run: .github/scripts/sign.sh",
            "      - run: .github/scripts/open.sh",
        );
        let text = "#!/usr/bin/env bash\necho \"x=1\" >> \"$GITHUB_OUTPUT\"\n";
        let found = findings(&[
            Source {
                path: WORKFLOW.to_owned(),
                text: flow,
            },
            Source {
                path: ".github/scripts/sign.sh".to_owned(),
                text: text.to_owned(),
            },
            Source {
                path: ".github/scripts/open.sh".to_owned(),
                text: text.to_owned(),
            },
        ]);
        assert_eq!(
            found
                .iter()
                .map(|f| (f.rule, f.path.as_str()))
                .collect::<Vec<_>>(),
            [("L8", ".github/scripts/sign.sh")],
            "{found:?}"
        );
    }

    /// L5: an unreviewed path, a path naming a key or state, and hidden files are refused; a
    /// reviewed path passes, inline or in a block.
    #[test]
    fn an_artifact_uploads_only_a_reviewed_path() {
        let upload = |with: &str| {
            format!(
                "      - uses: actions/upload-artifact@0123456789012345678901234567890123456789\n\
                 \x20       with:\n          name: x\n{with}"
            )
        };
        let path = ".github/workflows/ci.yml";
        let reviewed = workflow("", &upload("          path: target/mutant-cost.json"));
        assert_eq!(rules(path, &reviewed), Vec::<&str>::new());
        let block = upload(
            "          path: |\n            target/mutant-cost.json\n            deploy/tofu/plan.out",
        );
        assert_eq!(rules(path, &workflow("", &block)), ["L5", "L5"]);
        let unlisted = upload("          path: out/report.json");
        assert_eq!(rules(path, &workflow("", &unlisted)), ["L5"]);
        let hidden =
            upload("          path: target/mutant-cost.json\n          include-hidden-files: true");
        assert_eq!(rules(path, &workflow("", &hidden)), ["L5"]);
    }

    #[test]
    fn a_workflow_command_is_found_by_its_name() {
        let name = |line: &str| workflow_command(line);
        assert_eq!(name("echo \"::warning ::x\"").as_deref(), Some("warning"));
        assert_eq!(name("echo \"::add-mask::$V\"").as_deref(), Some("add-mask"));
        assert_eq!(
            name("echo '::error file=a.rs::boom'").as_deref(),
            Some("error")
        );
        assert_eq!(name("echo '::Warning::x'").as_deref(), Some("warning"));
        assert_eq!(
            name("printf 'ok\\n::warning::x'").as_deref(),
            Some("warning")
        );
        assert_eq!(name("printf 'ok\\r::error::x'").as_deref(), Some("error"));
        assert_eq!(name("rustc --print=cfg std::env::var"), None);
        assert_eq!(name("a::b"), None);
    }

    const SHA: &str = "0123456789012345678901234567890123456789";

    /// The bypasses the #1726 review found in the job rules, each a probe once let through in the
    /// secret job of [`workflow`]: every one is refused now, by the rule it slipped past.
    const JOB_BYPASSES: &[(&str, &str)] = &[
        (
            "L6",
            "      - uses: astral-sh/setup-uv@SHA\n        with:\n          enable-cache: true",
        ),
        (
            "L6",
            "      - uses: docker/build-push-action@SHA\n        with:\n          cache-from: type=gha\n          cache-to: type=gha,mode=max",
        ),
        ("L6", "      - uses: actions/setup-go@SHA"),
        (
            "L6",
            "      - uses: ruby/setup-ruby@SHA\n        with:\n          bundler-cache: true",
        ),
        ("L6", "      - uses: mozilla-actions/sccache-action@SHA"),
        (
            "L6",
            "      - uses: actions/setup-node@SHA\n        with: { cache: npm }",
        ),
        ("L6", "      - uses: ACTIONS/CACHE@SHA"),
        (
            "L6",
            "      - run: docker buildx build --cache-from type=gha .",
        ),
        ("L8", "      - run: printf 'ok\\n::warning::x\\n'"),
        ("L8", "      - run: echo '::Warning::x'"),
        ("L8", "      - run: echo \"x=1\" >> \"$GITHUB_STATE\""),
        (
            "L5",
            "      - with:\n          path: out/\n        uses: actions/upload-artifact@SHA",
        ),
        (
            "L5",
            "      - uses: actions/upload-artifact@SHA\n        with: { path: . }",
        ),
        ("L5", "      - uses: actions/upload-pages-artifact@SHA"),
        (
            "L5",
            "      - uses: actions/upload-artifact@SHA\n        with:\n          path: web/test-results/\n          include-hidden-files: 'TRUE'",
        ),
    ];

    /// The job rules' near misses the #1726 review's probes sit beside: each passes.
    const JOB_PASSING: &[&str] = &[
        "      - uses: actions/setup-go@SHA\n        with:\n          cache: false",
        "      - uses: astral-sh/setup-uv@SHA\n        with:\n          enable-cache: false",
        "      - uses: actions/setup-node@SHA\n        with: { node-version: 22 }",
        "      - run: echo done",
    ];

    #[test]
    fn every_bypass_the_job_rules_review_found_is_refused() {
        for (rule, steps) in JOB_BYPASSES {
            let text = workflow(&steps.replace("SHA", SHA), "      - run: echo ok");
            assert!(
                rules(WORKFLOW, &text).contains(rule),
                "{rule} refuses {steps:?}: {:?}",
                one(WORKFLOW, &text)
            );
        }
        for value in ["'TRUE'", "\"true\"", "True"] {
            let steps = format!(
                "      - uses: actions/upload-artifact@{SHA}\n        with:\n          path: x/\n          Include-Hidden-Files: {value}"
            );
            let found = one(WORKFLOW, &workflow(&steps, "      - run: echo ok"));
            assert!(
                found
                    .iter()
                    .any(|f| f.why == "an artifact uploads no hidden files"),
                "{value}: {found:?}"
            );
        }
        for steps in JOB_PASSING {
            let text = workflow(&steps.replace("SHA", SHA), "      - run: echo ok");
            assert_eq!(one(WORKFLOW, &text), Vec::new(), "{steps:?}");
        }
    }

    /// A job holds a secret by a `secrets.` or `secrets[` read in any case, by `id-token: write`,
    /// and by `github.token` with a write permission; a read-only `github.token` is not one. A job
    /// key with a trailing comment is a job of its own.
    #[test]
    fn a_job_holds_a_secret_by_any_of_its_shapes() {
        let job = |head: &str| {
            format!(
                "on: push\njobs:\n  open:\n    steps:\n      - run: echo ok\n  build: # the build\n{head}    steps:\n      - uses: actions/cache@{SHA}\n"
            )
        };
        for head in [
            "    env:\n      T: ${{ Secrets.TOKEN }}\n",
            "    env:\n      T: ${{ SECRETS.TOKEN }}\n",
            "    permissions:\n      id-token: write\n",
            "    permissions:\n      contents: write\n    env:\n      GH_TOKEN: ${{ github.token }}\n",
        ] {
            let text = job(head);
            assert!(rules(WORKFLOW, &text).contains(&"L6"), "{head:?}");
            assert_eq!(
                jobs(&text).iter().filter(|job| job.secret).count(),
                1,
                "{head:?}"
            );
        }
        let read_only = job(
            "    permissions:\n      contents: read\n    env:\n      GH_TOKEN: ${{ github.token }}\n",
        );
        assert_eq!(one(WORKFLOW, &read_only), Vec::new());
        assert_eq!(jobs(&read_only).len(), 2);
    }

    /// A finding names its file and its 1-based line.
    #[test]
    fn a_finding_names_its_file_and_line() {
        assert_eq!(
            one(SCRIPT, "#!/bin/sh\necho ok\nprintenv\n"),
            [Finding {
                rule: "L1",
                path: SCRIPT.to_owned(),
                line: 3,
                why: "`printenv` prints the environment",
            }]
        );
    }

    /// The repository's own workflows and scripts pass; each failing fixture appended to each of
    /// them is refused, so the guard is shown to read every real file (the seeded-bug check).
    #[test]
    fn the_repository_passes_and_every_seeded_line_is_refused() -> Result<()> {
        let real = sources(&repo_root()?)?;
        assert!(real.len() > 5, "the guard reads the workflows and scripts");
        assert_eq!(findings(&real), Vec::new());
        for (index, _) in real.iter().enumerate() {
            for (rule, line) in FAILING {
                let seeded: Vec<Source> = real
                    .iter()
                    .enumerate()
                    .map(|(at, source)| Source {
                        path: source.path.clone(),
                        text: if at == index {
                            format!("{}\n{line}\n", source.text)
                        } else {
                            source.text.clone()
                        },
                    })
                    .collect();
                assert!(
                    findings(&seeded)
                        .iter()
                        .any(|f| f.rule == *rule && f.path == real[index].path),
                    "{rule} {line:?} seeded into {}",
                    real[index].path
                );
            }
        }
        Ok(())
    }

    /// The seeded-bug check for the job rules on the real secret workflows: a cache and a
    /// runner-file write in `r2-lockcheck.yml`'s secret job, and a key in `release.yml`'s signed
    /// artifact, are each refused.
    #[test]
    fn the_real_secret_jobs_refuse_a_seeded_cache_write_and_artifact() -> Result<()> {
        let root = repo_root()?;
        let read = |path: &str| fs::read_to_string(root.join(path));
        let lockcheck = ".github/workflows/r2-lockcheck.yml";
        let release = ".github/workflows/release.yml";
        let steps = "    steps:\n";
        for (seed, rule) in [
            (
                "      - uses: actions/cache@0123456789012345678901234567890123456789\n",
                "L6",
            ),
            ("      - run: echo \"k=v\" >> \"$GITHUB_ENV\"\n", "L8"),
        ] {
            let text = read(lockcheck)?.replacen(steps, &format!("{steps}{seed}"), 1);
            assert!(rules(lockcheck, &text).contains(&rule), "{rule} {seed:?}");
        }
        let signed = read(release)?.replacen(
            "            signatures/\n",
            "            signatures/\n            cosign.key\n",
            1,
        );
        assert!(rules(release, &signed).contains(&"L5"));
        Ok(())
    }

    const PIN: &str = "0123456789abcdef0123456789abcdef01234567";

    /// L9: every `uses:` is pinned to a commit; a local action passes.
    #[test]
    fn every_action_is_pinned_to_a_commit() {
        let flow =
            |uses: &str| format!("on: push\njobs:\n  a:\n    steps:\n      - uses: {uses}\n");
        for uses in [
            format!("actions/checkout@{PIN} # v7"),
            "./.github/actions/x".to_owned(),
        ] {
            assert_eq!(rules(WORKFLOW, &flow(&uses)), Vec::<&str>::new(), "{uses}");
        }
        for uses in [
            "actions/checkout@v7",
            "actions/checkout@main",
            "actions/checkout",
        ] {
            assert_eq!(rules(WORKFLOW, &flow(uses)), ["L9"], "{uses}");
        }
    }

    /// L9's closed world: a step a secret reaches, from the step, its job or its workflow, and
    /// every step after it, runs only one `SECRET_RUNS` line or one `READ_ACTIONS` pin; a build
    /// before it passes.
    #[test]
    fn a_secret_step_runs_only_a_reviewed_line() {
        let path = ".github/workflows/r2-lockcheck.yml";
        let flow = |env: &str, job_env: &str, steps: &str| {
            format!(
                "on: workflow_dispatch\n{env}jobs:\n  a:\n    if: github.ref == 'refs/heads/main'\n\
                 {job_env}    steps:\n{steps}"
            )
        };
        let checkout = "      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1\n";
        let build = "      - run: cargo build --locked\n";
        let run = "      - run: target/debug/examples/r2_finding\n        env:\n          T: ${{ secrets.T }}\n";
        for steps in [format!("{checkout}{build}{run}"), run.to_owned()] {
            assert_eq!(one(path, &flow("", "", &steps)), Vec::new(), "{steps}");
        }
        let job_env = "    env:\n      T: ${{ secrets.T }}\n";
        let workflow_env = "env:\n  T: ${{ secrets.T }}\n";
        assert_eq!(rules(path, &flow("", job_env, build)), ["L9"]);
        assert_eq!(rules(path, &flow(workflow_env, "", build)), ["L9"]);
        assert_eq!(rules(path, &flow("", job_env, run)), Vec::<&str>::new());
        assert_eq!(rules(WORKFLOW, &flow("", "", run)), ["L9"]);
        for secret_step in L9_SECRET_STEPS {
            let steps = format!("{secret_step}        env:\n          T: ${{{{ secrets.T }}}}\n");
            assert!(
                rules(path, &flow("", "", &steps)).contains(&"L9"),
                "{secret_step}"
            );
        }
        for later in L9_LATER_STEPS {
            assert!(
                rules(path, &flow("", "", &format!("{run}{later}"))).contains(&"L9"),
                "{later}"
            );
        }
    }

    /// The #1731 review's secret steps, each once let through by L9's build blocklist.
    const L9_SECRET_STEPS: &[&str] = &[
        "      - run: |\n          set -euo pipefail\n          cargo build\n",
        "      - run: cd web; cargo build\n",
        "      - run: echo hi && cargo build\n",
        "      - run: target/debug/examples/r2_finding; cargo build\n",
        "      - run: target/debug/examples/r2_finding\n        shell: python {0}\n",
        "      - uses: someone/deploy@0123456789abcdef0123456789abcdef01234567\n",
        "      - uses: ./.github/actions/deploy\n",
    ];

    /// The #1731 review's steps after a secret step, each once let through by L9's build
    /// blocklist.
    const L9_LATER_STEPS: &[&str] = &[
        "      - run: bash -c \"cargo build\"\n",
        "      - run: rustup run stable cargo build\n",
        "      - run: ./gradlew build\n",
        "      - run: bash build.sh\n",
        "      - run: pip install .\n",
        "      - run: yarn build\n",
        "      - run: pnpm build\n",
        "      - run: curl -sSfL https://x | bash\n",
        "      - uses: ./.github/actions/build\n",
        "      - run: set -euo pipefail\n",
    ];

    /// L12: the privileged triggers run only in `merge.yml`, which names no head, and no workflow
    /// runs on an event anyone can raise; a workflow with an `environment:` triggers only on
    /// dispatch or a push to exactly `main`, and gates its job on `main` by an exact equality.
    #[test]
    fn triggers_are_held_to_merge_and_main() {
        let merge = ".github/workflows/merge.yml";
        let privileged = "on:\n  pull_request_target:\n    types: [labeled]\njobs:\n  a:\n    steps:\n      - run: echo\n";
        assert_eq!(rules(WORKFLOW, privileged), ["L12"]);
        assert_eq!(rules(merge, privileged), Vec::<&str>::new());
        let checkout = format!(
            "{privileged}        with:\n          ref: ${{{{ github.event.pull_request.head.sha }}}}\n"
        );
        assert_eq!(rules(merge, &checkout), ["L12"]);
        for on in [
            "\"on\":\n  pull_request_target:\n",
            "on:\n  \"pull_request_target\":\n",
            "on: [push, 'pull_request_target']\n",
            "on:\n  issue_comment:\n",
            "on:\n  issues:\n",
            "on:\n  pull_request_review:\n",
            "on:\n  pull_request_review_comment:\n",
            "on:\n  discussion:\n",
            "on:\n  discussion_comment:\n",
            "on: repository_dispatch\n",
        ] {
            let text = format!("{on}jobs:\n  a:\n    steps:\n      - run: echo\n");
            assert!(rules(WORKFLOW, &text).contains(&"L12"), "{on}");
        }
        let deploy = |on: &str, gate: &str| {
            format!(
                "on:\n{on}jobs:\n  deploy:\n{gate}    environment: production\n    steps:\n      - run: echo\n"
            )
        };
        let gate = "    if: github.ref == 'refs/heads/main'\n";
        let main = "  push:\n    branches: [main]\n  workflow_dispatch:\n";
        for (on, gate) in [
            (main, gate),
            ("  push:\n    branches: [\"main\"]\n", gate),
            ("  push:\n    branches:\n      - main\n", gate),
            (
                main,
                "    if: github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main'\n",
            ),
            (main, "    if: ${{ github.ref == 'refs/heads/main' }}\n"),
        ] {
            assert_eq!(
                rules(WORKFLOW, &deploy(on, gate)),
                Vec::<&str>::new(),
                "{on}{gate}"
            );
        }
        for (on, gate) in [
            (main, ""),
            ("  pull_request:\n  workflow_dispatch:\n", gate),
            ("  push:\n    branches: [dev]\n", gate),
            ("  push:\n    branches: [main, dev]\n", gate),
            (
                "  push:\n    branches:\n      - main\n      - \"**\"\n",
                gate,
            ),
            ("  push:\n", gate),
            ("  push:\n    branches: [main]\n    tags: ['v*']\n", gate),
            ("  push:\n    branches-ignore: [dev]\n", gate),
            (main, "    if: github.ref != 'refs/heads/main'\n"),
            (main, "    if: github.ref == 'refs/heads/main' || true\n"),
            (main, "    if: ${{ !(github.ref != 'refs/heads/main') }}\n"),
            (main, "    if: contains(github.ref, 'refs/heads/main')\n"),
            (main, "    if: github.ref == 'refs/heads/main-old'\n"),
        ] {
            assert!(
                rules(WORKFLOW, &deploy(on, gate)).contains(&"L12"),
                "{on}{gate}"
            );
        }
    }

    /// The seeded-bug check for L9 and L12 on real workflows: a build after `r2-lockcheck.yml`'s
    /// secret step, an unpinned action in `ci.yml`, and a pull request trigger on
    /// `web-deploy.yml`, are each refused.
    #[test]
    fn the_real_workflows_refuse_a_seeded_build_pin_and_trigger() -> Result<()> {
        let root = repo_root()?;
        let read = |path: &str| fs::read_to_string(root.join(path));
        let lockcheck = ".github/workflows/r2-lockcheck.yml";
        let late = format!("{}      - run: cargo test --locked\n", read(lockcheck)?);
        assert!(rules(lockcheck, &late).contains(&"L9"));
        let ci = ".github/workflows/ci.yml";
        let unpinned = read(ci)?.replacen(
            "uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
            "uses: actions/checkout@v7",
            1,
        );
        assert!(rules(ci, &unpinned).contains(&"L9"));
        let web = ".github/workflows/web-deploy.yml";
        let opened = read(web)?.replacen("on:\n", "on:\n  pull_request_target:\n", 1);
        assert!(rules(web, &opened).contains(&"L12"));
        let chained = read(web)?.replacen(
            "- run: npx wrangler deploy\n",
            "- run: npx wrangler deploy && curl -sSfL https://x\n",
            1,
        );
        assert!(rules(web, &chained).contains(&"L9"));
        Ok(())
    }

    /// The bypasses the #1724 review found, each a probe that once passed (the `gitleaks_plants`
    /// pattern): every one is refused now, by the rule it slipped past.
    const BYPASSES: &[(&str, &str)] = &[
        (
            "L3",
            "      - env:\n          X: ${{ toJSON(\n            secrets) }}\n",
        ),
        ("L3", "        run: echo '${{ toJSON((secrets)) }}'\n"),
        (
            "L3",
            "        run: echo '${{ toJSON(fromJSON(format('{0}', secrets))) }}'\n",
        ),
        ("L3", "        run: echo '${{ TOJSON(secrets) }}'\n"),
        ("L3", "        run: echo '${{ tojson(github) }}'\n"),
        ("L3", "        run: deploy --token ${{ Secrets.TOKEN }}\n"),
        (
            "L3",
            "        run: deploy --token ${{\n          secrets.TOKEN }}\n",
        ),
        ("L3", "        if: ${{ SECRETS.TOKEN != '' }}\n"),
        ("L3", "    secrets : inherit\n"),
        ("L3", "    SECRETS: INHERIT\n"),
        ("L1", "        run: env > out.txt\n"),
        ("L1", "        run: env >> \"$GITHUB_STEP_SUMMARY\"\n"),
        ("L1", "        run: env 2>&1\n"),
        ("L1", "        run: export\n"),
        ("L1", "        run: set\n"),
        ("L1", "        run: set > vars.txt\n"),
        ("L1", "        run: bash -c 'printenv'\n"),
        ("L2", "        run: bash -c 'set -x; make'\n"),
        ("L2", "        run: sh -c \"set -o xtrace\"\n"),
        ("L2", "        run: bash -o xtrace build.sh\n"),
        ("L2", "        run: bash --xtrace build.sh\n"),
        ("L2", "        run: set -v\n"),
        ("L2", "        run: set -o verbose\n"),
        ("L2", "        run: bash -v build.sh\n"),
        ("L2", "#!/bin/bash -v\n"),
        ("L1", "        run: FOO=1 env\n"),
        ("L1", "        run: if true; then env; fi\n"),
        ("L1", "        run: for x in 1; do printenv; done\n"),
        ("L1", "        run: { env; }\n"),
        ("L1", "        run: ! env\n"),
        ("L1", "        run: true || env\n"),
        ("L1", "        run: \\env\n"),
        ("L1", "        run: env -u HOME\n"),
        ("L1", "        run: env -C /tmp\n"),
        ("L1", "        run: env FOO=1\n"),
        ("L1", "        run: env -i FOO=1 > out.txt\n"),
        ("L1", "        run: env FOO=1 printenv\n"),
        ("L1", "        run: timeout 5 printenv\n"),
        ("L1", "        run: timeout -s KILL 5 env\n"),
        ("L1", "        run: echo | xargs env\n"),
        ("L1", "        run: echo | xargs -I x printenv\n"),
        ("L1", "        run: sudo -u runner env\n"),
        ("L1", "        run: sudo -E env\n"),
        ("L2", "        run: X=1 set -x\n"),
        ("L2", "        run: env bash -x build.sh\n"),
        (
            "L3",
            "        run: echo '${{ format('}}', toJSON(secrets)) }}'\n",
        ),
        ("L3", "        run: echo '${{ secrets }}'\n"),
        ("L3", "      - env:\n          X: ${{ secrets }}\n"),
        (
            "L3",
            "      - env:\n          X: ${{ join(secrets, ',') }}\n",
        ),
        (
            "L3",
            "      - env:\n          X: ${{ format('{0}', secrets) }}\n",
        ),
        ("L3", "      - env:\n          X: ${{ secrets.* }}\n"),
        ("L3", "        run: echo '${{ toJSON(job) }}'\n"),
        ("L3", "        run: echo '${{ toJSON(strategy) }}'\n"),
        ("L2", "          SHELLOPTS: xtrace\n"),
        ("L2", "        run: export SHELLOPTS=xtrace\n"),
        ("L2", "          BASH_XTRACEFD: 1\n"),
        ("L2", "        run: BASH_ENV=./trace.sh make\n"),
        ("L7", "        run: curl -ualice:$PW https://x\n"),
        ("L7", "        run: curl -sSu alice:$PW https://x\n"),
        (
            "L7",
            "        run: curl -sSH \"Authorization: Bearer $T\" https://x\n",
        ),
        ("L7", "        run: curl -D- https://x\n"),
        ("L7", "        run: curl -Dh.txt https://x\n"),
        ("L7", "        run: curl -sSfLv https://x\n"),
        ("L7", "        run: curl --dump-header=h.txt https://x\n"),
        ("L8", "        run: echo '::ADD-MASK::x'\n"),
        ("L5", "        run: gh release upload v1 dist/x\n"),
    ];

    #[test]
    fn every_bypass_the_review_found_is_refused() {
        for (rule, text) in BYPASSES {
            assert!(
                rules(WORKFLOW, text).contains(rule),
                "{rule} refuses {text:?}: {:?}",
                one(WORKFLOW, text)
            );
        }
        for text in [
            "        run: set -e\n",
            "        run: set +x\n",
            "        run: bash -c 'make build'\n",
            "        run: export X=1\n",
            "        run: bash -e build.sh\n",
            "        run: env -i PATH=/usr/bin make > build.log\n",
            "        if: github.ref == 'refs/heads/main'\n",
            "      - env:\n          X: ${{\n            secrets.TOKEN }}\n",
            "        run: env -i PATH=/usr/bin make\n",
            "        run: timeout 5 make test\n",
            "        run: if [ -n \"$X\" ]; then make; fi\n",
            "        run: echo \"${HOME}/bin\" | xargs -n 1 ls\n",
            "        run: sudo -u runner make\n",
            "        if: contains(github.event.head_commit.message, 'secrets')\n",
            "        run: echo '${{ inputs.secrets }}'\n",
            "        run: curl -sSfL -o out.json https://x\n",
            "        run: curl -sSfL -H @headers.txt https://x\n",
            "        run: curl -sSfLo out.json --retry 5 https://x\n",
            "        run: gh release view v1\n",
        ] {
            assert_eq!(one(WORKFLOW, text), Vec::new(), "{text:?}");
        }
    }

    /// An allowlist entry is bound to its file: the reviewed `merge.yml` line, written in any
    /// other workflow, is refused.
    #[test]
    fn an_allowed_line_is_allowed_only_in_its_file() {
        let line =
            "          RUN_PRS: ${{ toJSON(github.event.workflow_run.pull_requests.*.number) }}\n";
        assert_eq!(
            rules(".github/workflows/merge.yml", line),
            Vec::<&str>::new()
        );
        assert_eq!(rules(".github/workflows/ci.yml", line), ["L3"]);
    }

    /// Every `SECRET_RUNS` line is a `run:` its workflow still holds, so a stale entry is caught.
    #[test]
    fn every_secret_run_is_still_there() -> Result<()> {
        let root = repo_root()?;
        for (path, run) in SECRET_RUNS {
            let text = fs::read_to_string(root.join(path))?;
            assert!(
                text.lines().any(|l| l
                    .trim()
                    .trim_start_matches("- ")
                    .strip_prefix("run:")
                    .is_some_and(|value| value.trim() == *run)),
                "{path}: {run}"
            );
        }
        Ok(())
    }

    /// Every allowlisted line is still in its file, so a stale entry is caught.
    #[test]
    fn every_allowed_line_is_still_there() -> Result<()> {
        let root = repo_root()?;
        for (rule, path, line) in ALLOWED {
            let text = fs::read_to_string(root.join(path))?;
            assert!(
                text.lines().any(|l| l.trim() == *line),
                "{rule} {path}: {line}"
            );
        }
        Ok(())
    }
}
