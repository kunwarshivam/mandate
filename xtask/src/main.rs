//! Repository automation. Every CI job is one `cargo xtask ci <job>` so that CI, a laptop, and an
//! air-gapped rebuild run the same checks (ADR-0001 ES-12).

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  check                 run every per-PR job locally
  ci <job>              run one CI job: fast | full | nightly, or one part: lint | test | spec-guard |
                        refcases | reference | supply-chain | mutants
  layers                check crate layering and safety-critical policy (xtask/layers.toml)
  markers               check for debt markers and #[ignore] without a pending story
  feature-map           check the verification skill's feature map against the workspace
  deps                  check every direct dependency against docs/dependencies.md
  refcases [--write]    export reference-case YAML to fixtures/refcases (drift check unless --write)
";

/// The two required checks (DEC-76), so each pays the setup cost once.
const FAST_JOB: [&str; 3] = ["lint", "test", "spec-guard"];
const FULL_JOB: [&str; 4] = ["refcases", "reference", "supply-chain", "mutants"];
const PR_JOBS: [&str; 7] = [
    "lint",
    "test",
    "refcases",
    "reference",
    "supply-chain",
    "spec-guard",
    "mutants",
];

/// Lint header every safety-critical crate's `src/lib.rs` must carry (ADR-0001 ES-09, ES-21).
const REQUIRED_HEADER: &str = "#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]";

/// Paths whose changes need a DEC ID and may not ship with code in the same change (ES-22).
const PROTECTED_PATHS: [&str; 5] = [
    "docs/specs/",
    "schemas/",
    "reference/",
    "fixtures/refcases/",
    "crates/mandate-refcases/status.toml",
];
const CODE_PATHS: [&str; 2] = ["crates/", "xtask/src/"];

/// The verification skill's map of features to code, tests, and commands (AGENTS.md).
const FEATURE_MAP: &str = ".cursor/skills/verify-mandate/feature-map.md";
/// Top-level entries a feature-map path may start with.
const REPO_ROOTS: [&str; 11] = [
    ".cargo/",
    ".cursor/",
    ".github/",
    "crates/",
    "docs/",
    "fixtures/",
    "python/",
    "reference/",
    "schemas/",
    "xtask/",
    "AGENTS.md",
];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xtask: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let root = repo_root()?;
    env::set_current_dir(&root).context("entering the repository root")?;
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check"] => {
            for job in PR_JOBS {
                ci(job)?;
            }
            Ok(())
        }
        ["ci", job] => ci(job),
        ["layers"] => layers(),
        ["markers"] => markers(),
        ["feature-map"] => feature_map(),
        ["deps"] => deps(),
        ["refcases"] => refcases(false),
        ["refcases", "--write"] => refcases(true),
        _ => {
            eprint!("{USAGE}");
            bail!("unknown command: {}", args.join(" "))
        }
    }
}

fn ci(job: &str) -> Result<()> {
    eprintln!("==> ci {job}");
    match job {
        "lint" => {
            sh("cargo", &["fmt", "--all", "--check"])?;
            sh(
                "cargo",
                &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--locked",
                    "--",
                    "-D",
                    "warnings",
                ],
            )?;
            layers()?;
            markers()?;
            feature_map()?;
            sh("typos", &[])?;
            uv_tools(&["ruff", "check", "."])?;
            uv_tools(&["ruff", "format", "--check", "."])
        }
        "test" => {
            sh(
                "cargo",
                &[
                    "nextest",
                    "run",
                    "--workspace",
                    "--locked",
                    "--no-tests=pass",
                ],
            )?;
            if workspace_has_library()? {
                sh("cargo", &["test", "--workspace", "--locked", "--doc"])?;
            }
            uv_tools(&["pytest", "-q"])
        }
        "refcases" => refcases(false),
        "reference" => {
            reference(&["generate.py"])?;
            sh(
                "git",
                &[
                    "diff",
                    "--exit-code",
                    "--",
                    "docs/specs/reference-cases/mandate.yaml",
                ],
            )
            .context(
                "reference/mandate/generate.py changed mandate.yaml; regenerate and commit it",
            )?;
            reference(&["check_cases.py"])?;
            for seed in ["1", "2", "3"] {
                reference(&["fuzz.py", seed])?;
            }
            Ok(())
        }
        "supply-chain" => {
            sh("cargo", &["deny", "--locked", "check"])?;
            deps()?;
            match base_ref() {
                Some(base) => sh(
                    "gitleaks",
                    &[
                        "git",
                        "--no-banner",
                        "--redact",
                        &format!("--log-opts={base}..HEAD"),
                    ],
                ),
                None => sh("gitleaks", &["dir", "--no-banner", "--redact", "."]),
            }?;
            gitleaks_exceptions()
        }
        "spec-guard" => {
            commit_trailers()?;
            spec_guard()
        }
        "mutants" => mutants(),
        "fast" => {
            for part in FAST_JOB {
                ci(part)?;
            }
            Ok(())
        }
        "full" => {
            for part in FULL_JOB {
                ci(part)?;
            }
            Ok(())
        }
        "nightly" => {
            for part in PR_JOBS {
                ci(part)?;
            }
            for seed in 1..=10 {
                reference(&["fuzz.py", &seed.to_string()])?;
            }
            reference(&["mutants.py"])?;
            sh("gitleaks", &["git", "--no-banner", "--redact"])
        }
        other => bail!("unknown CI job: {other}"),
    }
}

fn sh(program: &str, args: &[&str]) -> Result<()> {
    eprintln!("    $ {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("starting `{program}` (is it installed? see AGENTS.md)"))?;
    if !status.success() {
        bail!("`{program} {}` failed with {status}", args.join(" "));
    }
    Ok(())
}

fn output(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("starting `{program}`"))?;
    if !out.status.success() {
        bail!(
            "`{program} {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8(out.stdout).context("non-UTF-8 output")
}

#[derive(Deserialize)]
struct Finding {
    #[serde(rename = "File")]
    file: String,
    #[serde(rename = "RuleID")]
    rule_id: String,
}

/// Cases for the `.gitleaks.toml` exceptions (DEC-89): a one-line file, and the rule that must
/// report it, or `None` where an exception must allow it. An exception allows its text only in the
/// files it names, and every other rule still applies there. The values are assembled at run time
/// so that this source matches no rule.
fn gitleaks_plants() -> Vec<(String, String, Option<&'static str>)> {
    let page = concat!(
        "U1BZfDIwMjYtMDktMjRUMTQ6MDA6",
        "fFBMQU5URUR8MTIzNDU2Nzg5MA=="
    );
    let json = format!("{{\"bars\":[],\"next_page_token\":\"{page}\"}}");
    let query = format!("/v2/stocks/bars?symbols=SPY&page_token={page}");
    let fixture = |name| format!("crates/mandate-marketdata/tests/fixtures/alpaca/planted/{name}");
    let key_id = format!("PK{}", "PLANTEDKEYID000000");
    let generic = Some("generic-api-key");
    vec![
        (fixture("page-1.json"), json.clone(), None),
        (fixture("requests.txt"), query.clone(), None),
        (fixture("page-2.json"), key_id, Some("alpaca-key-id")),
        (fixture("notes.json"), json.clone(), generic),
        ("stray.json".to_owned(), json, generic),
        ("stray.txt".to_owned(), query, generic),
    ]
}

/// Scans [`gitleaks_plants`], written to a temporary directory, with the repository's
/// `.gitleaks.toml`: exactly the expected findings must be reported.
fn gitleaks_exceptions() -> Result<()> {
    let dir = env::temp_dir().join(format!("mandate-gitleaks-{}", std::process::id()));
    let report_path = dir.with_extension("json");
    let mut expected = BTreeSet::new();
    for (path, line, rule) in gitleaks_plants() {
        let file = dir.join(&path);
        fs::create_dir_all(file.parent().context("planted file has no parent")?)?;
        fs::write(&file, format!("{line}\n"))?;
        expected.extend(rule.map(|rule| (path, rule.to_owned())));
    }
    let config = repo_root()?.join(".gitleaks.toml");
    eprintln!("    $ (in a temporary directory) gitleaks dir --config .gitleaks.toml .");
    let status = Command::new("gitleaks")
        .current_dir(&dir)
        .args([
            "dir",
            "--no-banner",
            "--exit-code",
            "0",
            "--report-format",
            "json",
        ])
        .arg("--config")
        .arg(&config)
        .arg("--report-path")
        .arg(&report_path)
        .arg(".")
        .status()
        .context("starting `gitleaks` (is it installed? see AGENTS.md)")?;
    let report_text = fs::read_to_string(&report_path);
    fs::remove_dir_all(&dir)?;
    if !status.success() {
        bail!("gitleaks failed on the planted cases with {status}");
    }
    fs::remove_file(&report_path)?;
    let findings: Vec<Finding> = serde_json::from_str(&report_text?)?;
    let found: BTreeSet<(String, String)> =
        findings.into_iter().map(|f| (f.file, f.rule_id)).collect();
    let mut problems: Vec<String> = (expected.difference(&found))
        .map(|(file, rule)| format!("{file}: not reported by {rule}"))
        .collect();
    problems.extend(
        (found.difference(&expected)).map(|(file, rule)| format!("{file}: reported by {rule}")),
    );
    report(problems, "gitleaks-exceptions")?;
    eprintln!(
        "    gitleaks-exceptions: {} planted findings reported",
        expected.len()
    );
    Ok(())
}

/// Runs a tool from the `python/` uv workspace (ruff, pytest, the exporter).
fn uv_tools(args: &[&str]) -> Result<()> {
    let mut full = vec!["run", "--locked", "--directory", "python"];
    full.extend_from_slice(args);
    sh("uv", &full)
}

/// Runs a frozen reference script in its own pinned environment (ADR-0001 ES-10).
fn reference(script_and_args: &[&str]) -> Result<()> {
    let Some((script, rest)) = script_and_args.split_first() else {
        bail!("no reference script given");
    };
    let path = format!("reference/mandate/{script}");
    let mut full = vec![
        "run",
        "--no-project",
        "--python",
        "3.14",
        "--with-requirements",
        "reference/mandate/requirements.txt",
        "python",
        path.as_str(),
    ];
    full.extend_from_slice(rest);
    sh("uv", &full)
}

fn repo_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .context("xtask has no parent directory")
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<Dependency>,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
    path: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Target {
    kind: Vec<String>,
}

fn metadata() -> Result<Metadata> {
    let json = output(
        "cargo",
        &["metadata", "--format-version", "1", "--no-deps", "--locked"],
    )?;
    serde_json::from_str(&json).context("parsing cargo metadata")
}

fn workspace_packages(meta: &Metadata) -> Vec<&Package> {
    let members: BTreeSet<&str> = meta.workspace_members.iter().map(String::as_str).collect();
    meta.packages
        .iter()
        .filter(|p| members.contains(p.id.as_str()))
        .collect()
}

fn workspace_has_library() -> Result<bool> {
    let meta = metadata()?;
    Ok(workspace_packages(&meta)
        .iter()
        .any(|p| p.targets.iter().any(|t| t.kind.iter().any(|k| k == "lib"))))
}

#[derive(Deserialize)]
struct Layers {
    impure_crates: Vec<String>,
    crates: BTreeMap<String, CratePolicy>,
}

#[derive(Deserialize)]
struct CratePolicy {
    layer: toml::Value,
    safety_critical: bool,
    pure: bool,
    #[serde(default)]
    allowed_external: Vec<String>,
}

enum Layer {
    Product(i64),
    Tool,
}

fn layer_of(policy: &CratePolicy, name: &str) -> Result<Layer> {
    match &policy.layer {
        toml::Value::Integer(n) => Ok(Layer::Product(*n)),
        toml::Value::String(s) if s == "tool" => Ok(Layer::Tool),
        other => bail!("xtask/layers.toml: `{name}` has invalid layer {other}"),
    }
}

fn layers() -> Result<()> {
    eprintln!("    layers: checking xtask/layers.toml against cargo metadata");
    let policy: Layers = toml::from_str(&fs::read_to_string("xtask/layers.toml")?)
        .context("parsing xtask/layers.toml")?;
    let meta = metadata()?;
    let packages = workspace_packages(&meta);
    let names: BTreeSet<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    let mut problems = Vec::new();

    for listed in policy.crates.keys() {
        if !names.contains(listed.as_str()) {
            problems.push(format!(
                "`{listed}` is in layers.toml but is not a workspace member"
            ));
        }
    }
    for pkg in &packages {
        let Some(own) = policy.crates.get(&pkg.name) else {
            problems.push(format!("`{}` has no entry in xtask/layers.toml", pkg.name));
            continue;
        };
        let own_layer = layer_of(own, &pkg.name)?;
        for dep in &pkg.dependencies {
            let internal = dep.path.is_some() && names.contains(dep.name.as_str());
            if internal {
                let Some(dep_policy) = policy.crates.get(&dep.name) else {
                    continue;
                };
                match (&own_layer, layer_of(dep_policy, &dep.name)?) {
                    (Layer::Product(_), Layer::Tool) => {
                        problems.push(format!(
                            "`{}` depends on tool crate `{}`",
                            pkg.name, dep.name
                        ));
                    }
                    (Layer::Product(a), Layer::Product(b))
                        if b >= *a && dep.kind.as_deref() != Some("dev") =>
                    {
                        problems.push(format!("`{}` (layer {a}) depends on `{}` (layer {b}); dependencies must point to lower layers", pkg.name, dep.name));
                    }
                    _ => {}
                }
                continue;
            }
            if own.pure && policy.impure_crates.contains(&dep.name) {
                problems.push(format!(
                    "pure crate `{}` depends on `{}`",
                    pkg.name, dep.name
                ));
            }
            if own.safety_critical
                && dep.kind.as_deref() != Some("dev")
                && !own.allowed_external.contains(&dep.name)
            {
                problems.push(format!("safety-critical crate `{}` depends on `{}`, which is not in its allowed_external list", pkg.name, dep.name));
            }
        }
        if own.safety_critical {
            let lib = pkg
                .manifest_path
                .parent()
                .map(|d| d.join("src/lib.rs"))
                .context("manifest without a directory")?;
            let text =
                fs::read_to_string(&lib).with_context(|| format!("reading {}", lib.display()))?;
            if !text.contains(REQUIRED_HEADER) {
                problems.push(format!(
                    "{} must contain the safety-critical lint header (xtask REQUIRED_HEADER)",
                    lib.display()
                ));
            }
        }
    }
    report(problems, "layers")
}

/// Rows of `docs/dependencies.md` shaped `| `name` | ecosystem | ...`.
fn registry() -> Result<BTreeSet<(String, String)>> {
    let text =
        fs::read_to_string("docs/dependencies.md").context("reading docs/dependencies.md")?;
    let mut entries = BTreeSet::new();
    for line in text.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if let [_, name, ecosystem, ..] = cells.as_slice()
            && let Some(name) = name.strip_prefix('`').and_then(|n| n.strip_suffix('`'))
        {
            entries.insert((ecosystem.to_string(), name.to_string()));
        }
    }
    Ok(entries)
}

fn python_direct_dependencies() -> Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    for manifest in [
        "python/pyproject.toml",
        "python/mandate_tools/pyproject.toml",
    ] {
        let doc: toml::Value = toml::from_str(&fs::read_to_string(manifest)?)
            .with_context(|| format!("parsing {manifest}"))?;
        let mut lists = Vec::new();
        if let Some(deps) = doc
            .get("project")
            .and_then(|p| p.get("dependencies"))
            .and_then(toml::Value::as_array)
        {
            lists.push(deps.clone());
        }
        if let Some(groups) = doc.get("dependency-groups").and_then(toml::Value::as_table) {
            lists.extend(groups.values().filter_map(toml::Value::as_array).cloned());
        }
        for spec in lists.iter().flatten().filter_map(toml::Value::as_str) {
            let name: String = spec
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                .collect();
            if !name.is_empty() && name != "mandate-tools" {
                found.insert(name.to_lowercase());
            }
        }
    }
    Ok(found)
}

fn deps() -> Result<()> {
    eprintln!("    deps: checking direct dependencies against docs/dependencies.md");
    let registered = registry()?;
    let meta = metadata()?;
    let packages = workspace_packages(&meta);
    let internal: BTreeSet<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    let mut problems = Vec::new();
    for pkg in &packages {
        for dep in pkg
            .dependencies
            .iter()
            .filter(|d| !internal.contains(d.name.as_str()))
        {
            if !registered.contains(&("cargo".to_string(), dep.name.clone())) {
                problems.push(format!(
                    "cargo dependency `{}` (used by `{}`) has no entry in docs/dependencies.md",
                    dep.name, pkg.name
                ));
            }
        }
    }
    for name in python_direct_dependencies()? {
        if !registered.contains(&("python".to_string(), name.clone())) {
            problems.push(format!(
                "python dependency `{name}` has no entry in docs/dependencies.md"
            ));
        }
    }
    report(problems, "deps")
}

fn refcases(write: bool) -> Result<()> {
    if write {
        return uv_tools(&[
            "python",
            "-m",
            "mandate_tools.export_refcases",
            "--out",
            "../fixtures/refcases",
        ]);
    }
    let tmp = env::temp_dir().join(format!("mandate-refcases-{}", std::process::id()));
    fs::create_dir_all(&tmp)?;
    let tmp_str = tmp.to_str().context("non-UTF-8 temp path")?;
    uv_tools(&[
        "python",
        "-m",
        "mandate_tools.export_refcases",
        "--out",
        tmp_str,
    ])?;
    let mut problems = Vec::new();
    let expected: BTreeSet<String> = fs::read_dir(&tmp)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let committed: BTreeSet<String> = fs::read_dir("fixtures/refcases")
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    for name in expected.union(&committed) {
        let fresh = fs::read(tmp.join(name)).ok();
        let saved = fs::read(Path::new("fixtures/refcases").join(name)).ok();
        if fresh != saved {
            problems.push(format!(
                "fixtures/refcases/{name} is out of date; run `cargo xtask refcases --write`"
            ));
        }
    }
    fs::remove_dir_all(&tmp).ok();
    report(problems, "refcases")
}

/// Debt markers get copied as precedent, `#[ignore]` without a pending story hides a test forever,
/// and plain comments become the justification agents copy for workarounds (AGENTS.md, "The trust
/// ladder"; DEC-80).
fn markers() -> Result<()> {
    eprintln!("    markers: checking for debt markers and unexplained #[ignore]");
    let debt = [
        concat!("TO", "DO"),
        concat!("FIX", "ME"),
        concat!("X", "XX"),
        concat!("HA", "CK"),
    ];
    let files = output(
        "git",
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "crates",
            "xtask/src",
        ],
    )?;
    let mut problems = Vec::new();
    for file in files.lines().filter(|f| f.ends_with(".rs")) {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for n in plain_comment_lines(&text) {
            problems.push(format!(
                "{file}:{n}: plain comment; put the reason in a name, a type, a test, an assertion \
                 message, or the item's doc comment (DEC-80)"
            ));
        }
        for (n, line) in (1..).zip(text.lines()) {
            for word in debt.iter().filter(|w| contains_word(line, w)) {
                problems.push(format!(
                    "{file}:{n}: `{word}` marker; fix it now or add a backlog item instead"
                ));
            }
            if line.trim_start().starts_with("#[ignore") && !is_pending_marker(line) {
                problems.push(format!(
                    "{file}:{n}: #[ignore] must be `#[ignore = \"pending E<n>-<n>\"]` (DEC-77)"
                ));
            }
        }
    }
    report(problems, "markers")
}

/// Line numbers (1-based) where a plain `//` or `/* */` comment starts, skipping doc comments
/// (`///`, `//!`, `/** */`, `/*! */`) and anything inside string, raw-string, or char literals.
fn plain_comment_lines(src: &str) -> Vec<usize> {
    let chars: Vec<char> = src.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let is_ident = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let raw_prefix = |before: Option<char>, before_that: Option<char>| {
        !is_ident(before) || (before == Some('b') && !is_ident(before_that))
    };
    let mut found = Vec::new();
    let (mut i, mut line) = (0, 1);
    while let Some(c) = at(i) {
        match (c, at(i + 1)) {
            ('\n', _) => line += 1,
            ('/', Some('/')) => {
                let doc =
                    (at(i + 2) == Some('/') && at(i + 3) != Some('/')) || at(i + 2) == Some('!');
                if !doc {
                    found.push(line);
                }
                while at(i).is_some_and(|c| c != '\n') {
                    i += 1;
                }
                continue;
            }
            ('/', Some('*')) => {
                let doc =
                    (at(i + 2) == Some('*') && at(i + 3) != Some('/')) || at(i + 2) == Some('!');
                if !doc {
                    found.push(line);
                }
                let mut depth = 0usize;
                while let Some(c) = at(i) {
                    match (c, at(i + 1)) {
                        ('/', Some('*')) => {
                            depth += 1;
                            i += 1;
                        }
                        ('*', Some('/')) => {
                            depth -= 1;
                            i += 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        ('\n', _) => line += 1,
                        _ => {}
                    }
                    i += 1;
                }
                continue;
            }
            ('r', _)
                if raw_prefix(i.checked_sub(1).and_then(at), i.checked_sub(2).and_then(at)) =>
            {
                let mut j = i + 1;
                while at(j) == Some('#') {
                    j += 1;
                }
                if at(j) == Some('"') {
                    let closing: Vec<char> = std::iter::once('"')
                        .chain(std::iter::repeat_n('#', j - i - 1))
                        .collect();
                    i = j + 1;
                    while i < chars.len() && !chars[i..].starts_with(&closing) {
                        if chars[i] == '\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                    i += closing.len();
                    continue;
                }
            }
            ('"', _) => {
                i += 1;
                while let Some(c) = at(i) {
                    match c {
                        '\\' => i += 1,
                        '"' => break,
                        '\n' => line += 1,
                        _ => {}
                    }
                    i += 1;
                }
            }
            ('\'', Some('\\')) => {
                i += 2;
                while at(i).is_some_and(|c| c != '\'') {
                    i += 1;
                }
            }
            ('\'', Some(_)) if at(i + 2) == Some('\'') => i += 2,
            _ => {}
        }
        i += 1;
    }
    found
}

fn contains_word(line: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    line.match_indices(word).any(|(i, _)| {
        let before = line[..i].chars().next_back().is_none_or(|c| !is_word(c));
        let after = line[i + word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !is_word(c));
        before && after
    })
}

fn is_pending_marker(line: &str) -> bool {
    let Some(story) = line
        .trim()
        .strip_prefix("#[ignore = \"pending E")
        .and_then(|rest| rest.strip_suffix("\"]"))
    else {
        return false;
    };
    matches!(story.split_once('-'), Some((epic, n))
        if !epic.is_empty() && !n.is_empty()
            && epic.chars().chain(n.chars()).all(|c| c.is_ascii_digit()))
}

/// The feature map names every crate and reference-case suite, and every path it names exists.
fn feature_map() -> Result<()> {
    eprintln!("    feature-map: checking {FEATURE_MAP} against the workspace");
    let text = fs::read_to_string(FEATURE_MAP).with_context(|| format!("reading {FEATURE_MAP}"))?;
    let mut problems = Vec::new();
    for pkg in workspace_packages(&metadata()?) {
        if !text.contains(&format!("`{}`", pkg.name)) {
            problems.push(format!("crate `{}` has no entry", pkg.name));
        }
    }
    for entry in fs::read_dir("fixtures/refcases")? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if !text.contains(&format!("fixtures/refcases/{name}")) {
            problems.push(format!("fixtures/refcases/{name} has no entry"));
        }
    }
    for path in backticked_paths(&text) {
        if !Path::new(path).exists() {
            problems.push(format!("names `{path}`, which does not exist"));
        }
    }
    report(problems, "feature-map")
}

/// Code spans that look like repository paths: no spaces or globs, starting at a known root.
fn backticked_paths(text: &str) -> impl Iterator<Item = &str> {
    text.split('`').skip(1).step_by(2).filter(|span| {
        !span.contains(char::is_whitespace)
            && !span.contains(['*', '<', '{'])
            && REPO_ROOTS.iter().any(|root| span.starts_with(root))
    })
}

/// Mutation testing on the diff of safety-critical library crates (ADR-0001 ES-11, ES-12). Every
/// mutant in changed source must be caught; approved exclusions live in `.cargo/mutants.toml`.
fn mutants() -> Result<()> {
    let Some(base) = base_ref() else {
        eprintln!("    mutants: HEAD is the base; nothing to check");
        return Ok(());
    };
    let dirs = mutated_source_dirs()?;
    let changed = output("git", &["diff", "--name-only", &format!("{base}...HEAD")])?;
    let touched: Vec<&str> = changed
        .lines()
        .filter(|f| f.ends_with(".rs") && dirs.iter().any(|d| f.starts_with(d.as_str())))
        .collect();
    if touched.is_empty() {
        eprintln!("    mutants: no safety-critical library source changed");
        return Ok(());
    }
    let mut args = vec!["diff".to_owned(), format!("{base}...HEAD"), "--".to_owned()];
    args.extend(touched.iter().map(|f| (*f).to_owned()));
    let diff = output("git", &args.iter().map(String::as_str).collect::<Vec<_>>())?;
    let diff_file = env::temp_dir().join(format!("mandate-mutants-{}.diff", std::process::id()));
    fs::write(&diff_file, diff)?;
    let diff_path = diff_file
        .to_str()
        .context("non-UTF-8 temp path")?
        .to_owned();
    let result = sh(
        "cargo",
        &[
            "mutants",
            "--in-diff",
            &diff_path,
            "--test-tool",
            "nextest",
            "--jobs",
            "2",
            "--output",
            "target",
        ],
    );
    fs::remove_file(&diff_file).ok();
    result
}

/// `src/` of every safety-critical product crate. The reference-case harness (a tool crate) is
/// excluded: its checks are proven by bugs seeded in the code it tests, not by mutating it. So is a
/// crate with `#[ignore = "pending <story>"]` tests: in a tests PR its code is stubs that no live
/// test runs, and the implementation PR, which deletes those markers and replaces every stub body,
/// passes the mutation gate instead (ADR-0001 ES-15, DEC-83).
fn mutated_source_dirs() -> Result<Vec<String>> {
    let policy: Layers = toml::from_str(&fs::read_to_string("xtask/layers.toml")?)
        .context("parsing xtask/layers.toml")?;
    let root = env::current_dir()?;
    let mut dirs = Vec::new();
    for pkg in workspace_packages(&metadata()?) {
        let Some(own) = policy.crates.get(&pkg.name) else {
            continue;
        };
        if own.safety_critical && matches!(layer_of(own, &pkg.name)?, Layer::Product(_)) {
            let dir = pkg
                .manifest_path
                .parent()
                .context("manifest without a directory")?
                .strip_prefix(&root)
                .context("crate outside the repository")?;
            let dir = dir.display().to_string();
            if has_pending_tests(&dir)? {
                eprintln!(
                    "    mutants: skipping `{}`: it has pending tests, so its implementation PR runs the gate",
                    pkg.name
                );
            } else {
                dirs.push(format!("{dir}/src/"));
            }
        }
    }
    Ok(dirs)
}

fn has_pending_tests(dir: &str) -> Result<bool> {
    let files = output(
        "git",
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            dir,
        ],
    )?;
    for file in files.lines().filter(|f| f.ends_with(".rs")) {
        if let Ok(text) = fs::read_to_string(file)
            && text.lines().any(is_pending_marker)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The commit a change is compared against. An all-zero `MANDATE_BASE_REF` is what a forge sends
/// for a branch's first push, meaning there is no base.
fn base_ref() -> Option<String> {
    if let Ok(base) = env::var("MANDATE_BASE_REF")
        && !base.is_empty()
    {
        return (!base.chars().all(|c| c == '0')).then_some(base);
    }
    let merge_base = output("git", &["merge-base", "HEAD", "origin/main"]).ok()?;
    let head = output("git", &["rev-parse", "HEAD"]).ok()?;
    let merge_base = merge_base.trim().to_string();
    (merge_base != head.trim()).then_some(merge_base)
}

/// Commits a change adds carry no `Co-authored-by` trailer: Cursor's agent hook fills it with the
/// founder's email, which must not enter the history.
fn commit_trailers() -> Result<()> {
    let Some(base) = base_ref() else {
        return Ok(());
    };
    let log = output(
        "git",
        &[
            "log",
            "--format=%h %(trailers:key=Co-authored-by,valueonly,separator=%x2C)",
            &format!("{base}..HEAD"),
        ],
    )?;
    let problems: Vec<String> = log
        .lines()
        .filter_map(|line| line.split_once(' '))
        .filter(|(_, trailer)| !trailer.trim().is_empty())
        .map(|(sha, _)| {
            format!("commit {sha} has a Co-authored-by trailer; remove it (.cursor/install.sh disables the hook that adds it)")
        })
        .collect();
    report(problems, "commit-trailers")
}

fn spec_guard() -> Result<()> {
    let Some(base) = base_ref() else {
        eprintln!("    spec-guard: HEAD is the base; nothing to check");
        return Ok(());
    };
    let changed = output("git", &["diff", "--name-only", &format!("{base}...HEAD")])?;
    let (protected, code) = classify(changed.lines());
    if protected.is_empty() {
        eprintln!("    spec-guard: no protected paths changed");
        return Ok(());
    }
    let mut problems = Vec::new();
    let messages = format!(
        "{}\n{}",
        env::var("MANDATE_PR_BODY").unwrap_or_default(),
        output("git", &["log", "--format=%B", &format!("{base}..HEAD")])?
    );
    if !contains_dec_id(&messages) {
        problems.push(format!(
            "protected paths changed ({}) but no DEC-<n> is cited in the PR description or commits",
            protected.len()
        ));
    }
    if !code.is_empty() {
        problems.push(format!("specs, schemas, or reference cases changed together with code ({} code files); split the change", code.len()));
    }
    report(problems, "spec-guard")
}

/// Splits changed paths into protected paths and code paths. A protected file under a code
/// directory (`crates/mandate-refcases/status.toml`) counts only as protected.
fn classify<'a>(changed: impl Iterator<Item = &'a str>) -> (Vec<&'a str>, Vec<&'a str>) {
    let mut protected = Vec::new();
    let mut code = Vec::new();
    for file in changed {
        if PROTECTED_PATHS.iter().any(|p| file.starts_with(p)) {
            protected.push(file);
        } else if CODE_PATHS.iter().any(|p| file.starts_with(p)) {
            code.push(file);
        }
    }
    (protected, code)
}

fn contains_dec_id(text: &str) -> bool {
    text.match_indices("DEC-").any(|(i, _)| {
        text.get(i + 4..)
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    })
}

fn report(problems: Vec<String>, check: &str) -> Result<()> {
    if problems.is_empty() {
        return Ok(());
    }
    for p in &problems {
        eprintln!("    {check}: {p}");
    }
    bail!("{check}: {} problem(s)", problems.len())
}

#[cfg(test)]
mod tests {
    use super::{
        backticked_paths, classify, contains_dec_id, contains_word, is_pending_marker,
        plain_comment_lines,
    };

    #[test]
    fn finds_plain_comments_but_not_docs_or_literals() {
        let src = concat!(
            "//! crate docs\n",
            "/// item docs\n",
            "fn f() {} // trailing\n",
            "let url = \"https://example.com\"; let c = '/'; let r = r#\"a // b\"#;\n",
            "//// four slashes is a plain comment\n",
            "/* block */ let x = 1;\n",
            "/** doc block */ /*! inner doc */\n",
            "fn g<'a>(s: &'a str) -> char { '\\'' } // after lifetimes\n",
            "let s = \"escaped \\\" // still a string\";\n",
            "/* outer /* nested */ still comment */ let y = 2;\n",
            "let b = b\"bytes // here\"; let e = br#\"raw \" // bytes\"#;\n",
            "let w = wr\"not raw\"; // counted\n",
        );
        assert_eq!(plain_comment_lines(src), [3, 5, 6, 8, 10, 12]);
    }

    #[test]
    fn debt_markers_match_whole_words_only() {
        let marker = concat!("TO", "DO");
        assert!(contains_word(&format!("// {marker}: later"), marker));
        assert!(contains_word(marker, marker));
        assert!(!contains_word(&format!("{marker}S"), marker));
        assert!(!contains_word(&format!("my_{marker}"), marker));
    }

    #[test]
    fn ignore_needs_a_pending_story() {
        assert!(is_pending_marker("    #[ignore = \"pending E5-1\"]"));
        assert!(is_pending_marker("#[ignore = \"pending E12-30\"]"));
        assert!(!is_pending_marker("#[ignore]"));
        assert!(!is_pending_marker("#[ignore = \"slow\"]"));
        assert!(!is_pending_marker("#[ignore = \"pending E5\"]"));
        assert!(!is_pending_marker("#[ignore = \"pending E-1\"]"));
        assert!(!is_pending_marker("#[ignore = \"pending E5-x\"]"));
    }

    #[test]
    fn feature_map_paths_are_repository_paths() {
        let text =
            "`crates/a/src/lib.rs` `cargo xtask check` `docs/*.md` `mandate-journal` `AGENTS.md`";
        assert_eq!(
            backticked_paths(text).collect::<Vec<_>>(),
            ["crates/a/src/lib.rs", "AGENTS.md"]
        );
    }

    #[test]
    fn status_toml_is_protected_not_code() {
        let (protected, code) = classify(
            [
                "crates/mandate-refcases/status.toml",
                "crates/mandate-journal/src/lib.rs",
                "docs/specs/journal.md",
                "xtask/src/main.rs",
                "docs/project/06-backlog-v1.md",
            ]
            .into_iter(),
        );
        assert_eq!(
            protected,
            [
                "crates/mandate-refcases/status.toml",
                "docs/specs/journal.md"
            ]
        );
        assert_eq!(
            code,
            ["crates/mandate-journal/src/lib.rs", "xtask/src/main.rs"]
        );
    }

    #[test]
    fn finds_dec_ids() {
        assert!(contains_dec_id("Implements DEC-72."));
        assert!(!contains_dec_id("DEC- nothing"));
        assert!(!contains_dec_id("no decision"));
    }
}
