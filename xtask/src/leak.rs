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

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// The directories the guard reads, under the repository root.
const GUARDED_DIRS: [&str; 2] = [".github/workflows", ".github/scripts"];

/// The lines a person has read and found to leak nothing, by rule, file, and trimmed line, matched
/// byte for byte:
/// - `no-credentials.sh` lists the environment's names, never its values, and prints only the
///   names that look like a credential's (DEC-964).
/// - `merge.yml`'s `RUN_PRS` is the JSON list of the pull request numbers a `workflow_run` names,
///   which the event holds in public.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "L1",
        ".github/scripts/no-credentials.sh",
        "found=$(compgen -e | grep -E -i 'KEY|SECRET|TOKEN|PASSWORD|PASSWD|CREDENTIAL|ALPACA|APCA|ROBINHOOD' || true)",
    ),
    (
        "L3",
        ".github/workflows/merge.yml",
        "RUN_PRS: ${{ toJSON(github.event.workflow_run.pull_requests.*.number) }}",
    ),
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
    for source in sources {
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
                    }
                }
                hits.extend(line_dump(trimmed).map(|why| ("L1", why)));
                hits.extend(line_trace(trimmed).map(|why| ("L2", why)));
                hits.extend(inherits(trimmed).map(|why| ("L3", why)));
            }
            for (rule, why) in hits {
                if !ALLOWED.contains(&(rule, source.path.as_str(), trimmed)) {
                    found.push(Finding {
                        rule,
                        path: source.path.clone(),
                        line: index.saturating_add(1),
                        why,
                    });
                }
            }
            if opens_env {
                env_block = Some(indent);
            }
        }
        let lines: Vec<&str> = source.text.lines().collect();
        for (index, expression) in expressions(&source.text) {
            let in_env = env_lines.get(index).copied().unwrap_or(false);
            let trimmed = lines.get(index).map(|line| line.trim()).unwrap_or_default();
            if let Some(why) = expression_rule(&expression, in_env)
                && !ALLOWED.contains(&("L3", source.path.as_str(), trimmed))
            {
                found.push(Finding {
                    rule: "L3",
                    path: source.path.clone(),
                    line: index.saturating_add(1),
                    why,
                });
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
