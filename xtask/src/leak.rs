//! `cargo xtask leak-guard`, part of `lint` (DEC-2032): the repository is public, so a CI log or
//! artifact is too. Each rule is a text match over `.github/workflows` and `.github/scripts` that
//! fails closed: a line it refuses stays refused until a person lists it, file and trimmed line
//! byte for byte, in [`ALLOWED`], in a change a reviewer sees.
//!
//! - **L1, environment dumps:** `printenv`, `declare`, `typeset`, a bare `export` or `export -p`,
//!   a bare `set`, a bare `env` (a redirect is not an argument), `compgen -e` or `-v`, and any
//!   `/proc/*/environ`.
//! - **L2, trace and debug output:** `set` with any flag holding `x` or `v`, or `-o xtrace` or
//!   `-o verbose`; a shell run with any of those or `--xtrace`/`--verbose`, its `-c` program read as
//!   a command of its own; a shebang with `-x` or `-v`; `PS4=`, the runner's and `gh`'s debug switches, `TF_LOG`,
//!   `RUST_LOG` at `trace`, `helm --debug`, `kubectl` above `--v=5`, `doctl -v` and `cosign -d`.
//! - **L3, contexts:** read in every GitHub expression whole (each `${{ … }}` joined across lines,
//!   and each `if:` value) and without regard to case: `toJSON(` whose argument names a context,
//!   however nested, `secrets[`, and `secrets.` anywhere but an `env:` block, so a secret reaches a
//!   step only as a variable and is never pasted into its script; and `secrets: inherit` in any
//!   case and spacing.
//! - **L5, artifacts:** each `upload-artifact` path is one of [`ARTIFACT_PATHS`], none names deploy
//!   state, a plan, a key, a secret, a token, a kubeconfig, an `.env` or auth, and none uploads
//!   hidden files.
//! - **L6, caches:** no cache is restored or saved in a secret job, a job with an `environment:` or
//!   a `secrets.` read, so nothing a pull request wrote runs beside a secret.
//! - **L7, curl and tools:** no verbose, traced or header-dumping `curl`, no `-H` or `-u` value but
//!   an `@file`, no `--aws-sigv4`, no `docker login -p`, and no `--password` but `--password-stdin`.
//! - **L8, runner files:** a secret job, and a script it runs, writes no `GITHUB_ENV`,
//!   `GITHUB_PATH`, `GITHUB_OUTPUT` or `GITHUB_STEP_SUMMARY` and issues no `::` workflow command;
//!   and nothing anywhere issues `::add-mask::` but a reviewed line that escapes its value first
//!   (`%` to `%25`, then CR to `%0D` and LF to `%0A`).
//! - **L9, the secret step's shape:** in a secret job, the step that reads `secrets.` starts with no
//!   `cargo`, `npm`, `npx` or `make`, and no step after it builds, so the secret never sits beside
//!   a build of a change's code; and every `uses:` anywhere is pinned to a 40-character commit.
//! - **L12, triggers:** `pull_request_target` and `workflow_run` only in `merge.yml`, which names no
//!   `head.sha`, `head.ref` or `head_ref`; a workflow holding an `environment:` triggers only on
//!   `workflow_dispatch` or a push to `main`, and each job with one has an `if:` naming
//!   `refs/heads/main`.

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
/// - `web-deploy.yml`'s deploy step runs `npx wrangler deploy`, the Worker deploy tool the earlier
///   `npm ci --ignore-scripts` installed from the lockfile, on `main` only; `web/scripts/workers.mjs`
///   pins the job's shape (#1700).
/// - `merge.yml`'s `RUN_PRS` is the JSON list of the pull request numbers a `workflow_run` names,
///   which the event holds in public.
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
        "L9",
        ".github/workflows/web-deploy.yml",
        "- run: npx wrangler deploy",
    ),
    (
        "L3",
        ".github/workflows/merge.yml",
        "RUN_PRS: ${{ toJSON(github.event.workflow_run.pull_requests.*.number) }}",
    ),
];

/// The programs a secret step may not start with, and no step after it may run (L9).
const BUILDS: [&str; 4] = ["cargo", "npm", "npx", "make"];

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
                    }
                }
                if trimmed.contains("::add-mask::") {
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

/// The commands of one line: split at `;`, `|`, `&`, `(`, `)` and backticks, then into words, with
/// a leading YAML key or list dash dropped and wrappers (`sudo`, `exec`, `command`, `nohup`,
/// `time`) stepped over, so each command's own name comes first. Quotes are dropped, not parsed:
/// a refused word inside a string is refused too.
fn segments(line: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for piece in line.split([';', '|', '&', '(', ')', '`']) {
        let mut words: Vec<String> = piece
            .split_whitespace()
            .map(|word| word.trim_matches(['"', '\'']).to_owned())
            .filter(|word| !word.is_empty())
            .collect();
        while words
            .first()
            .is_some_and(|word| word == "-" || word.ends_with(':') || word == "$")
        {
            words.remove(0);
        }
        while words.first().is_some_and(|word| {
            matches!(
                word.as_str(),
                "sudo" | "exec" | "command" | "nohup" | "time"
            )
        }) {
            words.remove(0);
            while words.first().is_some_and(|word| word.starts_with('-')) {
                words.remove(0);
            }
        }
        if !words.is_empty() {
            out.push(words);
        }
    }
    out
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
        "env" if args.iter().all(|a| a.starts_with('-')) => {
            Some("a bare `env` prints the environment")
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
const CONTEXTS: [&str; 9] = [
    "secrets", "env", "github", "vars", "inputs", "steps", "needs", "matrix", "runner",
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
        let end = text
            .get(body_at..)
            .and_then(|tail| tail.find("}}"))
            .map_or(text.len(), |close| body_at.saturating_add(close));
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

/// An expression lowercased, with its whitespace dropped.
fn compact(expression: &str) -> String {
    expression
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// L3 for one [`expressions`] entry, `in_env` when its line sits in an `env:` block: `toJSON(`
/// whose argument names a whole context anywhere in it, however nested, `secrets[`, and
/// `secrets.` outside an `env:` block.
fn expression_rule(expression: &str, in_env: bool) -> Option<&'static str> {
    if let Some(at) = expression.find("tojson(") {
        let argument = expression.get(at..).unwrap_or_default();
        let names_context = CONTEXTS.iter().any(|name| {
            argument.match_indices(name).any(|(found, _)| {
                let before = argument
                    .get(..found)
                    .and_then(|head| head.chars().next_back());
                let after = argument
                    .get(found.saturating_add(name.len())..)
                    .and_then(|tail| tail.chars().next());
                let word = |c: Option<char>| {
                    c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                };
                !word(before) && !word(after) && before != Some('\'')
            })
        });
        if names_context {
            return Some("`toJSON(` of a context prints every value in it");
        }
    }
    if expression.contains("secrets[") {
        return Some("`secrets[` reads a secret by a computed name");
    }
    if expression.contains("secrets.") && !in_env {
        return Some("a secret is read only in an `env:` block, never pasted into a step");
    }
    None
}

/// One job of a workflow: its lines with their 1-based numbers, and whether it holds a secret
/// (an `environment:` or a `secrets.` read).
struct Job {
    lines: Vec<(usize, String)>,
    secret: bool,
}

/// The jobs of a workflow's text: each key two spaces in under the top-level `jobs:`, up to the
/// next such key or the next top-level line.
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
            in_jobs = trimmed == "jobs:";
            continue;
        }
        if !in_jobs {
            continue;
        }
        if indent == 2 && trimmed.ends_with(':') {
            out.push(Job {
                lines: Vec::new(),
                secret: false,
            });
        }
        if let Some(job) = out.last_mut() {
            if trimmed.starts_with("environment:") || trimmed.contains("secrets.") {
                job.secret = true;
            }
            job.lines.push((index.saturating_add(1), raw.to_owned()));
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

/// L8 for a line of a secret job or of a script one runs.
fn runner_file(line: &str) -> Option<&'static str> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return None;
    }
    if [
        "GITHUB_ENV",
        "GITHUB_PATH",
        "GITHUB_OUTPUT",
        "GITHUB_STEP_SUMMARY",
    ]
    .iter()
    .any(|name| trimmed.contains(name))
    {
        return Some(
            "a secret job writes none of the runner's files, which later steps and logs read",
        );
    }
    workflow_command(trimmed)
        .filter(|name| *name != "add-mask")
        .map(|_| "a secret job issues no `::` workflow command but `::add-mask::`")
}

/// The name of the first `::name ...::` workflow command in a line: one that starts the line, a
/// word or a quoted string, so a Rust path such as `std::env::var` is not one.
fn workflow_command(line: &str) -> Option<&str> {
    let mut rest = line;
    while let Some(at) = rest.find("::") {
        let before = rest.get(..at).and_then(|head| head.chars().next_back());
        let after = rest.get(at.saturating_add(2)..).unwrap_or_default();
        if before.is_some_and(|c| !(c.is_whitespace() || c == '"' || c == '\'')) {
            rest = after;
            continue;
        }
        let name_len = after
            .find(|c: char| !(c.is_ascii_lowercase() || c == '-'))
            .unwrap_or(after.len());
        let name = after.get(..name_len).unwrap_or_default();
        let tail = after.get(name_len..).unwrap_or_default();
        if !name.is_empty()
            && (tail.starts_with("::") || (tail.starts_with(' ') && tail.contains("::")))
        {
            return Some(name);
        }
        rest = after;
    }
    None
}

/// L5, L6 and L8 for a workflow: what its jobs and steps hold, by rule, line number, text and why.
fn job_findings(path: &str, text: &str) -> Vec<(&'static str, usize, String, &'static str)> {
    let mut out = Vec::new();
    for job in jobs(text) {
        if job.secret {
            out.extend(step_shape(&job));
        }
        let mut in_upload: Option<usize> = None;
        let mut paths_from: Option<usize> = None;
        for (number, raw) in &job.lines {
            let trimmed = raw.trim();
            let indent = raw.len().saturating_sub(raw.trim_start().len());
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("- ") {
                in_upload = None;
                paths_from = None;
            }
            if trimmed.contains("actions/upload-artifact") {
                in_upload = Some(indent);
            }
            if job.secret {
                if trimmed.contains("actions/cache")
                    || trimmed.contains("rust-cache")
                    || trimmed.starts_with("cache:")
                    || trimmed.starts_with("cache-dependency-path:")
                {
                    out.push((
                        "L6",
                        *number,
                        trimmed.to_owned(),
                        "a secret job restores or saves no cache, which a pull request could have written",
                    ));
                }
                if let Some(why) = runner_file(trimmed) {
                    out.push(("L8", *number, trimmed.to_owned(), why));
                }
            }
            if in_upload.is_none() {
                continue;
            }
            if trimmed == "include-hidden-files: true" {
                out.push((
                    "L5",
                    *number,
                    trimmed.to_owned(),
                    "an artifact uploads no hidden files",
                ));
            }
            if let Some(from) = paths_from {
                if indent > from {
                    out.extend(artifact_path(path, *number, trimmed));
                    continue;
                }
                paths_from = None;
            }
            if let Some(value) = trimmed.strip_prefix("path:") {
                let value = value.trim();
                if value == "|" || value == ">" || value.is_empty() {
                    paths_from = Some(indent);
                } else {
                    out.extend(artifact_path(path, *number, value));
                }
            }
        }
    }
    out
}

/// One step of a job: its lines, whether it reads a secret, and the programs its `run:` starts.
struct Step {
    lines: Vec<(usize, String)>,
    secret: bool,
    programs: Vec<String>,
}

/// The steps of a job: each `- ` item at the indentation of the first one after `steps:`.
fn steps(job: &Job) -> Vec<Step> {
    let mut out: Vec<Step> = Vec::new();
    let mut steps_at: Option<usize> = None;
    let mut item: Option<usize> = None;
    for (number, raw) in &job.lines {
        let trimmed = raw.trim();
        let indent = raw.len().saturating_sub(raw.trim_start().len());
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "steps:" {
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
            out.push(Step {
                lines: Vec::new(),
                secret: false,
                programs: Vec::new(),
            });
        }
        if let Some(step) = out.last_mut() {
            step.secret |= trimmed.contains("secrets.");
            step.lines.push((*number, raw.to_owned()));
        }
    }
    for step in &mut out {
        let mut block: Option<usize> = None;
        let mut text: Vec<String> = Vec::new();
        for (_, raw) in &step.lines {
            let trimmed = raw.trim();
            let indent = raw.len().saturating_sub(raw.trim_start().len());
            if let Some(from) = block {
                if indent > from {
                    text.push(trimmed.to_owned());
                    continue;
                }
                block = None;
            }
            let key = trimmed.trim_start_matches("- ");
            if let Some(value) = key.strip_prefix("run:") {
                let value = value.trim();
                if value.starts_with('|') || value.starts_with('>') || value.is_empty() {
                    block = Some(indent);
                } else {
                    text.push(value.to_owned());
                }
            }
        }
        step.programs = text
            .iter()
            .flat_map(|line| segments(line))
            .map(|words| program(&words).to_owned())
            .collect();
    }
    out
}

/// L9 for a secret job: the secret step starts no build and no build follows it.
fn step_shape(job: &Job) -> Vec<(&'static str, usize, String, &'static str)> {
    let mut out = Vec::new();
    let mut after_secret = false;
    for step in steps(job) {
        let Some((number, first)) = step.lines.first() else {
            continue;
        };
        let builds = step.programs.iter().any(|p| BUILDS.contains(&p.as_str()));
        if step.secret
            && step
                .programs
                .first()
                .is_some_and(|p| BUILDS.contains(&p.as_str()))
        {
            out.push((
                "L9",
                *number,
                first.trim().to_owned(),
                "the step holding a secret starts no `cargo`, `npm`, `npx` or `make`",
            ));
        } else if after_secret && builds {
            out.push((
                "L9",
                *number,
                first.trim().to_owned(),
                "no build step follows the step holding a secret",
            ));
        }
        after_secret |= step.secret;
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
            in_on = trimmed.starts_with("on:");
            trigger = None;
            if let Some(inline) = trimmed.strip_prefix("on:").map(str::trim) {
                for name in inline
                    .trim_matches(['[', ']'])
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                {
                    triggers.push((number, name.to_owned(), String::new()));
                }
            }
            continue;
        }
        if in_on {
            if indent == 2 {
                let name = trimmed.trim_end_matches(':').to_owned();
                let name = name.split(':').next().unwrap_or_default().trim().to_owned();
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
            let main_push = name == "push"
                && (body.contains("branches: [main]") || body.contains("branches:\n- main\n"))
                && !body.contains("tags");
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
                let trimmed = line.trim();
                trimmed.starts_with("if:") && trimmed.contains("refs/heads/main")
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
        "curl" => {
            for (at, arg) in args.iter().enumerate() {
                let next = args.get(at.saturating_add(1)).map(String::as_str);
                let long = matches!(
                    arg.as_str(),
                    "--verbose" | "--include" | "--dump-header" | "--aws-sigv4"
                ) || arg.starts_with("--trace")
                    || arg.starts_with("--aws-sigv4=");
                let short = short_flag(arg)
                    && arg.chars().skip(1).all(|c| c.is_ascii_alphabetic())
                    && arg.contains(['v', 'i', 'D']);
                if long || short {
                    return Some(
                        "`curl` writes no verbose output, trace, headers or signed request",
                    );
                }
                let valued = match arg.as_str() {
                    "-H" | "--header" | "-u" | "--user" => next,
                    other => other
                        .strip_prefix("-H")
                        .or(other.strip_prefix("--header="))
                        .or(other.strip_prefix("--user="))
                        .filter(|value| !value.is_empty()),
                };
                if valued.is_some_and(|value| !value.starts_with('@')) {
                    return Some(
                        "a `curl` header or user is read from an `@file`, never written inline",
                    );
                }
            }
            None
        }
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
        assert_eq!(workflow_command("echo \"::warning ::x\""), Some("warning"));
        assert_eq!(
            workflow_command("echo \"::add-mask::$V\""),
            Some("add-mask")
        );
        assert_eq!(
            workflow_command("echo '::error file=a.rs::boom'"),
            Some("error")
        );
        assert_eq!(workflow_command("rustc --print=cfg std::env::var"), None);
        assert_eq!(workflow_command("a::b"), None);
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

    /// L9: in a secret job, the secret step starts no build, and no build follows it; a build
    /// before it, or in a job without a secret, passes.
    #[test]
    fn a_secret_step_starts_no_build_and_none_follows_it() {
        let job = |steps: &str| {
            format!(
                "on: workflow_dispatch\njobs:\n  a:\n    if: github.ref == 'refs/heads/main'\n\
                 \x20   steps:\n{steps}"
            )
        };
        let build = "      - run: cargo build --locked\n";
        let run = "      - run: target/debug/x\n        env:\n          T: ${{ secrets.T }}\n";
        let built_in_secret =
            "      - run: cargo run --locked\n        env:\n          T: ${{ secrets.T }}\n";
        assert_eq!(
            rules(WORKFLOW, &job(&format!("{build}{run}"))),
            Vec::<&str>::new()
        );
        assert_eq!(rules(WORKFLOW, &job(built_in_secret)), ["L9"]);
        assert_eq!(rules(WORKFLOW, &job(&format!("{run}{build}"))), ["L9"]);
        let block = "      - name: x\n        env:\n          T: ${{ secrets.T }}\n        run: |\n          make deploy\n";
        assert_eq!(rules(WORKFLOW, &job(block)), ["L9"]);
    }

    /// L12: the privileged triggers run only in `merge.yml`, which names no head; a workflow with
    /// an `environment:` triggers only on dispatch or a push to main, and gates its job on main.
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
        let deploy = |on: &str, gate: &str| {
            format!(
                "on:\n{on}jobs:\n  deploy:\n{gate}    environment: production\n    steps:\n      - run: echo\n"
            )
        };
        let gate = "    if: github.ref == 'refs/heads/main'\n";
        let main = "  push:\n    branches: [main]\n  workflow_dispatch:\n";
        assert_eq!(rules(WORKFLOW, &deploy(main, gate)), Vec::<&str>::new());
        assert_eq!(rules(WORKFLOW, &deploy(main, "")), ["L12"]);
        let pr = "  pull_request:\n  workflow_dispatch:\n";
        assert_eq!(rules(WORKFLOW, &deploy(pr, gate)), ["L12"]);
        let other = "  push:\n    branches: [dev]\n";
        assert_eq!(rules(WORKFLOW, &deploy(other, gate)), ["L12"]);
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
