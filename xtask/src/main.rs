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
                        refcases | reference | supply-chain
  layers                check crate layering and safety-critical policy (xtask/layers.toml)
  deps                  check every direct dependency against docs/dependencies.md
  refcases [--write]    export reference-case YAML to fixtures/refcases (drift check unless --write)
";

const FAST_JOB: [&str; 3] = ["lint", "test", "spec-guard"];
const FULL_JOB: [&str; 3] = ["refcases", "reference", "supply-chain"];
const PR_JOBS: [&str; 6] = [
    "lint",
    "test",
    "refcases",
    "reference",
    "supply-chain",
    "spec-guard",
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
            }
        }
        "spec-guard" => spec_guard(),
        // The two required checks (DEC-76): each pays the setup cost once.
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

// ---------------------------------------------------------------- processes

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

// ---------------------------------------------------------------- cargo metadata

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

// ---------------------------------------------------------------- layers

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

// ---------------------------------------------------------------- dependency registry

fn registry() -> Result<BTreeSet<(String, String)>> {
    let text =
        fs::read_to_string("docs/dependencies.md").context("reading docs/dependencies.md")?;
    let mut entries = BTreeSet::new();
    for line in text.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // | `name` | ecosystem | ...
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

// ---------------------------------------------------------------- reference-case fixtures

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

// ---------------------------------------------------------------- spec guard

fn base_ref() -> Option<String> {
    // An all-zero SHA is what a forge sends for a branch's first push: there is no base.
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

fn spec_guard() -> Result<()> {
    let Some(base) = base_ref() else {
        eprintln!("    spec-guard: HEAD is the base; nothing to check");
        return Ok(());
    };
    let changed = output("git", &["diff", "--name-only", &format!("{base}...HEAD")])?;
    let changed: Vec<&str> = changed.lines().collect();
    let protected: Vec<&&str> = changed
        .iter()
        .filter(|f| PROTECTED_PATHS.iter().any(|p| f.starts_with(p)))
        .collect();
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
    let code: Vec<&&str> = changed
        .iter()
        .filter(|f| CODE_PATHS.iter().any(|p| f.starts_with(p)))
        .collect();
    if !code.is_empty() {
        problems.push(format!("specs, schemas, or reference cases changed together with code ({} code files); split the change", code.len()));
    }
    report(problems, "spec-guard")
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
    use super::contains_dec_id;

    #[test]
    fn finds_dec_ids() {
        assert!(contains_dec_id("Implements DEC-72."));
        assert!(!contains_dec_id("DEC- nothing"));
        assert!(!contains_dec_id("no decision"));
    }
}
