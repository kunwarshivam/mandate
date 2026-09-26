//! Repository automation. Every CI job is one `cargo xtask ci <job>` so that CI, a laptop, and an
//! air-gapped rebuild run the same checks (ADR-0001 ES-12).

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  check                 run every per-PR job locally
  ci <job>              run one CI job: fast | full | nightly, or one part: lint | test | pending |
                        spec-guard | refcases | reference | supply-chain | postgres | mutants
  layers                check crate layering and safety-critical policy (xtask/layers.toml)
  markers               check for debt markers and #[ignore] without a pending story
  feature-map           check the verification skill's feature map against the workspace
  deps                  check every direct dependency against docs/dependencies.md
  refcases [--write]    export reference-case YAML to fixtures/refcases (drift check unless --write)
";

/// The two required checks (DEC-76), so each pays the setup cost once. `pending` follows `test` in
/// `fast` because it reuses the test binaries that `test` has just built.
const FAST_JOB: [&str; 4] = ["lint", "test", "pending", "spec-guard"];
const FULL_JOB: [&str; 5] = [
    "refcases",
    "reference",
    "supply-chain",
    "postgres",
    "mutants",
];
const PR_JOBS: [&str; 9] = [
    "lint",
    "test",
    "pending",
    "refcases",
    "reference",
    "supply-chain",
    "spec-guard",
    "postgres",
    "mutants",
];

/// The database the Postgres journal tests use, and the switch that makes its absence a failure
/// rather than a skip (DEC-109). CI's `full` and nightly jobs set both.
const PG_URL: &str = "MANDATE_PG_URL";
const PG_REQUIRED: &str = "MANDATE_PG_REQUIRED";

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
        "pending" => pending(),
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
        "postgres" => postgres(),
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

/// The Postgres journal tests against a real database (ADR-0001 ES-08). `test` runs them too, but
/// they skip there unless `MANDATE_PG_URL` is set; here they must run.
fn postgres() -> Result<()> {
    if env::var_os(PG_URL).is_none() {
        if env::var_os(PG_REQUIRED).is_some() {
            bail!("{PG_REQUIRED} is set but {PG_URL} is not");
        }
        eprintln!(
            "    postgres: {PG_URL} is unset; skipping (see crates/mandate-journal-pg/README.md)"
        );
        return Ok(());
    }
    let args = [
        "nextest",
        "run",
        "--package",
        "mandate-journal-pg",
        "--locked",
        "--no-tests=pass",
    ];
    eprintln!("    $ {PG_REQUIRED}=1 cargo {}", args.join(" "));
    let status = Command::new("cargo")
        .args(args)
        .env(PG_REQUIRED, "1")
        .status()
        .context("starting `cargo nextest`")?;
    if !status.success() {
        bail!("the Postgres journal tests failed with {status}");
    }
    Ok(())
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
    output_in(Path::new("."), program, args)
}

fn output_in(dir: &Path, program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .current_dir(dir)
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

/// Cases for the `.gitleaks.toml` exceptions (DEC-89): a line appended to a file, and the rule that
/// must report it, or `None` where an exception must allow it. An exception allows its text only in the
/// files it names, and every other rule still applies there. The values are assembled at run time
/// so that this source matches no rule.
fn gitleaks_plants() -> Vec<(String, String, Option<&'static str>)> {
    let page = concat!(
        "U1BZfDIwMjYtMDktMjRUMTQ6MDA6",
        "fFBMQU5URUR8MTIzNDU2Nzg5MA=="
    );
    let json = format!("{{\"bars\":[],\"next_page_token\":\"{page}\"}}");
    let query = format!("/v2/stocks/bars?symbols=SPY&page_token={page}");
    let http = "crates/mandate-marketdata/tests/http.rs";
    let fixture = |name| format!("crates/mandate-marketdata/tests/fixtures/alpaca/planted/{name}");
    let key_id = format!("PK{}", "PLANTEDKEYID000000");
    let sentinel = format!("const KEY: &str = \"PK{}\";", "SENTINELKEYID00000");
    let generic = Some("generic-api-key");
    vec![
        (fixture("page-1.json"), json.clone(), None),
        (fixture("requests.txt"), query.clone(), None),
        (
            fixture("page-2.json"),
            key_id.clone(),
            Some("alpaca-key-id"),
        ),
        (fixture("notes.json"), json.clone(), generic),
        ("stray.json".to_owned(), json, generic),
        ("stray.txt".to_owned(), query, generic),
        (http.to_owned(), sentinel.clone(), None),
        (
            http.replace("http.rs", "wire.rs"),
            sentinel.clone(),
            Some("alpaca-key-id"),
        ),
        ("stray.rs".to_owned(), sentinel, Some("alpaca-key-id")),
        (http.to_owned(), key_id, Some("alpaca-key-id")),
    ]
}

/// Scans [`gitleaks_plants`], written to a temporary directory, with the repository's
/// `.gitleaks.toml`: exactly the expected findings must be reported.
fn gitleaks_exceptions() -> Result<()> {
    let dir = env::temp_dir().join(format!("mandate-gitleaks-{}", std::process::id()));
    let report_path = dir.with_extension("json");
    let mut expected = BTreeMap::new();
    for (path, line, rule) in gitleaks_plants() {
        let file = dir.join(&path);
        fs::create_dir_all(file.parent().context("planted file has no parent")?)?;
        let mut out = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)?;
        writeln!(out, "{line}")?;
        if let Some(rule) = rule {
            *expected.entry((path, rule.to_owned())).or_insert(0) += 1;
        }
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
    let mut found = BTreeMap::new();
    for f in findings {
        *found.entry((f.file, f.rule_id)).or_insert(0) += 1;
    }
    let keys: BTreeSet<_> = expected.keys().chain(found.keys()).collect();
    let problems: Vec<String> = keys
        .into_iter()
        .filter_map(|key @ (file, rule)| {
            let (want, got) = (
                expected.get(key).unwrap_or(&0),
                found.get(key).unwrap_or(&0),
            );
            (want != got)
                .then(|| format!("{file}: {rule} reported {got} finding(s), expected {want}"))
        })
        .collect();
    report(problems, "gitleaks-exceptions")?;
    eprintln!(
        "    gitleaks-exceptions: {} planted findings reported",
        expected.values().sum::<usize>()
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
    metadata_in(Path::new("."))
}

fn metadata_in(dir: &Path) -> Result<Metadata> {
    let json = output_in(
        dir,
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

/// The uv workspace root and every member listed in its `[tool.uv.workspace].members`.
fn python_manifests() -> Result<Vec<String>> {
    let root = "python/pyproject.toml";
    let doc: toml::Value =
        toml::from_str(&fs::read_to_string(root)?).with_context(|| format!("parsing {root}"))?;
    let members = doc
        .get("tool")
        .and_then(|t| t.get("uv"))
        .and_then(|u| u.get("workspace"))
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
        .context("python/pyproject.toml has no [tool.uv.workspace].members")?;
    let mut manifests = vec![root.to_owned()];
    for member in members {
        let dir = member
            .as_str()
            .context("a uv workspace member is not a string")?;
        manifests.push(format!("python/{dir}/pyproject.toml"));
    }
    Ok(manifests)
}

fn python_direct_dependencies() -> Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    let mut workspace_members = BTreeSet::new();
    for manifest in python_manifests()? {
        let doc: toml::Value = toml::from_str(&fs::read_to_string(&manifest)?)
            .with_context(|| format!("parsing {manifest}"))?;
        if let Some(sources) = doc
            .get("tool")
            .and_then(|t| t.get("uv"))
            .and_then(|u| u.get("sources"))
            .and_then(toml::Value::as_table)
        {
            workspace_members.extend(
                sources
                    .iter()
                    .filter(|(_, source)| {
                        source.get("workspace").and_then(toml::Value::as_bool) == Some(true)
                    })
                    .map(|(name, _)| name.to_lowercase()),
            );
        }
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
            if !name.is_empty() && !workspace_members.contains(&name.to_lowercase()) {
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
    line.trim()
        .strip_prefix("#[ignore = \"")
        .and_then(|rest| rest.strip_suffix("\"]"))
        .is_some_and(is_pending_reason)
}

/// `pending E<n>-<n>`, the only reason an `#[ignore]` may give (DEC-77).
fn is_pending_reason(reason: &str) -> bool {
    let Some(story) = reason.strip_prefix("pending E") else {
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

/// A DEC-77 tests PR marks a test `#[ignore = "pending <story>"]` because it cannot pass on the
/// PR's stubs. One that passes anyway pins nothing, so every pending test in the workspace is run
/// and must fail; a failure by panic, `todo!()` included, counts (DEC-110). With no pending test
/// there is nothing to run.
fn pending() -> Result<()> {
    report(pending_problems(Path::new("."))?, "pending")
}

/// Runs every test the working tree marks pending and names each one that passes or does not run.
fn pending_problems(root: &Path) -> Result<Vec<String>> {
    let mut args = vec![
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "--",
    ];
    args.extend(CODE_PATHS);
    let files = output_in(root, "git", &args)?;
    let packages = package_dirs(root)?;
    let mut tests = Vec::new();
    for file in files.lines().filter(|f| f.ends_with(".rs")) {
        let Ok(text) = fs::read_to_string(root.join(file)) else {
            continue;
        };
        let found = pending_tests(&text);
        if found.is_empty() {
            continue;
        }
        let (package, binary) = test_binary(&packages, file)
            .with_context(|| format!("{file} has pending tests but no workspace package"))?;
        tests.extend(found.into_iter().map(|test| PendingTestRun {
            file: file.to_owned(),
            package: package.clone(),
            binary: binary.clone(),
            test,
        }));
    }
    if tests.is_empty() {
        eprintln!("    pending: no pending tests");
        return Ok(Vec::new());
    }
    let filter = tests
        .iter()
        .map(PendingTestRun::filterset)
        .collect::<Vec<_>>()
        .join(" | ");
    let args = [
        "nextest",
        "run",
        "--workspace",
        "--locked",
        "--no-fail-fast",
        "--run-ignored",
        "ignored-only",
        "--no-tests=pass",
        "--message-format",
        "libtest-json",
        "-E",
        &filter,
    ];
    eprintln!(
        "    pending: {} pending test(s) must fail on this change's code",
        tests.len()
    );
    eprintln!("    $ cargo {}", args.join(" "));
    let out = Command::new("cargo")
        .current_dir(root)
        .args(args)
        .env("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1")
        .stderr(Stdio::inherit())
        .output()
        .context("starting `cargo nextest` (is it installed? see AGENTS.md)")?;
    const ALL_PASSED: i32 = 0;
    const SOME_FAILED: i32 = 100;
    if !matches!(out.status.code(), Some(ALL_PASSED | SOME_FAILED)) {
        bail!(
            "`cargo nextest` failed with {} before any test ran",
            out.status
        );
    }
    let outcomes = test_outcomes(&String::from_utf8(out.stdout).context("non-UTF-8 output")?);
    let problems = verdicts(&tests, &outcomes);
    if problems.is_empty() {
        eprintln!(
            "    pending: all {} fail, as pending tests must; nextest's `test run failed` above is expected",
            tests.len()
        );
    }
    Ok(problems)
}

/// A test marked `#[ignore = "pending <story>"]`: its path within its file (enclosing inline `mod`
/// blocks, then the function name), its story, and the line of its name.
struct PendingTest {
    path: String,
    story: String,
    line: usize,
}

/// A pending test and the nextest binaries it can be compiled into: exactly `binary` for an
/// integration-test crate root, whose tests are named by their path in the file, and otherwise any
/// binary of `package`, whose test names end with that path.
struct PendingTestRun {
    file: String,
    package: String,
    binary: Option<String>,
    test: PendingTest,
}

impl PendingTestRun {
    fn filterset(&self) -> String {
        let path = &self.test.path;
        match &self.binary {
            Some(binary) => format!("(binary_id(={binary}) & test(={path}))"),
            None => format!("(package(={}) & test(/(^|::){path}$/))", self.package),
        }
    }

    fn matches(&self, binary_id: &str, name: &str) -> bool {
        let path = &self.test.path;
        match &self.binary {
            Some(binary) => binary_id == binary && name == path,
            None => {
                let package = &self.package;
                (binary_id == package || binary_id.starts_with(&format!("{package}::")))
                    && (name == path || name.ends_with(&format!("::{path}")))
            }
        }
    }
}

/// A problem for each pending test that passed in any binary or ran in none.
fn verdicts(tests: &[PendingTestRun], outcomes: &[TestOutcome]) -> Vec<String> {
    tests
        .iter()
        .filter_map(|t| {
            let passed: Vec<bool> = outcomes
                .iter()
                .filter(|o| t.matches(&o.binary_id, &o.name))
                .map(|o| o.passed)
                .collect();
            let at = format!(
                "{}:{}: `{}` (pending {})",
                t.file, t.test.line, t.test.path, t.test.story
            );
            if passed.is_empty() {
                Some(format!(
                    "{at} did not run, so nothing shows that it fails on this change's code"
                ))
            } else if passed.contains(&true) {
                Some(format!(
                    "{at} passes on this change's code; a pending test must fail until its story \
                     is implemented, so make it assert what the stubs cannot satisfy (DEC-77)"
                ))
            } else {
                None
            }
        })
        .collect()
}

/// One finished test from nextest's `libtest-json` output.
#[derive(Debug, PartialEq)]
struct TestOutcome {
    binary_id: String,
    name: String,
    passed: bool,
}

#[derive(Deserialize)]
struct LibtestEvent {
    #[serde(rename = "type")]
    kind: String,
    event: String,
    name: Option<String>,
}

/// Test events are named `<binary id>$<test name>`; `started` is not a result.
fn test_outcomes(stdout: &str) -> Vec<TestOutcome> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<LibtestEvent>(line).ok())
        .filter(|e| e.kind == "test" && e.event != "started")
        .filter_map(|e| {
            let (binary_id, name) = e
                .name?
                .split_once('$')
                .map(|(b, n)| (b.to_owned(), n.to_owned()))?;
            Some(TestOutcome {
                binary_id,
                name,
                passed: e.event == "ok",
            })
        })
        .collect()
}

/// Each workspace package's name and directory relative to `root` (empty for the root package).
fn package_dirs(root: &Path) -> Result<Vec<(String, String)>> {
    let meta = metadata_in(root)?;
    let root = root.canonicalize()?;
    workspace_packages(&meta)
        .into_iter()
        .map(|p| {
            let dir = p
                .manifest_path
                .parent()
                .context("manifest without a directory")?
                .canonicalize()?;
            let dir = dir
                .strip_prefix(&root)
                .context("crate outside the repository")?;
            Ok((p.name.clone(), dir.display().to_string()))
        })
        .collect()
}

/// The package owning `file`, and the test binary if `file` is an integration-test crate root
/// (`tests/<name>.rs` or `tests/<name>/main.rs`).
fn test_binary(packages: &[(String, String)], file: &str) -> Option<(String, Option<String>)> {
    let (package, rel) = packages
        .iter()
        .filter_map(|(name, dir)| {
            let rel = if dir.is_empty() {
                Some(file)
            } else {
                file.strip_prefix(dir.as_str())?.strip_prefix('/')
            };
            rel.map(|rel| (name, dir.len(), rel))
        })
        .max_by_key(|(_, len, _)| *len)
        .map(|(name, _, rel)| (name, rel))?;
    let target = rel.strip_prefix("tests/").and_then(|t| {
        t.strip_suffix("/main.rs")
            .or_else(|| t.strip_suffix(".rs"))
            .filter(|name| !name.contains('/'))
    });
    Some((
        package.clone(),
        target.map(|name| format!("{package}::{name}")),
    ))
}

/// Every function carrying `#[ignore = "pending <story>"]`, however the attribute is spaced, split
/// across lines, or quoted, and never text inside a comment or a literal.
fn pending_tests(src: &str) -> Vec<PendingTest> {
    let toks = tokens(src);
    let tok = |i: usize| toks.get(i).map(|(t, _)| t);
    let mut found = Vec::new();
    let mut mods: Vec<(String, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while let Some(t) = tok(i) {
        match t {
            Token::Punct('{') => depth += 1,
            Token::Punct('}') => {
                depth = depth.saturating_sub(1);
                if mods.last().is_some_and(|(_, d)| *d == depth) {
                    mods.pop();
                }
            }
            Token::Ident(kw) if kw == "mod" => {
                if let (Some(Token::Ident(name)), Some(Token::Punct('{'))) =
                    (tok(i + 1), tok(i + 2))
                {
                    mods.push((name.clone(), depth));
                    depth += 1;
                    i += 3;
                    continue;
                }
            }
            Token::Punct('#') => {
                if let Some(end) = attribute_end(&toks, i)
                    && let [
                        (Token::Ident(ignore), _),
                        (Token::Punct('='), _),
                        (Token::Str(reason), _),
                    ] = toks.get(i + 2..end - 1).unwrap_or_default()
                    && ignore == "ignore"
                    && is_pending_reason(reason)
                    && let Some((name, line)) = function_after(&toks, end)
                {
                    let path = mods
                        .iter()
                        .map(|(m, _)| m.as_str())
                        .chain([name.as_str()])
                        .collect::<Vec<_>>()
                        .join("::");
                    found.push(PendingTest {
                        path,
                        story: reason.trim_start_matches("pending ").to_owned(),
                        line,
                    });
                }
            }
            _ => {}
        }
        i += 1;
    }
    found
}

/// The index just past the `]` closing the attribute whose `#` is at `start`.
fn attribute_end(toks: &[(Token, usize)], start: usize) -> Option<usize> {
    if toks.get(start + 1).map(|(t, _)| t) != Some(&Token::Punct('[')) {
        return None;
    }
    let mut depth = 0usize;
    for (i, (t, _)) in toks.iter().enumerate().skip(start + 1) {
        match t {
            Token::Punct('[') => depth += 1,
            Token::Punct(']') => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The name and line of the function an attribute list ending at `start` belongs to, past any
/// further attributes and qualifiers (`pub(crate)`, `async`, `unsafe`, `extern "C"`).
fn function_after(toks: &[(Token, usize)], start: usize) -> Option<(String, usize)> {
    let qualifiers = [
        "pub", "crate", "super", "self", "in", "async", "unsafe", "const", "extern",
    ];
    let mut i = start;
    loop {
        match toks.get(i) {
            Some((Token::Punct('#'), _)) => i = attribute_end(toks, i)?,
            Some((Token::Ident(q), _)) if qualifiers.contains(&q.as_str()) => i += 1,
            Some((Token::Punct('(' | ')') | Token::Str(_), _)) => i += 1,
            Some((Token::Ident(kw), _)) if kw == "fn" => break,
            _ => return None,
        }
    }
    match toks.get(i + 1) {
        Some((Token::Ident(name), line)) => Some((name.clone(), *line)),
        _ => None,
    }
}

/// A Rust token, enough to read attributes, `mod` blocks, and function names: comments are dropped
/// and string literals kept whole, so a marker quoted in either is never mistaken for code.
#[derive(Debug, PartialEq)]
enum Token {
    Ident(String),
    Str(String),
    Punct(char),
}

/// The tokens of `src` with the line each starts on.
fn tokens(src: &str) -> Vec<(Token, usize)> {
    let chars: Vec<char> = src.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let text = |from: usize, to: usize| chars.get(from..to).unwrap_or_default().iter().collect();
    let mut found = Vec::new();
    let (mut i, mut line) = (0, 1);
    while let Some(c) = at(i) {
        let start = line;
        match (c, at(i + 1)) {
            ('\n', _) => {
                line += 1;
                i += 1;
            }
            (c, _) if c.is_whitespace() => i += 1,
            ('/', Some('/')) => {
                while at(i).is_some_and(|c| c != '\n') {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                let mut depth = 0usize;
                while let Some(c) = at(i) {
                    match (c, at(i + 1)) {
                        ('/', Some('*')) => {
                            depth += 1;
                            i += 2;
                        }
                        ('*', Some('/')) => {
                            depth -= 1;
                            i += 2;
                            if depth == 0 {
                                break;
                            }
                        }
                        ('\n', _) => {
                            line += 1;
                            i += 1;
                        }
                        _ => i += 1,
                    }
                }
            }
            ('"', _) => {
                let from = i + 1;
                i = from;
                while let Some(c) = at(i) {
                    match c {
                        '\\' => i += 1,
                        '"' => break,
                        '\n' => line += 1,
                        _ => {}
                    }
                    i += 1;
                }
                found.push((Token::Str(text(from, i)), start));
                i += 1;
            }
            ('\'', Some('\\')) => {
                i += 2;
                while at(i).is_some_and(|c| c != '\'') {
                    i += 1;
                }
                i += 1;
            }
            ('\'', Some(_)) if at(i + 2) == Some('\'') => i += 3,
            (c, _) if is_ident(c) => {
                let from = i;
                while at(i).is_some_and(is_ident) {
                    i += 1;
                }
                let word: String = text(from, i);
                let hashes = chars
                    .get(i..)
                    .unwrap_or_default()
                    .iter()
                    .take_while(|&&c| c == '#')
                    .count();
                if matches!(word.as_str(), "r" | "br" | "cr") && at(i + hashes) == Some('"') {
                    let closing: Vec<char> = std::iter::once('"')
                        .chain(std::iter::repeat_n('#', hashes))
                        .collect();
                    let from = i + hashes + 1;
                    i = from;
                    while at(i).is_some()
                        && !chars.get(i..).unwrap_or_default().starts_with(&closing)
                    {
                        if at(i) == Some('\n') {
                            line += 1;
                        }
                        i += 1;
                    }
                    found.push((Token::Str(text(from, i)), start));
                    i += closing.len();
                } else if word == "r" && hashes == 1 && at(i + 1).is_some_and(is_ident) {
                    let from = i + 1;
                    i = from;
                    while at(i).is_some_and(is_ident) {
                        i += 1;
                    }
                    found.push((Token::Ident(text(from, i)), start));
                } else {
                    found.push((Token::Ident(word), start));
                }
            }
            _ => {
                found.push((Token::Punct(c), start));
                i += 1;
            }
        }
    }
    found
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
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    use anyhow::{Context, Result};

    use super::{
        PendingTest, PendingTestRun, TestOutcome, backticked_paths, classify, contains_dec_id,
        contains_word, is_pending_marker, output_in, pending_problems, pending_tests,
        plain_comment_lines, repo_root, test_binary, test_outcomes, verdicts,
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

    fn found(src: &str) -> Vec<(String, String, usize)> {
        pending_tests(src)
            .into_iter()
            .map(|t| (t.path, t.story, t.line))
            .collect()
    }

    fn expected(tests: &[(&str, &str, usize)]) -> Vec<(String, String, usize)> {
        tests
            .iter()
            .map(|&(path, story, line)| (path.to_owned(), story.to_owned(), line))
            .collect()
    }

    #[test]
    fn finds_pending_tests_but_not_markers_in_comments_or_literals() {
        let src = concat!(
            "//! #[ignore = \"pending E1-1\"] in crate docs\n",
            "/// #[ignore = \"pending E1-2\"]\n",
            "/// fn in_doc_comment() {}\n",
            "#[test]\n",
            "#[ignore = \"pending E2-1\"]\n",
            "fn plain() {}\n",
            "// #[ignore = \"pending E1-3\"]\n",
            "// fn in_line_comment() {}\n",
            "/* #[ignore = \"pending E1-4\"] fn in_block_comment() {} */\n",
            "const S: &str = \"#[ignore = \\\"pending E1-5\\\"] fn in_string() {}\";\n",
            "const R: &str = r#\"#[ignore = \"pending E1-6\"] fn in_raw_string() {}\"#;\n",
            "#[test]\n",
            "#[ignore = \"slow\"]\n",
            "fn not_a_story() {}\n",
            "#[test]\n",
            "#[ignore]\n",
            "fn bare_ignore() {}\n",
            "/// Documented, with attributes on both sides of the marker.\n",
            "#[test]\n",
            "#[ignore = \"pending E2-2\"]\n",
            "/// More docs.\n",
            "#[should_panic(expected = \"}\")]\n",
            "pub(crate) async fn qualified() {}\n",
            "mod outer {\n",
            "    fn helper<'a>(s: &'a str) -> char { '{' }\n",
            "    mod inner {\n",
            "        #[test]\n",
            "        #[ignore = \"pending E2-3\"]\n",
            "        fn nested() {}\n",
            "    }\n",
            "    #[test]\n",
            "    #[ignore = \"pending E2-4\"]\n",
            "    fn after_inner() {}\n",
            "}\n",
            "#[ignore = \"pending E1-7\"]\n",
            "struct NotAFunction;\n",
            "proptest! {\n",
            "    #[test]\n",
            "    #[ignore = \"pending E2-5\"]\n",
            "    fn property(x in 0..10u8) { prop_assert!(x < 10); }\n",
            "}\n",
        );
        assert_eq!(
            found(src),
            expected(&[
                ("plain", "E2-1", 6),
                ("qualified", "E2-2", 23),
                ("outer::inner::nested", "E2-3", 29),
                ("outer::after_inner", "E2-4", 33),
                ("property", "E2-5", 40),
            ])
        );
    }

    #[test]
    fn a_pending_marker_can_span_lines_and_use_a_raw_string() {
        let src = concat!(
            "#[test]\n",
            "#[ignore =\n",
            "    \"pending E3-1\"\n",
            "]\n",
            "fn split_across_lines() {}\n",
            "#[test] #[ignore = r\"pending E3-2\"] fn raw() {}\n",
            "#[test]\n",
            "# [ ignore = r##\"pending E3-3\"## ]\n",
            "fn spaced_raw_hashes() {}\n",
            "#[test]\n",
            "#[ignore = \"pending E3\"]\n",
            "fn not_a_story_id() {}\n",
            "#[test]\n",
            "#[ignore = \"pending E3-4\"]\n",
            "fn r#match() {}\n",
        );
        assert_eq!(
            found(src),
            expected(&[
                ("split_across_lines", "E3-1", 5),
                ("raw", "E3-2", 6),
                ("spaced_raw_hashes", "E3-3", 9),
                ("match", "E3-4", 15),
            ])
        );
    }

    #[test]
    fn a_file_maps_to_its_package_and_integration_test_binary() {
        let packages = [
            ("root".to_owned(), String::new()),
            ("a".to_owned(), "crates/a".to_owned()),
            ("a-b".to_owned(), "crates/a-b".to_owned()),
        ];
        let at = |file| test_binary(&packages, file);
        let owned = |package: &str, binary: Option<&str>| {
            Some((package.to_owned(), binary.map(str::to_owned)))
        };
        assert_eq!(at("crates/a/tests/fs.rs"), owned("a", Some("a::fs")));
        assert_eq!(
            at("crates/a/tests/suite/main.rs"),
            owned("a", Some("a::suite"))
        );
        assert_eq!(at("crates/a/tests/common/mod.rs"), owned("a", None));
        assert_eq!(at("crates/a/src/tests.rs"), owned("a", None));
        assert_eq!(at("crates/a-b/tests/x.rs"), owned("a-b", Some("a-b::x")));
        assert_eq!(at("xtask/src/main.rs"), owned("root", None));
        assert_eq!(test_binary(&packages[1..], "xtask/src/main.rs"), None);
    }

    #[test]
    fn a_pending_test_that_passes_or_never_runs_is_a_problem() {
        let stdout = concat!(
            "{\"type\":\"suite\",\"event\":\"started\",\"test_count\":4}\n",
            "{\"type\":\"test\",\"event\":\"started\",\"name\":\"a::fs$passes\"}\n",
            "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"a::fs$passes\",\"exec_time\":0.1}\n",
            "{\"type\":\"test\",\"event\":\"failed\",\"name\":\"a::fs$fails\",\"stdout\":\"x\"}\n",
            "{\"type\":\"test\",\"event\":\"failed\",\"name\":\"a::fs$inner::passes\"}\n",
            "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"a$tests::unit\"}\n",
            "{\"type\":\"suite\",\"event\":\"failed\",\"passed\":2,\"failed\":2}\n",
            "not a JSON line\n",
        );
        let outcomes = test_outcomes(stdout);
        let outcome = |binary_id: &str, name: &str, passed| TestOutcome {
            binary_id: binary_id.to_owned(),
            name: name.to_owned(),
            passed,
        };
        assert_eq!(
            outcomes,
            [
                outcome("a::fs", "passes", true),
                outcome("a::fs", "fails", false),
                outcome("a::fs", "inner::passes", false),
                outcome("a", "tests::unit", true),
            ]
        );
        let new = |line, binary: Option<&str>, path: &str| PendingTestRun {
            file: "f.rs".to_owned(),
            package: "a".to_owned(),
            binary: binary.map(str::to_owned),
            test: PendingTest {
                path: path.to_owned(),
                story: "E1-1".to_owned(),
                line,
            },
        };
        let tests = [
            new(1, Some("a::fs"), "passes"),
            new(2, Some("a::fs"), "fails"),
            new(3, Some("a::fs"), "inner::passes"),
            new(4, None, "unit"),
            new(5, None, "missing"),
            new(6, Some("a::other"), "fails"),
        ];
        let problems: Vec<String> = verdicts(&tests, &outcomes)
            .iter()
            .map(|p| p.split([',', ';']).next().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(
            problems,
            [
                "f.rs:1: `passes` (pending E1-1) passes on this change's code",
                "f.rs:4: `unit` (pending E1-1) passes on this change's code",
                "f.rs:5: `missing` (pending E1-1) did not run",
                "f.rs:6: `fails` (pending E1-1) did not run",
            ]
        );
    }

    /// A git repository in a temporary directory holding a one-crate workspace laid out like this
    /// one, removed on drop.
    struct Fixture(PathBuf);

    const STUBS: &str = concat!(
        "#[derive(Debug, PartialEq)]\n",
        "pub struct Missing;\n",
        "pub fn answer() -> u32 { todo!() }\n",
        "pub fn lookup() -> Result<u32, Missing> { Err(Missing) }\n",
    );

    impl Fixture {
        fn new(name: &str) -> Result<Self> {
            let dir = env::temp_dir().join(format!("mandate-xtask-{name}-{}", std::process::id()));
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
            }
            let fixture = Fixture(dir);
            fixture.write(
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/fx\"]\nresolver = \"3\"\n",
            )?;
            fixture.write(
                "crates/fx/Cargo.toml",
                "[package]\nname = \"fx\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
            )?;
            fixture.write("crates/fx/src/lib.rs", STUBS)?;
            fixture.write(".gitignore", "/target\n")?;
            fs::copy(
                repo_root()?.join("rust-toolchain.toml"),
                fixture.0.join("rust-toolchain.toml"),
            )?;
            output_in(&fixture.0, "cargo", &["generate-lockfile", "--offline"])?;
            output_in(&fixture.0, "git", &["init", "-q"])?;
            Ok(fixture)
        }

        fn write(&self, path: &str, text: &str) -> Result<()> {
            let file = self.0.join(path);
            fs::create_dir_all(file.parent().context("fixture file without a directory")?)?;
            fs::write(file, text)?;
            Ok(())
        }

        fn commit(&self) -> Result<()> {
            output_in(&self.0, "git", &["add", "-A"])?;
            output_in(
                &self.0,
                "git",
                &[
                    "-c",
                    "user.name=xtask",
                    "-c",
                    "user.email=xtask@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                    "commit",
                    "-q",
                    "-m",
                    "fixture",
                ],
            )?;
            Ok(())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn the_gate_fails_exactly_the_pending_tests_that_pass_on_the_stubs() -> Result<()> {
        let fx = Fixture::new("pending")?;
        let live = "#[test]\nfn live() { assert!(fx::lookup().is_err()); }\n";
        fx.write("crates/fx/tests/stubs.rs", live)?;
        fx.commit()?;
        assert_eq!(
            pending_problems(&fx.0)?,
            Vec::<String>::new(),
            "with no pending test there is nothing to run"
        );

        let failing = concat!(
            "#[test]\n#[ignore = \"pending E1-1\"]\n",
            "fn fails_by_todo() { assert_eq!(fx::answer(), 42); }\n",
            "#[test]\n#[ignore = \"pending E1-1\"]\n",
            "fn fails_by_assertion() { assert_eq!(fx::lookup(), Ok(42)); }\n",
            "mod inner {\n#[test]\n#[ignore = \"pending E1-1\"]\n",
            "fn passes_on_the_stubs() { assert_eq!(fx::answer(), 42); }\n}\n",
        );
        fx.write("crates/fx/tests/stubs.rs", &format!("{live}{failing}"))?;
        fx.commit()?;
        assert_eq!(
            pending_problems(&fx.0)?,
            Vec::<String>::new(),
            "pending tests that fail on the stubs pass the gate"
        );

        let passing = concat!(
            "#[test]\n#[ignore = \"pending E1-1\"]\n",
            "fn passes_on_the_stubs() { assert!(fx::lookup().is_err()); }\n",
            "#[cfg(any())]\n#[test]\n#[ignore = \"pending E1-1\"]\n",
            "fn never_compiled() {}\n",
        );
        fx.write(
            "crates/fx/tests/stubs.rs",
            &format!("{live}{failing}{passing}"),
        )?;
        fx.commit()?;
        fx.write(
            "crates/fx/tests/untracked.rs",
            "#[test]\n#[ignore = \"pending E1-2\"]\nfn untracked_and_passing() {}\n",
        )?;
        let unit = concat!(
            "#[cfg(test)]\nmod tests {\n#[test]\n#[ignore = \"pending E1-1\"]\n",
            "fn unit_passes() { assert!(super::lookup().is_err()); }\n}\n",
        );
        fx.write("crates/fx/src/lib.rs", &format!("{STUBS}{unit}"))?;
        let mut problems: Vec<String> = pending_problems(&fx.0)?
            .iter()
            .map(|p| p.split([',', ';']).next().unwrap_or_default().to_owned())
            .collect();
        problems.sort();
        assert_eq!(
            problems,
            [
                "crates/fx/src/lib.rs:9: `tests::unit_passes` (pending E1-1) passes on this change's code",
                "crates/fx/tests/stubs.rs:16: `passes_on_the_stubs` (pending E1-1) passes on this change's code",
                "crates/fx/tests/stubs.rs:20: `never_compiled` (pending E1-1) did not run",
                "crates/fx/tests/untracked.rs:3: `untracked_and_passing` (pending E1-2) passes on this change's code",
            ]
        );
        Ok(())
    }
}
