//! `cargo xtask leak-guard`, part of `lint` (DEC-2032): the repository is public, so a CI log or
//! artifact is too. Each rule is a text match over `.github/workflows` and `.github/scripts` that
//! fails closed: a line it refuses stays refused until a person lists it, file and trimmed line
//! byte for byte, in [`ALLOWED`], in a change a reviewer sees.
//!
//! - **L1, environment dumps:** `printenv`, `declare`, `typeset`, `export -p`, a bare `env`,
//!   `compgen -e` or `-v`, and any `/proc/*/environ`.
//! - **L2, trace and debug output:** `set` with any flag holding `x` or `-o xtrace`, a shell run
//!   with `-x`, a shebang with `-x`, `PS4=`, the runner's and `gh`'s debug switches, `TF_LOG`,
//!   `RUST_LOG` at `trace`, `helm --debug`, `kubectl` above `--v=5`, `doctl -v` and `cosign -d`.
//! - **L3, contexts:** `toJSON(` of a whole context, `secrets[`, `secrets: inherit`, and
//!   `secrets.` anywhere but an `env:` block, so a secret reaches a step only as a variable and is
//!   never pasted into its script.

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
        for (index, raw) in source.text.lines().enumerate() {
            let trimmed = raw.trim();
            let indent = raw.len().saturating_sub(raw.trim_start().len());
            if env_block.is_some_and(|key| !trimmed.is_empty() && indent <= key) {
                env_block = None;
            }
            let opens_env = trimmed == "env:" || trimmed == "- env:";
            let inline_env = trimmed.starts_with("env:") || trimmed.starts_with("- env:");
            let mut hits: Vec<(&'static str, &'static str)> = Vec::new();
            if let Some(why) = shebang_trace(trimmed) {
                hits.push(("L2", why));
            }
            if !trimmed.starts_with('#') {
                for words in segments(trimmed) {
                    hits.extend(dump(&words).map(|why| ("L1", why)));
                    hits.extend(trace(&words).map(|why| ("L2", why)));
                }
                hits.extend(line_dump(trimmed).map(|why| ("L1", why)));
                hits.extend(line_trace(trimmed).map(|why| ("L2", why)));
                hits.extend(
                    context(trimmed, env_block.is_some() || inline_env).map(|why| ("L3", why)),
                );
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
    }
    found
}

/// The commands of one line: split at `;`, `|`, `&`, `(`, `)` and backticks, then into words, with
/// a leading YAML key or list dash dropped and wrappers (`sudo`, `exec`, `command`, `nohup`,
/// `time`) stepped over, so each command's own name comes first. Quotes are dropped, not parsed:
/// a refused word inside a string is refused too, which errs toward refusing.
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

/// L1 for one command.
fn dump(words: &[String]) -> Option<&'static str> {
    let args = words.get(1..).unwrap_or_default();
    match program(words) {
        "printenv" => Some("`printenv` prints the environment"),
        "declare" | "typeset" => Some("`declare` and `typeset` can print every variable"),
        "export" if args.iter().any(|a| a == "-p") => Some("`export -p` prints the environment"),
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

/// L2 for one command.
fn trace(words: &[String]) -> Option<&'static str> {
    let args = words.get(1..).unwrap_or_default();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    match program(words) {
        "set" if args.iter().any(|a| short_flag(a) && a.contains('x')) || has("xtrace") => {
            Some("`set -x` traces every command with its expanded values")
        }
        "bash" | "sh" | "zsh" | "dash" | "ksh"
            if args
                .iter()
                .take_while(|a| a.starts_with('-'))
                .any(|a| short_flag(a) && a.contains('x')) =>
        {
            Some("a shell run with `-x` traces every command with its expanded values")
        }
        "helm" if has("--debug") => Some("`helm --debug` prints rendered values"),
        "kubectl" if kubectl_verbosity(args) > 5 => {
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

/// L2 for a shebang that runs its shell with `-x`.
fn shebang_trace(line: &str) -> Option<&'static str> {
    let rest = line.strip_prefix("#!")?;
    rest.split_whitespace()
        .skip(1)
        .any(|word| short_flag(word) && word.contains('x'))
        .then_some("a shebang with `-x` traces every command with its expanded values")
}

/// The contexts whose whole value `toJSON(` must never print.
const CONTEXTS: [&str; 9] = [
    "secrets", "env", "github", "vars", "inputs", "steps", "needs", "matrix", "runner",
];

/// L3 for a line, `in_env` when it sits in an `env:` block.
fn context(line: &str, in_env: bool) -> Option<&'static str> {
    let compact: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    for (at, _) in line.match_indices("toJSON(") {
        let after = line
            .get(at.saturating_add("toJSON(".len())..)
            .unwrap_or_default()
            .trim_start();
        if CONTEXTS.iter().any(|name| {
            after.strip_prefix(name).is_some_and(|rest| {
                rest.chars()
                    .next()
                    .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            })
        }) {
            return Some("`toJSON(` of a context prints every value in it");
        }
    }
    if line.contains("secrets[") {
        return Some("`secrets[` reads a secret by a computed name");
    }
    if compact.contains("secrets: inherit") {
        return Some("`secrets: inherit` hands every secret to a called workflow");
    }
    if line.contains("secrets.") && !in_env {
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
