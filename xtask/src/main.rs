//! Repository automation. Every CI job is one `cargo xtask ci <job>` so that CI, a laptop, and an
//! air-gapped rebuild run the same checks (ADR-0001 ES-12).

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  check                 run every per-PR job locally
  ci <job>              run one CI job: fast | full | nightly, or one part: lint | test | pending |
                        spec-guard | refcases | reference | supply-chain | postgres | mutants
  ci mutants --plan     print the mutation matrix this diff needs, as GITHUB_OUTPUT lines
  layers                check crate layering and safety-critical policy (xtask/layers.toml)
  markers               check for debt markers and #[ignore] without a pending story
  feature-map [--index] check the verification skill's feature map against the workspace, or
                        list its feature files and titles
  deps                  check every direct dependency against docs/dependencies.md
  live-feature          check that only the runner may build `live`, and CI only compiles it
  refcases [--write]    export reference-case YAML to fixtures/refcases (drift check unless --write)
";

/// The two required-check command groups (DEC-76). `pending` follows `test` in `fast` because it
/// reuses the test binaries that `test` has just built. CI shards `mutants` separately and makes
/// its required `full` verdict depend on every shard (DEC-464); the local `check` command still
/// runs every entry in [`PR_JOBS`] once, unsharded.
const FAST_JOB: [&str; 4] = ["lint", "test", "pending", "spec-guard"];
const FULL_JOB: [&str; 4] = ["refcases", "reference", "supply-chain", "postgres"];
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
const MUTANT_SHARD_ENV: &str = "MANDATE_MUTANT_SHARD";

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
const PR_NUMBER: &str = "MANDATE_PR_NUMBER";
const E77_ATOMIC_PR: u64 = 598;
const E77_ATOMIC_BRANCH: &str = "cursor/e77-complete-tracer-4832";
const E77_ATOMIC_MARKER: &str = "ES-22-atomic-exception: E7-7 PR #598 (DEC-462)";

/// The verification skill's map of features to code, tests, and commands (AGENTS.md): one Markdown
/// file a feature, so a PR adding a feature adds a file and one changing a feature edits only
/// that feature's file. The directory is the map; `cargo xtask feature-map --index` lists it.
const FEATURE_MAP: &str = ".cursor/skills/verify-mandate/features";
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
        ["ci", "mutants", "--plan"] => {
            let plan = mutants_plan(Path::new("."), base_ref()?.as_deref())?;
            eprintln!(
                "    mutants: {} mutant(s) over {} shard(s)",
                plan.mutants, plan.shards
            );
            print!("{}", plan.outputs());
            Ok(())
        }
        ["ci", job] => ci(job),
        ["layers"] => layers(),
        ["markers"] => markers(),
        ["feature-map"] => feature_map(),
        ["feature-map", "--index"] => feature_map_index(),
        ["deps"] => deps(),
        ["live-feature"] => live_feature_in(Path::new(".")),
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
        "lint" => lint(Path::new("."), workspace_lint),
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
            reference(&["mandate/generate.py"])?;
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
            reference(&["mandate/check_cases.py"])?;
            for seed in ["1", "2", "3"] {
                reference(&["mandate/fuzz.py", seed])?;
            }
            reference(&["journal/generate.py", "--check"]).context(
                "reference/journal/generate.py --check failed: a validator or seeded bug check failed, or journal.yaml is not what it generates; run it without --check and commit the result",
            )?;
            mutation_anchors()
        }
        "supply-chain" => {
            sh("cargo", &["deny", "--locked", "check"])?;
            deps()?;
            match base_ref()? {
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
        "mutants" => mutants(Path::new("."), base_ref()?.as_deref()),
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
                reference(&["mandate/fuzz.py", &seed.to_string()])?;
            }
            reference(&["mandate/mutants.py"])?;
            sh("gitleaks", &["git", "--no-banner", "--redact"])
        }
        other => bail!("unknown CI job: {other}"),
    }
}

/// The Postgres journal tests against a real database (ADR-0001 ES-08), `mandate-journal-pg`'s and
/// the CLI's control journal's (P0, DEC-520). `test` runs them too, but they skip there unless
/// `MANDATE_PG_URL` is set; here they must run.
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
        "--package",
        "mandate-cli",
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

/// The lint job over the repository at `root` (ADR-0001 ES-12): ShellCheck over its
/// `.github/scripts/` (DEC-329) and `deploy/` (the demo host's runbook, DEC-822), actionlint over its `.github/workflows/` (DEC-330), then
/// `workspace_checks`, the checks that need the Cargo and uv workspaces, which `ci lint` passes as
/// [`workspace_lint`]. The job takes the repository it runs in, so a fixture repository drives it
/// and neither tool's result can be dropped without a test failing (DEC-331, the DEC-139 pattern).
fn lint(root: &Path, workspace_checks: impl FnOnce() -> Result<()>) -> Result<()> {
    shellcheck_scripts(&root.join(".github/scripts"))?;
    shellcheck_scripts(&root.join("deploy"))?;
    actionlint_workflows(&root.join(".github/workflows"))?;
    live_feature_in(root)?;
    workspace_checks()
}

/// The lint job's checks over the Cargo and uv workspaces in the current directory: fmt, clippy
/// `-D warnings`, crate layering, markers, saved proptest seeds, the feature map, typos, and ruff.
fn workspace_lint() -> Result<()> {
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
    proptest_seeds()?;
    feature_map()?;
    sh("typos", &[])?;
    uv_tools(&["ruff", "check", "."])?;
    uv_tools(&["ruff", "format", "--check", "."])
}

/// Every file under `dir` whose extension is one of `extensions`, sorted by path so the linter's
/// command line and report order are stable (E1-4, DEC-329). Directory symlinks are not followed.
fn files_by_extension(dir: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let mut entries: Vec<_> = fs::read_dir(&dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if entry
                .file_type()
                .with_context(|| format!("reading {}", path.display()))?
                .is_dir()
            {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|e| extensions.contains(&e.to_string_lossy().as_ref()))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Runs a linter over paths and embeds its findings in the failure, so a planted `SC2086` or an
/// unknown workflow key fails the job naming its code. Both tools report their findings on stdout
/// and keep stderr for their own errors, so both streams travel with the failure. A missing
/// binary is the loud error [`sh`] reports, never a silent skip (E1-4, DEC-330).
fn lint_paths(program: &str, args: &[String]) -> Result<()> {
    eprintln!("    $ {program} {}", args.join(" "));
    let out = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("starting `{program}` (is it installed? see AGENTS.md)"))?;
    if !out.status.success() {
        let mut findings = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stderr.trim().is_empty() {
            if !findings.trim().is_empty() {
                findings.push('\n');
            }
            findings.push_str(&stderr);
        }
        bail!(
            "`{program} {}` failed: {}",
            args.join(" "),
            findings.trim_end()
        );
    }
    Ok(())
}

/// ShellCheck over every `*.sh` under `.github/scripts/` (E1-4, DEC-329), the shell that CI's
/// short path and the merge gate run as they are on the default branch. A directory holding no
/// scripts still runs the version check, so an uninstalled tool fails the job it belongs to.
fn shellcheck_scripts(dir: &Path) -> Result<()> {
    let scripts = files_by_extension(dir, &["sh"])?;
    if scripts.is_empty() {
        return sh("shellcheck", &["--version"]);
    }
    let args: Vec<String> = scripts
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    lint_paths("shellcheck", &args)
}

/// actionlint over every `*.yml` and `*.yaml` under `.github/workflows/` (E1-4, DEC-330), so a
/// workflow mistake fails `fast` instead of waiting for a reviewer. A directory holding no
/// workflows still runs the version check, so an uninstalled tool fails the job it belongs to.
fn actionlint_workflows(dir: &Path) -> Result<()> {
    let workflows = files_by_extension(dir, &["yml", "yaml"])?;
    if workflows.is_empty() {
        return sh("actionlint", &["--version"]);
    }
    let args: Vec<String> = workflows
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    lint_paths("actionlint", &args)
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
/// so that this source matches no rule. The palette rows prove the "Web palette ramp references"
/// exception (the `lapis` token names contain "api"): the ramp row is allowed in the design-source
/// file alone, the same row in a stray file is reported, and a real key pasted into that file is
/// reported too. The fingerprint rows prove the "Pinned GPG key fingerprints" exception: allowed in
/// a deploy script in the `NAME_FINGERPRINT=<hex>` shape alone.
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
    let palette = "web/src/lib/palette.ts";
    let ramp = "  \"lapis-soft\": \"ultramarine-100\",";
    let palette_key = format!("  \"api-key\": \"{page}\",");
    let hex = "CC94B39C77AE7342A68B89628A682D308D4E5E73";
    let bootstrap = "deploy/bootstrap.sh";
    let fingerprint = format!("CLOUDFLARE_FINGERPRINT={hex}");
    let not_fingerprint = format!("CLOUDFLARE_KEY={hex}");
    let cloudflare = Some("cloudflare-api-key");
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
        (palette.to_owned(), ramp.to_owned(), None),
        ("stray-palette.ts".to_owned(), ramp.to_owned(), generic),
        (palette.to_owned(), palette_key, generic),
        (bootstrap.to_owned(), fingerprint.clone(), None),
        ("stray-fingerprint.sh".to_owned(), fingerprint, cloudflare),
        (bootstrap.to_owned(), not_fingerprint, cloudflare),
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

/// Runs a frozen reference script, named relative to `reference/`, in the reference
/// implementations' own pinned environment (ADR-0001 ES-10): `reference/mandate/requirements.txt`
/// pins the PyYAML and jsonschema that `reference/journal/` needs too, so one environment serves
/// both. The per-PR `reference` job runs the mandate reference's generator, case check and three
/// fuzz seeds, the journal generator's `--check` (its self-test, validators, independent
/// recomputation and seeded bugs, then the committed `journal.yaml` must match what it generates;
/// #443 round 3, #476 round 1), and [`mutation_anchors`]; the nightly adds seven fuzz seeds and the
/// mutant sweep.
fn reference(script_and_args: &[&str]) -> Result<()> {
    let Some((script, rest)) = script_and_args.split_first() else {
        bail!("no reference script given");
    };
    let path = format!("reference/{script}");
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

/// The anchor-only check of `reference/mandate/mutants.py` (#443 round 3): every mutant's `old`
/// text must still occur in `ref.py`, which the sweep asserts before its first run. Only the
/// nightly runs the sweep, so an anchor an edit to `ref.py` left behind passed every per-PR job
/// until this check; it runs in well under a second from the `python/` workspace, which lints and
/// tests it (`python/mandate_tools/tests/test_mutation_anchors.py`).
fn mutation_anchors() -> Result<()> {
    uv_tools(&[
        "python",
        "-m",
        "mandate_tools.mutation_anchors",
        "../reference/mandate",
    ])
    .context(
        "a mutation anchor in reference/mandate/mutants.py no longer matches ref.py; re-anchor it, or the nightly's mutants.py fails before its first run",
    )
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
    /// The package's `[features]`: each name with what it enables.
    #[serde(default)]
    features: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
    path: Option<PathBuf>,
    /// The features this dependency turns on in the crate it names.
    #[serde(default)]
    features: Vec<String>,
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

/// Every workspace package with the workspace packages it is built on, itself included. A test
/// exercises exactly the code its own package links, so this is what turns the reference
/// harness's dependency list into the full set of packages its suites can fail on (DEC-497),
/// without anyone auditing which call reaches which layer.
fn workspace_closure(meta: &Metadata) -> BTreeMap<String, BTreeSet<String>> {
    let dirs = member_dirs(meta);
    let direct: BTreeMap<&str, Vec<&str>> = workspace_packages(meta)
        .into_iter()
        .map(|pkg| {
            let deps = pkg
                .dependencies
                .iter()
                .filter(|dep| on_member(dep, &dirs))
                .map(|dep| dep.name.as_str())
                .collect();
            (pkg.name.as_str(), deps)
        })
        .collect();
    direct
        .keys()
        .map(|package| {
            let mut reached = BTreeSet::new();
            let mut pending = vec![*package];
            while let Some(next) = pending.pop() {
                if reached.insert(next.to_owned()) {
                    pending.extend(direct.get(next).into_iter().flatten().copied());
                }
            }
            ((*package).to_owned(), reached)
        })
        .collect()
}

/// Each workspace member's directory, by name: where a path dependency on it must point.
fn member_dirs(meta: &Metadata) -> BTreeMap<&str, &Path> {
    workspace_packages(meta)
        .into_iter()
        .filter_map(|pkg| Some((pkg.name.as_str(), pkg.manifest_path.parent()?)))
        .collect()
}

/// Whether `dep` is a path dependency on the workspace member it names, by that member's
/// directory, not by name alone: a crate outside the workspace can take a member's name (DEC-525).
fn on_member(dep: &Dependency, dirs: &BTreeMap<&str, &Path>) -> bool {
    dep.path
        .as_deref()
        .is_some_and(|path| dirs.get(dep.name.as_str()) == Some(&path))
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
    /// Crates planned but not yet created, which a `forbidden_internal` list may already name
    /// (DEC-525).
    #[serde(default)]
    planned: Vec<String>,
    crates: BTreeMap<String, CratePolicy>,
}

#[derive(Deserialize)]
struct CratePolicy {
    layer: toml::Value,
    safety_critical: bool,
    pure: bool,
    #[serde(default)]
    allowed_external: Vec<String>,
    /// Workspace crates this crate may never reach, directly or through any chain of path
    /// dependencies, dev-dependencies included, whatever the layers allow (DEC-525).
    #[serde(default)]
    forbidden_internal: Vec<String>,
    /// When set, the only workspace crates that may depend on this crate, by a dependency of any
    /// kind: an allowlist, so a crate added later is refused until it is named (DEC-642 item 7).
    #[serde(default)]
    allowed_dependents: Option<Vec<String>>,
    /// Whether every workspace crate that depends on this one must do so as a dev-dependency, as
    /// test support must (DEC-645).
    #[serde(default)]
    dev_only: bool,
    /// The one crate that may declare a `live` cargo feature: the deployment runner (ES-23 as
    /// DEC-529 item 3 narrows it). No crate is marked until the runner's G1a slice marks it.
    #[serde(default)]
    live_feature: bool,
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
    report(layer_problems(&policy, &metadata()?)?, "layers")
}

/// Every way the workspace in `meta` breaks `policy`.
fn layer_problems(policy: &Layers, meta: &Metadata) -> Result<Vec<String>> {
    let packages = workspace_packages(meta);
    let names: BTreeSet<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    let closure = workspace_closure(meta);
    let dirs = member_dirs(meta);
    let mut problems = Vec::new();

    for listed in policy.crates.keys() {
        if !names.contains(listed.as_str()) {
            problems.push(format!(
                "`{listed}` is in layers.toml but is not a workspace member"
            ));
        }
    }
    for (listed, own) in &policy.crates {
        let unknown = own
            .forbidden_internal
            .iter()
            .filter(|name| !names.contains(name.as_str()) && !policy.planned.contains(name));
        problems.extend(unknown.map(|name| {
            format!(
                "`{listed}` forbids `{name}`, which is neither a workspace member nor in `planned`, \
                 so a misspelt name would guard nothing (DEC-525)"
            )
        }));
    }
    for (listed, own) in &policy.crates {
        let unknown = own
            .allowed_dependents
            .iter()
            .flatten()
            .filter(|name| !names.contains(name.as_str()) && !policy.planned.contains(name));
        problems.extend(unknown.map(|name| {
            format!(
                "`{listed}` allows `{name}` as a dependent, which is neither a workspace member nor \
                 in `planned` (DEC-642 item 7)"
            )
        }));
    }
    for pkg in &packages {
        let Some(own) = policy.crates.get(&pkg.name) else {
            problems.push(format!("`{}` has no entry in xtask/layers.toml", pkg.name));
            continue;
        };
        let own_layer = layer_of(own, &pkg.name)?;
        if let Some(reached) = closure.get(&pkg.name) {
            problems.extend(forbidden_reached(
                &pkg.name,
                &own.forbidden_internal,
                reached,
            ));
        }
        for dep in &pkg.dependencies {
            if let Some(path) = dep.path.as_deref()
                && !on_member(dep, &dirs)
            {
                problems.push(format!(
                    "`{}` has a path dependency on `{}`, which is not a workspace member at {}, so \
                     the layers cannot see what it reaches (DEC-525)",
                    pkg.name,
                    dep.name,
                    path.display()
                ));
                continue;
            }
            let internal = dep.path.is_some() && names.contains(dep.name.as_str());
            if internal {
                let Some(dep_policy) = policy.crates.get(&dep.name) else {
                    continue;
                };
                if dep_policy.dev_only && dep.kind.as_deref() != Some("dev") {
                    problems.push(format!(
                        "`{}` depends on `{}`, which is dev-only in xtask/layers.toml, other than as \
                         a dev-dependency (DEC-645)",
                        pkg.name, dep.name
                    ));
                }
                if let Some(allowed) = &dep_policy.allowed_dependents
                    && !allowed.contains(&pkg.name)
                {
                    problems.push(format!(
                        "`{}` depends on `{}`, whose allowed_dependents in xtask/layers.toml does \
                         not name it (DEC-642 item 7)",
                        pkg.name, dep.name
                    ));
                }
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
    Ok(problems)
}

/// A problem for each crate in `forbidden` that `name` reaches, `reached` being every workspace
/// crate it is built on through path dependencies of any kind (DEC-525).
fn forbidden_reached(name: &str, forbidden: &[String], reached: &BTreeSet<String>) -> Vec<String> {
    forbidden
        .iter()
        .filter(|crate_name| reached.contains(*crate_name))
        .map(|crate_name| {
            format!(
                "`{name}` reaches `{crate_name}`, which its forbidden_internal list in \
                 xtask/layers.toml rules out (DEC-525)"
            )
        })
        .collect()
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

/// Where proptest saves a failing case as a seed beside the test that produced it, in both
/// shapes it writes: `proptest-regressions/<file>.txt` at the crate root, its default, and
/// `<file>.proptest-regressions` next to the source. Git pathspecs, so `*` crosses directories.
const PROPTEST_SEED_PATHSPECS: [&str; 2] = ["*.proptest-regressions", "*proptest-regressions/*"];

/// Every proptest failure seed in the tree at `dir`, tracked or untracked, ignored or not.
/// Proptest replays a saved seed before it draws fresh cases, so a seed left by an earlier run, on
/// other code or under a planted bug, changes which cases every later run tries (DEC-164); a
/// committed one does that in every checkout (#449 review, M1).
fn proptest_seeds_in(dir: &Path) -> Result<Vec<String>> {
    let mut args = vec!["ls-files", "--cached", "--others", "--"];
    args.extend_from_slice(&PROPTEST_SEED_PATHSPECS);
    Ok(output_in(dir, "git", &args)?
        .lines()
        .map(str::to_owned)
        .collect())
}

/// `.gitignore` keeps a seed out of a commit made with `git add -A`; this check also fails on one
/// that was added by name or was left in the working tree (DEC-381).
fn proptest_seeds() -> Result<()> {
    eprintln!("    proptest-seeds: checking for saved proptest failure seeds");
    let problems = proptest_seeds_in(Path::new("."))?
        .into_iter()
        .map(|file| {
            format!(
                "{file}: a saved proptest failure seed; delete it, since proptest replays it \
                 before drawing fresh cases (DEC-164, DEC-381)"
            )
        })
        .collect();
    report(problems, "proptest-seeds")
}

/// Debt markers get copied as precedent, `#[ignore]` without a pending story hides a test forever,
/// and plain comments become the justification agents copy for workarounds (AGENTS.md, "The trust
/// ladder"; DEC-80).
fn markers() -> Result<()> {
    eprintln!(
        "    markers: checking for debt markers, unexplained #[ignore], and ungated pending tests"
    );
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
        for (n, why) in generated_pending_markers(&text) {
            problems.push(format!(
                "{file}:{n}: a pending marker {why}; the pending gate reads the source, so a \
                 pending test must be written out as a plain function or it is never gated \
                 (DEC-110, DEC-137)"
            ));
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
    report(
        feature_map_problems(Path::new("."), &metadata()?)?,
        "feature-map",
    )
}

/// Every feature file of the map under `root`, sorted by name, with its text: every `.md` file in
/// [`FEATURE_MAP`] but its README.
fn feature_files(root: &Path) -> Result<Vec<(String, String)>> {
    let mut files = Vec::new();
    for entry in
        fs::read_dir(root.join(FEATURE_MAP)).with_context(|| format!("reading {FEATURE_MAP}"))?
    {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        if name.ends_with(".md") && name != "README.md" {
            let text = fs::read_to_string(&path)
                .with_context(|| format!("reading {FEATURE_MAP}/{name}"))?;
            files.push((name, text));
        }
    }
    files.sort();
    Ok(files)
}

/// The map's drift against the workspace under `root`, the checks the single `feature-map.md` had:
/// every workspace crate is named in some feature as `` `crate` ``, every reference-case fixture
/// as its path, and every repository path a feature names exists. Each feature file must also open
/// with its `# ` title, which is what `--index` lists.
fn feature_map_problems(root: &Path, meta: &Metadata) -> Result<Vec<String>> {
    let files = feature_files(root)?;
    let mut problems = Vec::new();
    if files.is_empty() {
        problems.push(format!("{FEATURE_MAP} has no feature files"));
    }
    let text: String = files
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for pkg in workspace_packages(meta) {
        if !text.contains(&format!("`{}`", pkg.name)) {
            problems.push(format!("crate `{}` has no entry", pkg.name));
        }
    }
    for entry in fs::read_dir(root.join("fixtures/refcases"))? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if !text.contains(&format!("fixtures/refcases/{name}")) {
            problems.push(format!("fixtures/refcases/{name} has no entry"));
        }
    }
    for (name, text) in &files {
        if feature_title(text).is_none() {
            problems.push(format!(
                "{FEATURE_MAP}/{name} does not open with a `# ` title"
            ));
        }
        for path in backticked_paths(text) {
            if !root.join(path).exists() {
                problems.push(format!("{name} names `{path}`, which does not exist"));
            }
        }
    }
    Ok(problems)
}

/// A feature file's title: its first line, when that is a `# ` heading.
fn feature_title(text: &str) -> Option<&str> {
    text.lines().next()?.strip_prefix("# ")
}

/// `cargo xtask feature-map --index`: the map's table of contents, derived from the files, so no
/// hand-kept list can drift or become a file every feature PR edits.
fn feature_map_index() -> Result<()> {
    for (name, text) in feature_files(Path::new("."))? {
        println!("{name}: {}", feature_title(&text).unwrap_or("(no title)"));
    }
    Ok(())
}

/// One of the [`ci_files`], by its path in the repository, with its text.
struct CiFile {
    path: String,
    text: String,
}

/// Every file that decides what CI or a release builds, each of which may be absent: workflows,
/// scripts and actions under `.github`, `deploy`'s scripts, and `.cargo/config.toml`.
fn ci_files(root: &Path) -> Result<Vec<CiFile>> {
    let mut files = Vec::new();
    for (dir, extensions) in [
        (".github/workflows", &["yml", "yaml"][..]),
        (".github/scripts", &["sh"][..]),
        (".github/actions", &["yml", "yaml"][..]),
        ("deploy", &["sh"][..]),
    ] {
        let dir = root.join(dir);
        if dir.is_dir() {
            files.extend(files_by_extension(&dir, extensions)?);
        }
    }
    let cargo_config = root.join(".cargo/config.toml");
    if cargo_config.is_file() {
        files.push(cargo_config);
    }
    files
        .into_iter()
        .map(|file| {
            let text =
                fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
            let path = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .display()
                .to_string();
            Ok(CiFile { path, text })
        })
        .collect()
}

/// [`live_feature_problems`] over the repository at `root`, as `cargo xtask live-feature` and
/// the lint job run it.
fn live_feature_in(root: &Path) -> Result<()> {
    eprintln!("    live-feature: checking that only the runner may build `live` (ES-23, DEC-529)");
    let policy: Layers = toml::from_str(&fs::read_to_string(root.join("xtask/layers.toml"))?)
        .context("parsing xtask/layers.toml")?;
    let files = ci_files(root)?;
    report(
        live_feature_problems(&policy, &metadata_in(root)?, &files)?,
        "live-feature",
    )
}

/// Every way the workspace and its CI break ES-23's ban on a `live` build, as DEC-529 item 3
/// narrows it for the founder's one live order (E7-26, X1): at most one crate is marked
/// `live_feature` in `policy`, and only it may declare a `live` feature; no other feature of it
/// (`default` included) and no dependency or feature of another crate turns `live` on; and CI
/// never passes `--all-features`, and passes `live` only in at most one `cargo check` of the
/// marked crate, so the feature compiles but no live build is ever produced or run.
fn live_feature_problems(policy: &Layers, meta: &Metadata, ci: &[CiFile]) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let packages = workspace_packages(meta);
    let names: BTreeSet<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    let marked: Vec<&str> = policy
        .crates
        .iter()
        .filter(|(_, crate_policy)| crate_policy.live_feature)
        .map(|(name, _)| name.as_str())
        .collect();
    if marked.len() > 1 {
        let listed: Vec<String> = marked.iter().map(|name| format!("`{name}`")).collect();
        problems.push(format!(
            "{} are marked `live_feature`; at most one crate, the runner, may be",
            listed.join(", ")
        ));
    }
    for name in &marked {
        if !names.contains(name) {
            problems.push(format!(
                "`{name}` is marked `live_feature` but is not a workspace member"
            ));
        }
    }
    let runner = match marked.as_slice() {
        [only] if names.contains(only) => Some(*only),
        _ => None,
    };
    for pkg in &packages {
        let is_runner = runner == Some(pkg.name.as_str());
        if pkg.features.contains_key(LIVE) && !is_runner {
            problems.push(format!(
                "`{}` declares a `{LIVE}` feature; only the crate marked `live_feature` may",
                pkg.name
            ));
        }
        for (feature, enables) in &pkg.features {
            for enabled in enables {
                let turns_on_own = is_runner && feature != LIVE && enabled == LIVE;
                let turns_on_other = enabled
                    .rsplit_once('/')
                    .is_some_and(|(_, feature)| feature == LIVE);
                if turns_on_own || turns_on_other {
                    problems.push(format!(
                        "`{}`'s feature `{feature}` turns on `{enabled}`; only an explicit \
                         `--features {LIVE}` may",
                        pkg.name
                    ));
                }
            }
        }
        for dep in &pkg.dependencies {
            if dep.features.iter().any(|f| f == LIVE) {
                problems.push(format!(
                    "`{}` turns on `{LIVE}` in its dependency on `{}`",
                    pkg.name, dep.name
                ));
            }
        }
    }
    let mut compile_only_seen = false;
    for file in ci {
        let yaml = file.path.ends_with(".yml") || file.path.ends_with(".yaml");
        let with_lines = with_block_lines(&file.text);
        for (number, line) in logical_lines(&file.path, &file.text) {
            let at = format!("{}:{number}", file.path);
            let ctx = LineContext {
                runner,
                cargo_config: file.path.ends_with(".cargo/config.toml"),
                with_value: yaml && with_lines.contains(&number),
            };
            match live_flag(&line, ctx) {
                LiveFlag::Absent => {}
                LiveFlag::AllFeatures => problems.push(format!(
                    "{at}: `--all-features` would build the `{LIVE}` feature"
                )),
                LiveFlag::Cfg => problems.push(format!(
                    "{at}: sets a feature through `--cfg feature=…`, which bypasses `--features`"
                )),
                LiveFlag::Unreadable => problems.push(format!(
                    "{at}: cargo words the shell expands or `xargs` gives cannot be checked"
                )),
                LiveFlag::CompileOnly if !compile_only_seen => compile_only_seen = true,
                LiveFlag::CompileOnly => problems.push(format!(
                    "{at}: a second compile-only `{LIVE}` job; CI may compile it once"
                )),
                LiveFlag::Build => problems.push(format!(
                    "{at}: passes `{LIVE}` outside the one `cargo check -p <runner>` CI may run"
                )),
            }
        }
    }
    Ok(problems)
}

/// The cargo feature that holds the live hosts (ES-23, DEC-529 item 3).
const LIVE: &str = "live";

/// What one line of a workflow, script or cargo configuration does with the `live` feature, worst
/// first: a line holding several commands takes the worst of theirs.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum LiveFlag {
    /// `live` passed to anything but the one compile-only form.
    Build,
    /// `--all-features`, which turns `live` on wherever it is declared.
    AllFeatures,
    /// `--cfg feature=…`, which sets a feature without `--features`.
    Cfg,
    /// A cargo command whose words the shell expands or `xargs` gives, which cannot be read.
    Unreadable,
    /// `cargo check -p <runner>` with `live`: the one form CI may run, which builds nothing it
    /// could run.
    CompileOnly,
    /// The line passes neither `live` nor a way around naming it.
    Absent,
}

/// A CI file's commands, each with the line it starts on: a line ending in `\` runs on, and a
/// workflow's folded (`>`, `>-`) or plain `key: value` runs on into each more-indented line, as
/// YAML folds it; a literal (`|`) block keeps its lines apart, as the shell's newlines do.
fn logical_lines(path: &str, text: &str) -> Vec<(usize, String)> {
    let yaml = path.ends_with(".yml") || path.ends_with(".yaml");
    let lines: Vec<&str> = text.lines().collect();
    let mut logical = Vec::new();
    let mut index = 0;
    while let Some(first) = lines.get(index) {
        let start = index.saturating_add(1);
        index = start;
        let (mut command, fold_below) = match yaml.then(|| yaml_value(first)).flatten() {
            Some((_, value)) if value.starts_with('|') => (String::new(), None),
            Some((column, value)) if value.starts_with('>') => (String::new(), Some(column)),
            Some((column, value)) if !value.is_empty() => (value.to_owned(), Some(column)),
            _ => ((*first).to_owned(), None),
        };
        while let Some(next) = lines.get(index) {
            let indent = next.len().saturating_sub(next.trim_start().len());
            let folds = fold_below.is_some_and(|column| indent > column) && !next.trim().is_empty();
            if let Some(head) = command.trim_end().strip_suffix('\\') {
                command = head.to_owned();
            } else if !folds {
                break;
            }
            command.push(' ');
            command.push_str(next.trim());
            index = index.saturating_add(1);
        }
        logical.push((start, command));
    }
    logical
}

/// A workflow line `key: value` or `- key: value`: the column its key starts at, and its value.
fn yaml_value(line: &str) -> Option<(usize, &str)> {
    let rest = line.trim_start_matches([' ', '-']);
    let (key, value) = rest.split_once(':')?;
    let plain_key = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    (plain_key && value.chars().next().is_none_or(char::is_whitespace))
        .then(|| (line.len().saturating_sub(rest.len()), value.trim()))
}

/// Whether `line` sets a feature through `--cfg`: `feature` then `=`, with any spaces, quotes or
/// backslashes between, wherever it appears (a command, `RUSTFLAGS`, or `rustflags`).
fn sets_cfg_feature(line: &str) -> bool {
    let bare = line.replace(['"', '\'', '\\'], "");
    bare.split("feature")
        .skip(1)
        .any(|after| after.trim_start().starts_with('='))
}

/// One token of a shell command line.
enum ShellToken {
    /// A word without its quotes; a `$` or backtick in it, however quoted, may expand.
    Word(String),
    /// A redirection whose target is the next word (`>`, `>>`, `2>`, `<`, `&>`, `>|`, `<<<`, …);
    /// in a descriptor copy such as `2>&1` the target is the descriptor (`1`).
    Redirect,
    /// What ends a command: `|`, `||`, `|&`, `&&`, `&`, `;`, `(`, `)`.
    End,
}

/// Splits `line` into tokens as the shell does: quotes group a word, a backslash escapes the next
/// character, and a word starting with `#` begins a comment. `||`, `|&` and `&&` are two ends,
/// and a redirection's descriptor (the `2` of `2>`) a word of its own, which cargo never reads.
fn shell_tokens(line: &str, nested: &mut Vec<String>) -> Vec<ShellToken> {
    let chars: Vec<char> = line.chars().collect();
    let (mut tokens, mut word, mut quote, mut index) = (Vec::new(), None, None, 0);
    while let Some(&c) = chars.get(index) {
        index = index.saturating_add(1);
        let ends = match (quote, c) {
            (Some(open), _) if c == open => {
                quote = None;
                None
            }
            (None, '\'' | '"') => {
                quote = Some(c);
                word.get_or_insert_with(String::new);
                None
            }
            (None, ' ' | '\t') => Some(None),
            (None, '#') if word.is_none() => break,
            (None, ';' | '(' | ')' | '|') => Some(Some(ShellToken::End)),
            (None, '&') if chars.get(index) != Some(&'>') => Some(Some(ShellToken::End)),
            (None, '<' | '>' | '&') => {
                while chars
                    .get(index)
                    .is_some_and(|d| matches!(d, '<' | '>' | '|' | '-'))
                {
                    index = index.saturating_add(1);
                }
                index = index.saturating_add(usize::from(chars.get(index) == Some(&'&')));
                Some(Some(ShellToken::Redirect))
            }
            (_, '\\') if quote != Some('\'') => {
                word.get_or_insert_with(String::new)
                    .extend(chars.get(index));
                index = index.saturating_add(1);
                None
            }
            _ => {
                if quote != Some('\'') && matches!(c, '$' | '`') {
                    substitution(&chars, c, &mut index, nested);
                }
                word.get_or_insert_with(String::new).push(c);
                None
            }
        };
        if let Some(token) = ends {
            tokens.extend(word.take().map(ShellToken::Word));
            tokens.extend(token);
        }
    }
    tokens.extend(word.map(ShellToken::Word));
    tokens
}

/// After a `$` or an opening backtick, reads a `$(...)` or backtick substitution to its close
/// and adds its inner text to `nested`, because the shell runs it as a command line of its own.
fn substitution(chars: &[char], start: char, index: &mut usize, nested: &mut Vec<String>) {
    let close = match (start, chars.get(*index)) {
        ('`', _) => '`',
        (_, Some('(')) => ')',
        _ => return,
    };
    *index = index.saturating_add(usize::from(start == '$'));
    let mut depth = 0_usize;
    let inner: String = chars
        .get(*index..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&c| {
            depth = match c {
                _ if c == close && depth == 0 => return false,
                '(' if close == ')' => depth.saturating_add(1),
                ')' => depth.saturating_sub(1),
                _ => depth,
            };
            true
        })
        .collect();
    *index = index
        .saturating_add(inner.chars().count())
        .saturating_add(1);
    nested.push(inner);
}

/// What a line of a CI file is read with: the runner, and what kind of line it is.
#[derive(Clone, Copy)]
struct LineContext<'a> {
    /// The crate marked `live_feature`, when exactly one workspace member is.
    runner: Option<&'a str>,
    /// A line of `.cargo/config.toml`, whose aliases and flags are all cargo's own.
    cargo_config: bool,
    /// A line inside a workflow's `with:` block: an action's input has no command word, so it
    /// may be cargo's arguments, and `-F` and a short-flag cluster are feature flags there.
    with_value: bool,
}

/// The numbers of the lines inside a workflow's `with:` blocks, the inputs a step passes to an
/// action: every line below `with:` that is blank or indented further than its key.
fn with_block_lines(text: &str) -> BTreeSet<usize> {
    let mut lines = BTreeSet::new();
    let mut block: Option<usize> = None;
    for (number, line) in (1_usize..).zip(text.lines()) {
        let indent = line.len().saturating_sub(line.trim_start().len());
        if block.is_some_and(|column| line.trim().is_empty() || indent > column) {
            lines.insert(number);
            continue;
        }
        block = yaml_value(line)
            .filter(|(_, value)| value.is_empty())
            .filter(|_| line.trim_start_matches([' ', '-']).starts_with("with:"))
            .map(|(column, _)| column);
    }
    lines
}

/// Judges each command of `line` on its own and keeps the worst verdict: the commands its
/// tokens' ends separate, each substitution, and each word holding a space, which `bash -c` or a
/// cargo alias may run (each shorter than `line`, so the reading ends), except the words of the
/// one compile-only command, whose quoted feature list names `live` by design. A redirection's
/// one target word is skipped, so it may hold a `$` (`>> "$GITHUB_OUTPUT"`); the words after it
/// are read. Every word, a redirection's target included, is also held to the cargo-word rule.
fn live_flag(line: &str, ctx: LineContext) -> LiveFlag {
    if sets_cfg_feature(line) {
        return LiveFlag::Cfg;
    }
    let (mut nested, mut words, mut verdict, mut target_next) =
        (Vec::new(), Vec::new(), LiveFlag::Absent, false);
    for token in shell_tokens(line, &mut nested)
        .into_iter()
        .chain([ShellToken::End])
    {
        if let ShellToken::Word(text) = &token
            && cargo_word_refused(text)
        {
            verdict = verdict.min(LiveFlag::Unreadable);
        }
        match token {
            ShellToken::Word(..) if target_next => target_next = false,
            ShellToken::Word(text) => words.push(text),
            ShellToken::Redirect => target_next = true,
            ShellToken::End => {
                let flag = command_live_flag(&words, ctx);
                verdict = verdict.min(flag);
                if flag != LiveFlag::CompileOnly {
                    if feature_values_refused(&words, ctx) {
                        verdict = verdict.min(LiveFlag::Unreadable);
                    }
                    nested.extend(
                        words
                            .drain(..)
                            .filter(|text| text.contains([' ', '\t']) && text.len() < line.len()),
                    );
                }
                words.clear();
                target_next = false;
            }
        }
    }
    nested
        .iter()
        .map(|inner| live_flag(inner, ctx))
        .fold(verdict, LiveFlag::min)
}

/// Whether `word` names cargo: a word `cargo`, or a path ending `/cargo`.
fn is_cargo(word: &str) -> bool {
    word == "cargo" || word.ends_with("/cargo")
}

/// Reads one command's words, if it is a cargo command (a word `cargo` or `…/cargo`, or any line
/// of the cargo configuration; `-F` is other tools' elsewhere, `gh api -F`), which cannot be read
/// when the shell expands a word or `xargs` gives the arguments. `--features <list>`,
/// `--features=<list>`, `-F <list>`, `-F<list>` and a short-flag cluster `-qF<list>` name
/// features, with items split on commas and spaces, each matched after any `<crate>/` or
/// `<crate>?/` prefix. A word is read without the brackets and commas of a cargo configuration
/// array (`["--features", "live"]`). Beside them, the live-token backstop: any word that holds
/// the token `live` ([`holds_live`]) and is not the feature list of the one compile-only form is
/// refused, whatever the command.
fn command_live_flag(words: &[String], ctx: LineContext) -> LiveFlag {
    let names: Vec<&str> = words
        .iter()
        .map(|w| w.trim_matches(['[', ']', ',']))
        .collect();
    if names.contains(&"--all-features") {
        return LiveFlag::AllFeatures;
    }
    let any_live = names.iter().any(|word| holds_live(word));
    let cargo_at = names.iter().position(|word| is_cargo(word));
    if !ctx.cargo_config && cargo_at.is_none() {
        return if any_live {
            LiveFlag::Build
        } else {
            LiveFlag::Absent
        };
    }
    let fed = names
        .get(..cargo_at.unwrap_or_default())
        .is_some_and(|before| before.contains(&"xargs"));
    let unreadable = if fed || names.iter().any(|word| word.contains(['$', '`'])) {
        LiveFlag::Unreadable
    } else {
        LiveFlag::Absent
    };
    let lists: Vec<(usize, &str)> = names
        .iter()
        .enumerate()
        .filter_map(|(index, word)| match *word {
            "--features" | "-F" => names
                .get(index.saturating_add(1))
                .map(|list| (index.saturating_add(1), *list)),
            _ => word
                .strip_prefix("--features=")
                .or_else(|| cluster_rest(word))
                .map(|list| (index, list)),
        })
        .collect();
    let passes_live = lists
        .iter()
        .flat_map(|(_, list)| list.split([',', ' ']))
        .any(|item| item.rsplit('/').next() == Some(LIVE));
    let stray_live = names
        .iter()
        .enumerate()
        .any(|(index, word)| !lists.iter().any(|(at, _)| *at == index) && holds_live(word));
    if stray_live || (any_live && !passes_live) {
        return LiveFlag::Build;
    }
    if !passes_live {
        return unreadable;
    }
    let checks = names
        .windows(2)
        .any(|pair| is_cargo(pair[0]) && pair[1] == "check");
    let packages: Vec<&str> = names
        .windows(2)
        .filter(|pair| pair[0] == "-p" || pair[0] == "--package")
        .map(|pair| pair[1])
        .collect();
    let whole_workspace = names.contains(&"--workspace") || names.contains(&"--all");
    let verdict = match (checks, packages.as_slice(), ctx.runner) {
        (true, [package], Some(runner)) if *package == runner && !whole_workspace => {
            LiveFlag::CompileOnly
        }
        _ => LiveFlag::Build,
    };
    verdict.min(unreadable)
}

/// The live-token backstop: whether `word`'s items, split on `,`, `=`, `/`, `?`, whitespace and
/// quotes, hold `live` exactly but in any case, or a short-flag cluster whose list does
/// (`-qFlive`). So `liveness` is another item, and `the-runner/live` holds it.
fn holds_live(word: &str) -> bool {
    word.split([',', '=', '/', '?', ' ', '\t', '"', '\''])
        .any(|item| item.eq_ignore_ascii_case(LIVE) || cluster_rest(item).is_some_and(holds_live))
}

/// The list after the first `F` of a short-flag cluster, `-<letters>F<list>` (`-qFlive`, `-Fa`).
fn cluster_rest(item: &str) -> Option<&str> {
    let (letters, rest) = item.strip_prefix('-')?.split_once('F')?;
    letters
        .chars()
        .all(|c| c.is_ascii_alphabetic())
        .then_some(rest)
}

/// Whether `word` holds the token `cargo`: a run of letters, digits, `_`, `-` and `.` that is
/// `cargo` in any case, so `!cargo` holds it and `.cargo` and `cargo-nextest` do not.
fn holds_cargo_token(word: &str) -> bool {
    word.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
        .any(|item| item.eq_ignore_ascii_case("cargo"))
}

/// The cargo-word rule: a word holding the token `cargo`, wherever it stands and whatever the
/// command, is refused when it also holds a `$`, a backtick or a `%` format directive, which can
/// build a flag no scan reads, or a feature flag whose value [`feature_value_refused`] refuses.
fn cargo_word_refused(word: &str) -> bool {
    holds_cargo_token(word)
        && (word.contains(['$', '`'])
            || format_directive(word)
            || flag_refused(word, None, true, false))
}

/// Whether `word` holds a `printf` format directive, `%` then flags, a width or a precision, and a
/// letter (`%s`, `%-8.3x`).
fn format_directive(word: &str) -> bool {
    word.split('%').skip(1).any(|after| {
        after
            .trim_start_matches(|c: char| matches!(c, '-' | '+' | ' ' | '#' | '0'..='9' | '.'))
            .starts_with(|c: char| c.is_ascii_alphabetic())
    })
}

/// The value rule over a command's words, whatever the command: each feature flag's value must
/// be a complete literal ([`feature_value_refused`]). `-F` is a feature flag only in a cargo
/// command, the cargo configuration or a `with:` value, or in a word holding the token `cargo`,
/// so `awk -F:` and `gh api -F owner="$o"` are not; a short-flag cluster is one only in a `with:`
/// value.
fn feature_values_refused(words: &[String], ctx: LineContext) -> bool {
    let cargo = ctx.with_value || ctx.cargo_config || words.iter().any(|word| is_cargo(word));
    words.iter().enumerate().any(|(index, word)| {
        let next = words.get(index.saturating_add(1)).map(String::as_str);
        flag_refused(word, next, cargo || holds_cargo_token(word), ctx.with_value)
    })
}

/// Whether `word`, read piece by piece on whitespace, holds `--all-features` or a feature flag
/// whose value is refused: `--features`, `--features=`, `FEATURES=`, `-F` and `-F<list>` when
/// `dash_f`, and a cluster `-qF<list>` when `cluster`. A flag that is the whole word takes `next`
/// as its value; one that ends the word without a value has the empty value.
fn flag_refused(word: &str, next: Option<&str>, dash_f: bool, cluster: bool) -> bool {
    let pieces: Vec<&str> = word.split_whitespace().collect();
    word.contains("--all-features")
        || pieces.iter().enumerate().any(|(index, piece)| {
            let value = if *piece == "--features" || (*piece == "-F" && dash_f) {
                Some(
                    pieces
                        .get(index.saturating_add(1))
                        .copied()
                        .or(next.filter(|_| pieces.len() == 1))
                        .unwrap_or_default(),
                )
            } else {
                piece
                    .strip_prefix("--features=")
                    .or_else(|| piece.strip_prefix("-F").filter(|_| dash_f))
                    .or_else(|| cluster_rest(piece).filter(|_| cluster))
                    .or_else(|| piece.split_once("FEATURES=").map(|(_, value)| value))
            };
            value.is_some_and(feature_value_refused)
        })
}

/// A feature value is read only as a complete literal of `[a-z0-9_,-]` without the token `live`;
/// any other value, an empty one or one holding `$`, a backtick or `${{` included, is refused.
fn feature_value_refused(value: &str) -> bool {
    value.is_empty()
        || !value
            .chars()
            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | ',' | '-'))
        || holds_live(value)
}

/// Code spans that look like repository paths: no spaces or globs, starting at a known root.
fn backticked_paths(text: &str) -> impl Iterator<Item = &str> {
    text.split('`').skip(1).step_by(2).filter(|span| {
        !span.contains(char::is_whitespace)
            && !span.contains(['*', '<', '{'])
            && REPO_ROOTS.iter().any(|root| span.starts_with(root))
    })
}

/// Mutation testing on the diff of safety-critical crates, `tool` crates included (ADR-0001 ES-11,
/// ES-12, DEC-253). Every mutant in changed source must be caught; approved exclusions live in
/// `.cargo/mutants.toml`. A crate whose tests are still pending runs the gate as well (DEC-137
/// amends DEC-83): there a missed mutant in a stub body is named and skipped, and every other one
/// still fails. The diff
/// is the change's net effect on its base, main's tip on a `pull_request` run (`choose_base`): a
/// line main already has was mutated when it landed there, so an edit main made independently is
/// not mutated again.
///
/// CI runs it once per shard of the matrix [`mutants_plan`] sized, and passes the plan's count in
/// [`MUTANTS_PLANNED_ENV`]; [`check_schedule`] refuses a shard whose own listing or shard total
/// disagrees with that plan (DEC-538).
fn mutants(root: &Path, base: Option<&str>) -> Result<()> {
    let shard = env::var(MUTANT_SHARD_ENV)
        .ok()
        .map(|value| MutantShard::parse(&value))
        .transpose()
        .with_context(|| format!("parsing {MUTANT_SHARD_ENV}"))?;
    let planned = env::var(MUTANTS_PLANNED_ENV)
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()
        .with_context(|| format!("parsing {MUTANTS_PLANNED_ENV}"))?;
    mutants_scheduled(root, base, shard, planned)
}

/// [`mutants`] with its schedule passed in rather than read from the environment, which the tests
/// cannot set (`unsafe` environment mutation is forbidden workspace-wide).
fn mutants_scheduled(
    root: &Path,
    base: Option<&str>,
    shard: Option<MutantShard>,
    planned: Option<usize>,
) -> Result<()> {
    let Some(diff) = gate_diff(root, base)? else {
        return check_schedule(0, shard, planned);
    };
    let listed = gate_mutants(root, &diff)?;
    check_schedule(listed.values().sum(), shard, planned)?;
    if listed.is_empty() {
        eprintln!("    mutants: the diff generates no mutants");
        return Ok(());
    }
    live_tests_judge_every_mutant(root, &listed)?;
    let mut test_packages: Vec<&str> = diff
        .crates
        .iter()
        .filter(|krate| {
            diff.touched
                .iter()
                .any(|file| file.starts_with(&krate.src_dir()))
        })
        .map(|krate| krate.package.as_str())
        .collect();
    external_oracles(&mut test_packages, &workspace_closure(&metadata_in(root)?));
    let args = mutants_args(diff.path()?, shard, &test_packages);
    fs::remove_dir_all(root.join(MUTANTS_OUT)).ok();
    eprintln!("    $ cargo {}", args.join(" "));
    let started = fs::metadata(&diff.file)
        .and_then(|meta| meta.modified())
        .context("timing the diff this run reads")?;
    let status = mutants_job_cargo(root, &args.iter().map(String::as_str).collect::<Vec<_>>())
        .status()
        .context("starting `cargo mutants` (is it installed? see AGENTS.md)");
    mutants_outcome(root, &diff.crates, status?.code(), started)
}

/// The source diff the mutation gate judges: the changed `.rs` files of safety-critical crates,
/// written to a file `cargo mutants --in-diff` reads. Removed when dropped.
struct GateDiff {
    crates: Vec<MutatedCrate>,
    touched: Vec<String>,
    file: PathBuf,
}

impl GateDiff {
    fn path(&self) -> Result<&str> {
        self.file.to_str().context("non-UTF-8 temp path")
    }
}

impl Drop for GateDiff {
    fn drop(&mut self) {
        fs::remove_file(&self.file).ok();
    }
}

/// The diff [`mutants`] and [`mutants_plan`] both judge, or `None`, saying why, when there is no
/// base or no safety-critical crate's source changed. One function, so the plan that sizes CI's
/// matrix and the gate each shard runs cannot select different source (DEC-538).
fn gate_diff(root: &Path, base: Option<&str>) -> Result<Option<GateDiff>> {
    let Some(base) = base else {
        eprintln!("    mutants: HEAD is the base; nothing to check");
        return Ok(None);
    };
    let crates = mutated_crates(root)?;
    let changed = output_in(
        root,
        "git",
        &["diff", "--name-only", &format!("{base}...HEAD")],
    )?;
    let touched: Vec<String> = changed
        .lines()
        .filter(|f| f.ends_with(".rs") && crates.iter().any(|c| f.starts_with(&c.src_dir())))
        .map(str::to_owned)
        .collect();
    if touched.is_empty() {
        eprintln!("    mutants: no safety-critical crate's source changed");
        return Ok(None);
    }
    let mut args = vec!["diff".to_owned(), format!("{base}...HEAD"), "--".to_owned()];
    args.extend(touched.iter().cloned());
    let text = output_in(
        root,
        "git",
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    let file = env::temp_dir().join(format!("mandate-mutants-{}.diff", std::process::id()));
    fs::write(&file, text)?;
    Ok(Some(GateDiff {
        crates,
        touched,
        file,
    }))
}

/// Every mutant the gate tests on `diff`, counted per package: `cargo mutants --list` with the
/// same `--in-diff` and the same `.cargo/mutants.toml` exclusions as the run, which reads that file
/// too. DEC-137's stub bodies are not filtered out here or in the run: their mutants are tested,
/// and only a miss in one is exempted afterwards, so they count toward a shard's load.
fn gate_mutants(root: &Path, diff: &GateDiff) -> Result<BTreeMap<String, usize>> {
    let listed = output_in(
        root,
        "cargo",
        &["mutants", "--list", "--json", "--in-diff", diff.path()?],
    )?;
    listed_mutant_counts(&listed)
}

/// The variable CI sets on each mutation shard to the count its plan reported (DEC-538).
const MUTANTS_PLANNED_ENV: &str = "MANDATE_MUTANTS_PLANNED";

/// The matrix width before DEC-538, and still its ceiling: DEC-498's 192 slice shards.
const MUTANT_SHARDS: usize = 192;

/// How CI's mutation matrix is sized for one diff (DEC-538): `mutants` is the count the gate tests
/// and `shards` is `min(MUTANT_SHARDS, mutants)`.
///
/// cargo-mutants' `slice` sharding gives shard `k` of `N` the `k`-th run of `ceil(n / N)` listed
/// mutants. For `n <= 192` that run is one mutant at either width, and for `n > 192` the width is
/// 192 as before, so shard `k` of the plan tests exactly what shard `k` of 192 tested, and every
/// shard the plan drops was one that tested nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MutantPlan {
    mutants: usize,
    shards: usize,
}

impl MutantPlan {
    fn for_mutants(mutants: usize) -> Self {
        Self {
            mutants,
            shards: mutants.min(MUTANT_SHARDS),
        }
    }

    /// The plan as `GITHUB_OUTPUT` lines: the count, the matrix's shard indices as a JSON array
    /// (`[]` for none), and the shard total each shard's `INDEX/TOTAL` names.
    fn outputs(self) -> String {
        let indices: Vec<String> = (0..self.shards).map(|k| k.to_string()).collect();
        format!(
            "mutants={}\nshards=[{}]\ntotal={}\n",
            self.mutants,
            indices.join(","),
            self.shards
        )
    }
}

/// `cargo xtask ci mutants --plan`: the mutant count the gate would test on this diff and the
/// matrix it needs, without building anything (DEC-538). It goes through [`gate_diff`] and
/// [`gate_mutants`], the gate's own selection.
fn mutants_plan(root: &Path, base: Option<&str>) -> Result<MutantPlan> {
    let mutants = match gate_diff(root, base)? {
        Some(diff) => gate_mutants(root, &diff)?.values().sum(),
        None => 0,
    };
    Ok(MutantPlan::for_mutants(mutants))
}

/// Refuses a shard whose schedule disagrees with the plan that launched it: a listing of another
/// size, or a shard total other than the plan's. Either would mean the shards no longer cover the
/// listing between them. Without a plan (a local run) any schedule is accepted.
fn check_schedule(listed: usize, shard: Option<MutantShard>, planned: Option<usize>) -> Result<()> {
    let Some(planned) = planned else {
        return Ok(());
    };
    if listed != planned {
        bail!(
            "this shard lists {listed} mutant(s) but the plan that sized the matrix counted \
             {planned}; the shards would not cover the gate's mutants (DEC-538)"
        );
    }
    if let Some(shard) = shard {
        let expected = MutantPlan::for_mutants(planned).shards;
        if shard.total != expected {
            bail!(
                "shard {} names a total of {} but a plan of {planned} mutant(s) runs {expected} \
                 shard(s) (DEC-538)",
                shard.argument(),
                shard.total
            );
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MutantShard {
    index: usize,
    total: usize,
}

impl MutantShard {
    fn parse(value: &str) -> Result<Self> {
        let Some((index, total)) = value.split_once('/') else {
            bail!("mutation shard `{value}` must have the form INDEX/TOTAL");
        };
        if total.contains('/') {
            bail!("mutation shard `{value}` must contain exactly one slash");
        }
        let index = index
            .parse::<usize>()
            .with_context(|| format!("mutation shard `{value}` has a non-numeric index"))?;
        let total = total
            .parse::<usize>()
            .with_context(|| format!("mutation shard `{value}` has a non-numeric total"))?;
        if total == 0 || index >= total {
            bail!(
                "mutation shard `{value}` must satisfy 0 <= INDEX < TOTAL and TOTAL must be positive"
            );
        }
        Ok(Self { index, total })
    }

    fn argument(self) -> String {
        format!("{}/{}", self.index, self.total)
    }
}

/// Adds [`REFCASES`] to the packages a mutation run tests when any package it mutates is one the
/// reference suites are built on, and reports whether it did.
///
/// A mutant is judged only by the tests cargo-mutants runs, and the oracle for several crates
/// lives in `mandate-refcases`, a package those crates do not depend on, so cargo-mutants never
/// discovers it (DEC-497). `closure` is every workspace package `mandate-refcases` is built on,
/// transitively, from cargo's own graph, so the reach is whatever the suites actually compile
/// against and nothing has to be written down and kept true. The whole package runs: narrowing to
/// the suites that cover the mutated crate was tried and withdrawn, because three review rounds
/// each found another crate a suite exercised and its declaration did not name, and because the
/// nine narrow suites together cost about a fifth of the three harnesses that every such
/// declaration reaches anyway (DEC-497).
fn external_oracles(
    packages: &mut Vec<&str>,
    closure: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    let Some(built_on) = closure.get(REFCASES) else {
        return false;
    };
    if packages.contains(&REFCASES) || !packages.iter().any(|package| built_on.contains(*package)) {
        return false;
    }
    packages.push(REFCASES);
    true
}

fn mutants_args(
    diff_path: &str,
    shard: Option<MutantShard>,
    test_packages: &[&str],
) -> Vec<String> {
    let mut args = [
        "mutants",
        "--in-diff",
        diff_path,
        "--test-tool",
        "nextest",
        "--jobs",
        MUTANT_JOBS,
        "--output",
        "target",
        "--timeout",
        MUTANT_TEST_TIMEOUT,
        "--build-timeout",
        MUTANT_BUILD_TIMEOUT,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    args.extend(
        test_packages
            .iter()
            .map(|package| format!("--test-package={package}")),
    );
    if let Some(shard) = shard {
        args.extend([
            "--shard".to_owned(),
            shard.argument(),
            "--sharding".to_owned(),
            "slice".to_owned(),
        ]);
    }
    args
}

/// The build directory a caller may export for their own builds. The mutants job's `cargo` children
/// that build — the pre-flight's test listing and the mutants run — take it out of their
/// environment: `cargo mutants` builds its baseline and each of the `--jobs` concurrent mutant
/// copies in scratch copies of the tree, and every build of the same package from every copy writes
/// the same artifact basename (`<package>-<metadata hash>`) into the target directory it is given,
/// so one directory shared by the baseline, the mutant copies, and the pre-flight's build of the
/// unmutated source can have one build's binary judge another's source and flip either verdict:
/// #419's review met a caught mutant reported missed, and
/// `a_callers_cargo_target_dir_cannot_flip_the_mutants_verdict` plants the opposite, a missed
/// mutant reported caught, the direction a gate must never fail in (#419 review, nit 3). With the
/// variable out of the environment each scratch copy builds in a target tree of its own, which is
/// how the run is judged when no directory is exported.
const CARGO_TARGET_DIR: &str = "CARGO_TARGET_DIR";

/// The profile every build of the mutants job runs under: no debuginfo, for the `dev` profile and
/// for the `test` profile that inherits it (DEC-519).
///
/// A mutant's build is the phase [`MUTANT_BUILD_TIMEOUT`] caps. The baseline builds only the
/// mutated packages, so a low-layer mutant's first build compiles the rest of its tested packages'
/// graph from nothing: a `mandate-time` mutant whose tested packages included `mandate-shell` ran
/// past the cap on CI's runner (#677). Without debuginfo that build took about a quarter less CPU
/// time from cold, and nearly half less after a one-file change, measured in DEC-519. Debuginfo
/// changes no test's verdict, only what a backtrace can name, and the gate reads verdicts. It is set
/// on the job's children rather than left to the caller, so CI and `cargo xtask check` build
/// mutants the same way whatever the caller exported.
const MUTANT_BUILD_PROFILE: [(&str, &str); 2] = [
    ("CARGO_PROFILE_DEV_DEBUG", "0"),
    ("CARGO_PROFILE_TEST_DEBUG", "0"),
];

/// A `cargo` child of the mutants job that builds, with the caller's [`CARGO_TARGET_DIR`] out of its
/// environment and [`MUTANT_BUILD_PROFILE`] in it: the pre-flight's `cargo nextest list`, which
/// builds the very packages the run is about to judge, and the `cargo mutants` run itself, whose
/// baseline and mutant builds inherit both. The job's other `cargo` children — `cargo mutants
/// --list` and `cargo metadata` — build nothing, so they carry none of this state.
fn mutants_job_cargo(root: &Path, args: &[&str]) -> Command {
    let mut cargo = Command::new("cargo");
    cargo
        .current_dir(root)
        .args(args)
        .env_remove(CARGO_TARGET_DIR)
        .envs(MUTANT_BUILD_PROFILE);
    cargo
}

/// The captured stdout of a [`mutants_job_cargo`] child, or its stderr in the failure, as
/// `output_in` reports a child that fails.
fn mutants_job_cargo_output(root: &Path, args: &[&str]) -> Result<String> {
    let out = mutants_job_cargo(root, args)
        .output()
        .context("starting `cargo nextest list` (is it installed? see AGENTS.md)")?;
    if !out.status.success() {
        bail!(
            "`cargo {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8(out.stdout).context("non-UTF-8 output")
}

/// Every mutant the run is about to test must have at least one live test that can judge it, or the
/// run's verdict on it is not evidence.
///
/// `cargo mutants` tests each mutant with `cargo nextest run --package=<the mutated crate>`, and
/// nextest answers a package whose every test is `#[ignore]`d with `0 tests run … error: no tests to
/// run` and a non-zero exit. `cargo mutants` 27.1.0 classifies a mutant on that exit status alone —
/// it has no outcome for "nothing ran" and no `--no-tests` option — so it records every one of them
/// as **caught**. A crate with no live test therefore reports a clean gate while nothing ran: on PR
/// #175 all 30 `mandate-builder` mutants came back caught and `BuilderError::code` returning `""`
/// survived every live test in the workspace. Counting the live tests here is what replaces that
/// exit code with evidence, and it fails the crate rather than skipping it, because a tests PR that
/// adds live code owes the gate something to judge it with (DEC-139).
///
/// The count is per package because that is the package `cargo mutants` runs tests from. Were a
/// workspace-wide test run ever configured, this count could only be stricter than it needs to be,
/// never looser.
///
/// The listing is [`gate_mutants`]'s, and it is never empty here: a diff with no mutants, such as one
/// that touches only `#[cfg(test)]` code, ends the job before this check, without starting a run
/// that could only test nothing.
fn live_tests_judge_every_mutant(root: &Path, mutants: &BTreeMap<String, usize>) -> Result<()> {
    let packages: Vec<String> = mutants
        .keys()
        .map(|package| format!("--package={package}"))
        .collect();
    let mut args = vec!["nextest", "list", "--locked", "--message-format", "json"];
    args.extend(packages.iter().map(String::as_str));
    eprintln!("    $ cargo {}", args.join(" "));
    let listing = mutants_job_cargo_output(root, &args)?;
    let live = live_test_counts(&listing)?;
    report(unjudged_mutants(mutants, &live), "mutants")
}

/// One mutant of `cargo mutants --list --json`. Only its package is read here: that is the package
/// whose tests the run judges it by.
#[derive(Deserialize)]
struct ListedMutant {
    package: String,
}

/// How many mutants the run will test in each package. `cargo mutants` 27.1.0 answers `--list
/// --json --in-diff` with no output at all, not `[]`, when the diff generates no mutant (a diff
/// that touches only `#[cfg(test)]` code; #227's round-2 delta review, #233), so empty output is
/// zero mutants; any other output that is not a JSON list is still an error.
fn listed_mutant_counts(listing: &str) -> Result<BTreeMap<String, usize>> {
    if listing.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let listed: Vec<ListedMutant> =
        serde_json::from_str(listing).context("parsing the mutants listing")?;
    let mut counts = BTreeMap::new();
    for mutant in listed {
        let count = counts.entry(mutant.package).or_insert(0usize);
        *count = count.saturating_add(1);
    }
    Ok(counts)
}

/// `cargo nextest list --message-format json`, read for which tests would actually run.
#[derive(Deserialize)]
struct NextestListing {
    #[serde(rename = "rust-suites")]
    rust_suites: BTreeMap<String, NextestSuite>,
}

#[derive(Deserialize)]
struct NextestSuite {
    #[serde(rename = "package-name")]
    package_name: String,
    testcases: BTreeMap<String, NextestCase>,
}

#[derive(Deserialize)]
struct NextestCase {
    ignored: bool,
}

/// How many live tests each package has: the tests that run, and so the tests that can fail, when
/// `cargo mutants` tests a mutant of that package. A package the listing does not mention has none,
/// which is the same verdict as a package whose every test is `#[ignore]`d.
fn live_test_counts(listing: &str) -> Result<BTreeMap<String, usize>> {
    let listing: NextestListing =
        serde_json::from_str(listing).context("parsing the nextest listing")?;
    let mut counts = BTreeMap::new();
    for suite in listing.rust_suites.into_values() {
        let live = suite
            .testcases
            .values()
            .filter(|case| !case.ignored)
            .count();
        let count = counts.entry(suite.package_name).or_insert(0usize);
        *count = count.saturating_add(live);
    }
    Ok(counts)
}

/// Names every package whose mutants no live test would judge, with what the run would have claimed
/// about them (DEC-139). A package of nothing but stubs is named too: DEC-137 item 3 exempts a stub
/// body's mutants from a run that *tested* them and found them missed, which a package with no live
/// test never produces.
fn unjudged_mutants(
    mutants: &BTreeMap<String, usize>,
    live: &BTreeMap<String, usize>,
) -> Vec<String> {
    mutants
        .iter()
        .filter(|(package, _)| live.get(package.as_str()).copied().unwrap_or(0) == 0)
        .map(|(package, count)| {
            format!(
                "`{package}`: no live tests to judge {count} mutant(s); it needs at least one \
                 live test before the gate can judge any of them, a crate of nothing but stubs \
                 included, since a stub body's mutants are exempt only on a run that tested them \
                 (#175 added live tests over its error codes and vocabulary). `cargo nextest run \
                 --package={package}` reports `no tests to run` and exits non-zero, which `cargo \
                 mutants` reads as every one of them caught, so the run would report a clean gate \
                 while nothing ran (DEC-139)"
            )
        })
        .collect()
}

/// Where `--output target` puts the run's outcomes. Removed before every run and required to be
/// newer than the diff the run reads, which is written moments before it starts, so nothing a
/// previous run or a restored cache left behind can be read as this run's result (DEC-137). The
/// diff's own timestamp is the reference because a clock read is disallowed here (ADR-0001 ES-05).
const MUTANTS_OUT: &str = "target/mutants.out";
const REFCASES: &str = "mandate-refcases";

/// How many mutants cargo-mutants tests at once.
///
/// One, not two, so that a single mutant stays short. DEC-498 gives a CI shard at most one mutant,
/// so there is nothing for a second worker to do there; this setting governs the unsharded local
/// run, where there is. Measured
/// on `ubuntu-24.04` over four runs of the same nine mutants, two workers contend: the per-mutant
/// test phase reaches 181 seconds at two and 83 at one. Those runs show no throughput difference
/// between the settings — 82 and 99 seconds a mutant at one worker, 83 and 108 at two — but two
/// runs each are too few to claim one either way, and none is needed.
const MUTANT_JOBS: &str = "1";

/// How long one mutant's tests may run before cargo-mutants calls it a timeout, in seconds.
///
/// Set explicitly because the value cargo-mutants derives is wrong for this gate and wrong in the
/// direction that fails a green change. It takes five times the unmutated baseline, floored at
/// twenty seconds, and it runs that baseline over the packages the shard's own mutants are in —
/// not over whatever `--test-package` names, so the [`REFCASES`] that [`external_oracles`] adds
/// reaches the baseline only when a mutant of it is in the slice. A shard mutating a core crate
/// therefore measures a second of that crate's tests, the floor gives twenty seconds, and a
/// mutant the reference harness catches in its thirtieth second is reported `TIMEOUT`, not caught.
/// DEC-498's first measurement run met exactly that: nine mutants, nine timeouts, every one at
/// the twenty-second cap.
///
/// Three minutes against the 83 seconds that was the slowest test phase over the two measured
/// runs at [`MUTANT_JOBS`] workers, so a mutant the harness judges at its ordinary pace is judged
/// rather than cut off. The margin is the reason the job count is one: at two workers the same
/// nine mutants reached 181 seconds, which this timeout would have cut off. A mutation slow
/// enough to reach the cap anyway is bounded by it, which is what lets DEC-498 size a shard on
/// this number rather than on how fast mutants have happened to run.
///
/// Unlike [`MUTANT_BUILD_TIMEOUT`], this one also caps the unmutated baseline's test phase, which
/// matters because a shard whose mutants are in [`REFCASES`] runs the whole reference harness as
/// its baseline. Shown by command: the same run at `--timeout 5` kills the baseline mid-suite and
/// reports `cargo test failed in an unmutated tree, so no mutants were tested`.
const MUTANT_TEST_TIMEOUT: &str = "180";

/// How long one mutant's build may run before cargo-mutants calls it a timeout, in seconds.
///
/// cargo-mutants leaves this off by default, on the reasoning that build times vary and a cap
/// risks flaky runs. This gate sets it because a mutant can in fact make a build arbitrarily
/// slow, which [`MUTANT_TEST_TIMEOUT`] does nothing about: operators are mutated wherever they
/// appear, top-level `const` items included, so `const N: usize = 1024 % 7` becomes
/// `1024 + 7` and any `[u8; N]` grows with it. DEC-498's budget needs every per-mutant phase
/// bounded, and this is the only phase a shard cannot otherwise bound.
///
/// Sixty seconds against the 16 to 27 that mutant builds took over the two measured runs at
/// [`MUTANT_JOBS`] workers. The flakiness cargo-mutants warns about is contained here because
/// this cap, unlike [`MUTANT_TEST_TIMEOUT`], reaches mutants alone: a `--build-timeout` run
/// leaves the unmutated baseline uncapped, shown by command, so a cold cache compiling the
/// workspace from scratch is never cut off. A mutant's build is not purely incremental on that
/// baseline, since it compiles [`REFCASES`] where the baseline may not have, but the measured
/// range already includes that.
const MUTANT_BUILD_TIMEOUT: &str = "60";

/// The status `cargo mutants` exits with when mutants survived and nothing else went wrong. Only
/// this status reads the outcomes and may exempt a stub body: a build failure, a diff that no longer
/// matches the source, a timeout, or a run that stopped before it tested anything keeps the failure,
/// whatever `target/mutants.out` holds (DEC-137).
const MISSED_MUTANTS: i32 = 2;

fn mutants_outcome(
    root: &Path,
    crates: &[MutatedCrate],
    code: Option<i32>,
    started: SystemTime,
) -> Result<()> {
    match code {
        Some(0) => Ok(()),
        Some(MISSED_MUTANTS) => stub_exemptions(root, crates, started),
        other => bail!(
            "`cargo mutants` exited with {other:?}, which is not the status it reports for missed \
             mutants ({MISSED_MUTANTS}); the run did not finish, so nothing in {MUTANTS_OUT} is \
             this run's result"
        ),
    }
}

/// A safety-critical crate the mutation gate covers, and whether its tests still carry pending
/// markers, which is what exempts a stub body of its own.
struct MutatedCrate {
    package: String,
    dir: String,
    pending: bool,
}

impl MutatedCrate {
    fn src_dir(&self) -> String {
        format!("{}/src/", self.dir)
    }
}

/// Every crate `xtask/layers.toml` marks `safety_critical = true`, whatever its layer: the
/// reference-case harness, a `tool` crate, is mutated too (DEC-253), since a harness line no test
/// can fail is a case that passes while checking nothing. Bugs seeded in the code it tests still
/// prove what its checks catch; mutating it proves each of its lines can fail. A crate with
/// `#[ignore = "pending <story>"]` tests is no longer excluded (DEC-137 amends DEC-83): a tests PR
/// carries stubs no live test runs, so only its stub bodies are exempt, and everything else it adds
/// is gated in the PR that adds it rather than one PR later (ADR-0001 ES-15).
fn mutated_crates(root: &Path) -> Result<Vec<MutatedCrate>> {
    let policy: Layers = toml::from_str(&fs::read_to_string(root.join("xtask/layers.toml"))?)
        .context("parsing xtask/layers.toml")?;
    let root = fs::canonicalize(root)?;
    let mut crates = Vec::new();
    for pkg in workspace_packages(&metadata_in(&root)?) {
        let Some(own) = policy.crates.get(&pkg.name) else {
            continue;
        };
        if own.safety_critical {
            let dir = pkg
                .manifest_path
                .parent()
                .context("manifest without a directory")?
                .strip_prefix(&root)
                .context("crate outside the repository")?;
            let dir = dir.display().to_string();
            let pending = has_pending_tests(&root, &dir)?;
            if pending {
                eprintln!(
                    "    mutants: `{}` has pending tests, so a missed mutant is a failure there \
                     unless the function it mutates is a stub (DEC-137)",
                    pkg.name
                );
            }
            crates.push(MutatedCrate {
                package: pkg.name.clone(),
                dir,
                pending,
            });
        }
    }
    Ok(crates)
}

/// The outcomes of the run `cargo mutants` writes under its output directory.
#[derive(Deserialize)]
struct MutantRun {
    outcomes: Vec<MutantOutcome>,
}

/// One scenario's result. `scenario` is `"Baseline"` for the unmutated run and `{"Mutant": ...}`
/// for every mutant, so the mutant is read out of the value rather than typed as an enum.
#[derive(Deserialize)]
struct MutantOutcome {
    scenario: serde_json::Value,
    summary: String,
}

#[derive(Deserialize)]
struct Mutant {
    name: String,
    package: String,
    file: String,
    function: Option<MutatedFunction>,
}

#[derive(Deserialize)]
struct MutatedFunction {
    span: Span,
}

#[derive(Deserialize)]
struct Span {
    start: LineColumn,
    end: LineColumn,
}

#[derive(Deserialize)]
struct LineColumn {
    line: usize,
}

/// Reads the outcomes of a run that missed mutants and fails unless every mutant it missed sits in a
/// stub body of a crate whose tests are still pending; those are named as skipped. Anything else
/// the run reports, a failed baseline or a timeout included, is a problem of its own (DEC-137).
fn stub_exemptions(root: &Path, crates: &[MutatedCrate], started: SystemTime) -> Result<()> {
    let outcomes = root.join(MUTANTS_OUT).join("outcomes.json");
    let written = fs::metadata(&outcomes)
        .and_then(|meta| meta.modified())
        .with_context(|| {
            format!(
                "reading {} after a run that missed mutants",
                outcomes.display()
            )
        })?;
    if written < started {
        bail!(
            "{} is older than the diff this run read, so it is not this run's result",
            outcomes.display()
        );
    }
    let text =
        fs::read_to_string(&outcomes).with_context(|| format!("reading {}", outcomes.display()))?;
    let (problems, skipped) = mutant_verdicts(root, crates, &text)
        .with_context(|| format!("reading {}", outcomes.display()))?;
    for name in &skipped {
        eprintln!(
            "    mutants: skipping `{name}`: a stub body in a crate whose tests are still pending, \
             so no live test can catch it until its story lands (DEC-83, DEC-137)"
        );
    }
    if !problems.is_empty() {
        return report(problems, "mutants");
    }
    if skipped.is_empty() {
        bail!("`cargo mutants` reports missed mutants that {MUTANTS_OUT} does not name");
    }
    eprintln!(
        "    mutants: {} mutant(s) survived, all in stub bodies; the implementation PR that \
         replaces them puts them under the gate",
        skipped.len()
    );
    Ok(())
}

/// The run's problems and the mutants it is allowed to skip: a missed mutant is a problem unless it
/// sits in a stub body of a crate whose tests are still pending, and every other summary the run
/// reports, a failed baseline or a timeout included, is a problem of its own (DEC-137).
fn mutant_verdicts(
    root: &Path,
    crates: &[MutatedCrate],
    outcomes: &str,
) -> Result<(Vec<String>, Vec<String>)> {
    let run: MutantRun = serde_json::from_str(outcomes).context("parsing the mutants outcomes")?;
    let mut problems = Vec::new();
    let mut skipped = Vec::new();
    for outcome in &run.outcomes {
        let summary = outcome.summary.as_str();
        let Some(mutated) = outcome.scenario.get("Mutant") else {
            if summary != "Success" {
                problems.push(format!("the unmutated baseline reports {summary}"));
            }
            continue;
        };
        let mutant: Mutant = serde_json::from_value(mutated.clone()).context("reading a mutant")?;
        match summary {
            "CaughtMutant" | "Unviable" | "Success" => {}
            "MissedMutant" if is_stub_mutant(root, crates, &mutant) => skipped.push(mutant.name),
            other => problems.push(format!(
                "{}: {other}, and the function it mutates is not a stub",
                mutant.name
            )),
        }
    }
    Ok((problems, skipped))
}

/// Whether a missed mutant is exempt: its crate's tests are still pending and the function it
/// mutates is a stub in the post-change source.
fn is_stub_mutant(root: &Path, crates: &[MutatedCrate], mutant: &Mutant) -> bool {
    let Some(krate) = crates.iter().find(|c| c.package == mutant.package) else {
        return false;
    };
    let Some(function) = &mutant.function else {
        return false;
    };
    if !krate.pending {
        return false;
    }
    let in_crate = root.join(&krate.dir).join(&mutant.file);
    let path = if in_crate.exists() {
        in_crate
    } else {
        root.join(&mutant.file)
    };
    let Ok(src) = fs::read_to_string(path) else {
        return false;
    };
    is_stub_function(&src, function.span.start.line, function.span.end.line)
}

/// Whether the function spanning these lines of `src` is a stub the mutation gate may exempt:
/// `Unimplemented` or `todo!()`, the two forms DEC-137 names, and no other always-failing body.
fn is_stub_function(src: &str, start: usize, end: usize) -> bool {
    let text = src
        .lines()
        .skip(start.saturating_sub(1))
        .take(end.saturating_sub(start).saturating_add(1))
        .collect::<Vec<_>>()
        .join("\n");
    let toks = tokens(&text);
    matches!(
        function_body(&toks, 0).and_then(stub_return).as_deref(),
        Some(UNIMPLEMENTED_VARIANT | TODO_PANIC)
    )
}

/// The variant every crate's stubs but `mandate-num`'s return, and the panic `todo!()` prints.
const UNIMPLEMENTED_VARIANT: &str = "Unimplemented";
const TODO_PANIC: &str = "not yet implemented";

/// The tokens inside the braces of the function whose `fn` is at or after `from`, or `None` for a
/// signature without a body.
fn function_body(toks: &[(Token, usize)], from: usize) -> Option<&[(Token, usize)]> {
    let open = toks
        .iter()
        .enumerate()
        .skip(from)
        .find_map(|(i, (t, _))| match t {
            Token::Punct('{') => Some(Some(i)),
            Token::Punct(';') => Some(None),
            _ => None,
        })??;
    let close = delimited_end(toks, open, '{', '}')?;
    toks.get(open.saturating_add(1)..close.saturating_sub(1))
}

/// The error a body that can only fail returns: nothing but argument discards (`let _ = ...;`) and a
/// last statement returning `Err(...)` or panicking through `todo!()`. The name is the last
/// capitalised one of the `Err(...)`, so `Err(GateError::Unimplemented("evaluate", "E6-3"))` gives
/// `Unimplemented` and `Err(NumError::Overflow)` gives `Overflow`. Such a body is not a stub by
/// itself: the mutation gate exempts only the two forms DEC-137 names, and the pending gate accepts
/// a name here only where `STUB_ERROR_EXCEPTIONS` declares it.
fn stub_return(body: &[(Token, usize)]) -> Option<String> {
    let statements = statements(body);
    let (last, discards) = statements.split_last()?;
    if !discards.iter().all(|s| discards_an_argument(s)) {
        return None;
    }
    let names: Vec<&str> = last
        .iter()
        .filter_map(|(t, _)| match t {
            Token::Ident(name) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let returned = names.strip_prefix(&["return"]).unwrap_or(&names);
    if matches!(returned, ["todo"] | ["unimplemented"]) {
        return Some("not yet implemented".to_owned());
    }
    if returned.first() != Some(&"Err") {
        return None;
    }
    returned
        .iter()
        .skip(1)
        .rfind(|name| name.starts_with(char::is_uppercase))
        .map(|name| (*name).to_owned())
}

/// The body's statements, split at every `;` outside a bracket.
fn statements(body: &[(Token, usize)]) -> Vec<&[(Token, usize)]> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (i, (t, _)) in body.iter().enumerate() {
        match t {
            Token::Punct('(' | '[' | '{') => depth = depth.saturating_add(1),
            Token::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            Token::Punct(';') if depth == 0 => {
                found.push(body.get(start..i).unwrap_or_default());
                start = i.saturating_add(1);
            }
            _ => {}
        }
    }
    let last = body.get(start..).unwrap_or_default();
    if !last.is_empty() {
        found.push(last);
    }
    found
}

fn discards_an_argument(statement: &[(Token, usize)]) -> bool {
    matches!(statement,
        [(Token::Ident(let_), _), (Token::Ident(hole), _), (Token::Punct('='), _), ..]
            if let_ == "let" && hole == "_")
}

fn has_pending_tests(root: &Path, dir: &str) -> Result<bool> {
    let files = output_in(
        root,
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
        if let Ok(text) = fs::read_to_string(root.join(file))
            && text.lines().any(is_pending_marker)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A DEC-77 tests PR marks a test `#[ignore = "pending <story>"]` because it cannot pass on the
/// PR's stubs. One that passes anyway pins nothing, so every pending test in the workspace is run
/// and must fail; a failure by panic, `todo!()` included, counts (DEC-110). The failure must also
/// name the story's stub, or a test that can never pass on correct code would pass the gate
/// (DEC-137). With no pending test there is nothing to run.
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
    let rows = behaviour_only_rows(root)?;
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
    let mut stale: Vec<String> = rows
        .iter()
        .filter(|row| {
            root.join(&row.file).exists() && !tests.iter().any(|t| row.names(&t.file, &t.test.path))
        })
        .map(|row| {
            format!(
                "{BEHAVIOUR_ONLY_DIR}/{} names `{}` in {}, which is no longer a pending test \
                 there; delete the row, which is how the exception expires (DEC-137)",
                row.file_name().unwrap_or_default(),
                row.test,
                row.file
            )
        })
        .collect();
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
        "    pending: {} pending test(s) must fail on this change's code, at their story's stub",
        tests.len()
    );
    let pinned = PENDING_PROPTEST_ENV
        .iter()
        .map(|(var, value)| format!("{var}={value}"))
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!("    $ {pinned} cargo {}", args.join(" "));
    let mut nextest = Command::new("cargo");
    nextest.current_dir(root).args(args);
    for (var, _) in env::vars_os() {
        if var.to_string_lossy().starts_with("PROPTEST_") {
            nextest.env_remove(var);
        }
    }
    let out = nextest
        .env("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1")
        .envs(PENDING_PROPTEST_ENV)
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
    let mut problems = verdicts(&tests, &outcomes, &rows);
    problems.append(&mut stale);
    if problems.is_empty() {
        eprintln!(
            "    pending: all {} fail at a stub, as pending tests must; nextest's `test run failed` above is expected",
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

/// The environment every pending property runs under in `ci pending`, so the gate gives one verdict
/// for one tree. The seed is the one every pending property draws its cases from: a pending
/// property's failure can depend on which cases it draws. #196's review and #199's measured three
/// `mandate-executor` properties pending E7-4
/// (`protective_sell_quantity_never_exceeds_the_position`,
/// `every_unprotected_interval_has_a_journaled_start_and_end`,
/// `no_interval_exceeds_the_limit_without_an_alert`) putting a later slice's stub report in their
/// output on some seeds and none on others, which turned the required `fast` check red and green on
/// the same code. A pinned seed cannot hide a pending test that passes: #199's review planted one
/// passing on every case and one failing on one input in 100,000, and the gate reported both as
/// passing.
///
/// A pinned seed alone was not one verdict per tree (DEC-164). The gate read a property's whole
/// output, and a property prints a panic for every failing case it tries while shrinking, so a
/// stub's report from a case shrinking moved past stood in for the failure proptest reports: on
/// #230 thirty `mandate-executor` properties whose minimal failure was a prefix assertion passed
/// on that incidental text. Proptest also replays the failures it saved in
/// `*.proptest-regressions` files beside the tests before drawing from the seed, so a checkout's
/// earlier runs, on other code, changed which cases a later run tried; and an inherited
/// `PROPTEST_*` variable (`PROPTEST_CASES`, `PROPTEST_MAX_SHRINK_ITERS`) changed what a property
/// that takes proptest's defaults runs. So the run clears every inherited `PROPTEST_*` variable,
/// sets these two, and [`failure_cause`] reads only the minimal failure proptest reports. Case
/// counts stay the ones each property names in its source, which is what they mean in `ci test`.
/// Live properties keep drawing a fresh seed and saving their failures in `ci test`, where variety
/// finds bugs; only the pending verdict is pinned.
const PENDING_PROPTEST_ENV: [(&str, &str); 2] = [
    ("PROPTEST_RNG_SEED", "20260927"),
    ("PROPTEST_DISABLE_FAILURE_PERSISTENCE", "1"),
];

/// How a stub reports itself, and the only evidence the gate accepts: `Unimplemented` is the
/// `Debug` of the variant every stub error carries, `unimplemented` its `code()`, "is not
/// implemented yet" and "<story> has not been implemented yet" the two `Display` forms in use, and
/// "not yet implemented" the panic of `todo!()`. A story id is not evidence: a test can write one
/// into its own assertion message, and a stub naming another story (`E6-1` under a `pending E6-5`
/// test) is still a stub. A crate whose stubs report none of these is fixed, not excused: that is
/// why `mandate-num`'s E4-2 stubs and `mandate-spec`'s `Confirmation::update` are `todo!()`
/// (DEC-137).
const STUB_MARKERS: [&str; 5] = [
    "Unimplemented",
    "unimplemented",
    "not implemented",
    "implemented yet",
    "not yet implemented",
];

/// The pending tests that fail on the answer a partly implemented crate gives rather than at a
/// stub, one TOML file a row, so that two PRs adding or deleting rows never touch the same lines.
/// What a row is, why each exists, and what the gate refuses are in the directory's README
/// (DEC-137).
const BEHAVIOUR_ONLY_DIR: &str = "xtask/behaviour-only";

/// The one file outside `crates/` a behaviour-only row may name: xtask's own unit tests, in its
/// `tests` module, which `cargo xtask ci pending` finds pending like any crate's.
const XTASK_OWN_TESTS: &str = "xtask/src/main.rs";

/// One row of [`BEHAVIOUR_ONLY_DIR`]: a pending test, by its file and its path as the pending
/// marker names it, and why it fails on behaviour rather than at its story's stub.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
struct BehaviourOnlyRow {
    file: String,
    test: String,
    reason: String,
}

impl BehaviourOnlyRow {
    /// The only name this row's file may have: `<crate>__<suite>__<test>.toml`, with `::` in the
    /// test path written `__`. One name per row is what makes a duplicate a path collision. A row
    /// of [`XTASK_OWN_TESTS`] is crate `xtask`, suite `main`, and its test must be in `tests`.
    fn file_name(&self) -> Option<String> {
        let (krate, suite) = if self.file == XTASK_OWN_TESTS {
            if !self.test.starts_with("tests::") {
                return None;
            }
            ("xtask", "main")
        } else {
            let rest = self.file.strip_prefix("crates/")?;
            let (krate, _) = rest.split_once('/')?;
            (krate, Path::new(rest).file_stem()?.to_str()?)
        };
        Some(format!(
            "{krate}__{suite}__{}.toml",
            self.test.replace("::", "__")
        ))
    }

    fn names(&self, file: &str, test: &str) -> bool {
        self.file == file && self.test == test
    }
}

/// Every row in [`BEHAVIOUR_ONLY_DIR`], sorted by file and test; none in a repository without the
/// directory, which only makes the gate stricter, since a row only excuses. Refuses anything there
/// but the README and `.toml` rows, a row that does not parse or has an empty field, and a file
/// whose name is not [`BehaviourOnlyRow::file_name`]. That name is a function of the row, so a
/// second copy of a row is a second file at the same path, which git refuses to merge.
fn behaviour_only_rows(root: &Path) -> Result<Vec<BehaviourOnlyRow>> {
    let dir = root.join(BEHAVIOUR_ONLY_DIR);
    let mut rows = Vec::new();
    if !dir.exists() {
        return Ok(rows);
    }
    let mut problems = Vec::new();
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
        .with_context(|| format!("reading {BEHAVIOUR_ONLY_DIR}"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        if name == "README.md" {
            continue;
        }
        if !name.ends_with(".toml") || !path.is_file() {
            problems.push(format!(
                "{BEHAVIOUR_ONLY_DIR}/{name} is not a row; the directory holds its README and \
                 one `.toml` file a row"
            ));
            continue;
        }
        let row: BehaviourOnlyRow = match fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|text| toml::from_str(&text).map_err(anyhow::Error::from))
        {
            Ok(row) => row,
            Err(err) => {
                problems.push(format!(
                    "{BEHAVIOUR_ONLY_DIR}/{name} is not a row of `file`, `test` and `reason`: \
                     {err}"
                ));
                continue;
            }
        };
        if [&row.file, &row.test, &row.reason]
            .iter()
            .any(|field| field.trim().is_empty())
        {
            problems.push(format!(
                "{BEHAVIOUR_ONLY_DIR}/{name} has an empty field; a row names its file, its test, \
                 and the reason it fails on behaviour"
            ));
            continue;
        }
        match row.file_name() {
            Some(expected) if expected == name => rows.push(row),
            Some(expected) => problems.push(format!(
                "{BEHAVIOUR_ONLY_DIR}/{name} names `{}` in {}, so it must be named {expected}",
                row.test, row.file
            )),
            None => problems.push(format!(
                "{BEHAVIOUR_ONLY_DIR}/{name} names `{}` in {}, which is not a file under \
                 `crates/<crate>/` nor a test in {XTASK_OWN_TESTS}'s `tests` module",
                row.test, row.file
            )),
        }
    }
    rows.sort();
    report(problems, "behaviour-only")?;
    Ok(rows)
}

/// Whether a pending test's failure output shows that it stopped at a stub. A one-word marker
/// matches as a whole word, so `Unimplemented` is not found in `Unimplementedish`; the phrases
/// match as they are printed.
fn names_a_stub(output: &str) -> bool {
    STUB_MARKERS.iter().any(|marker| {
        if marker.contains(' ') {
            output.contains(marker)
        } else {
            contains_word(output, marker)
        }
    })
}

/// The part of a failing test's output its verdict rests on. A property's output holds a panic for
/// every failing case proptest tried, and only the last is the failure it reports: `Test failed:
/// <why>.` over the minimal input, or `Test aborted: <why>` when it gave up. That reason alone is
/// the cause, so a stub's report from a case shrinking moved past, or from the input's `Debug`, is
/// not evidence (DEC-164). Any other test fails on one panic, and its whole output is read, as
/// DEC-137 says.
fn failure_cause(output: &str) -> &str {
    const FAILED: &str = "Test failed: ";
    const ABORTED: &str = "Test aborted: ";
    let reported = output.rmatch_indices("panicked at ").find_map(|(at, _)| {
        let (_, message) = output.get(at..)?.split_once('\n')?;
        (message.starts_with(FAILED) || message.starts_with(ABORTED)).then_some(message)
    });
    let Some(message) = reported else {
        return output;
    };
    let end = if message.starts_with(FAILED) {
        message.find(".\nminimal failing input:")
    } else {
        message.find('\n')
    };
    end.and_then(|end| message.get(..end)).unwrap_or(message)
}

/// The panic a failure starts with: the `panicked at` line and the message under it, or the first
/// lines of the output when the harness printed no panic.
fn first_panic_line(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let from = lines
        .iter()
        .position(|line| line.contains("panicked at"))
        .unwrap_or_default();
    let text = lines
        .get(from..)
        .unwrap_or_default()
        .iter()
        .take(2)
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return "no output".to_owned();
    }
    const KEPT: usize = 240;
    match text.char_indices().nth(KEPT) {
        Some((cut, _)) => format!("{}...", text.get(..cut).unwrap_or_default()),
        None => text,
    }
}

/// A problem for each pending test that passed in any binary, ran in none, or failed on something
/// other than its story's stub. The stub check comes first: a [`BEHAVIOUR_ONLY_DIR`] row applies
/// only to a test that fails it, and a listed test that stops at its stub anyway is a problem too,
/// naming the row to delete, so the exception can only shrink (#194 review, round 1, finding 4).
fn verdicts(
    tests: &[PendingTestRun],
    outcomes: &[TestOutcome],
    rows: &[BehaviourOnlyRow],
) -> Vec<String> {
    tests
        .iter()
        .filter_map(|t| {
            let runs: Vec<&TestOutcome> = outcomes
                .iter()
                .filter(|o| t.matches(&o.binary_id, &o.name))
                .collect();
            let at = format!(
                "{}:{}: `{}` (pending {})",
                t.file, t.test.line, t.test.path, t.test.story
            );
            if runs.is_empty() {
                Some(format!(
                    "{at} did not run, so nothing shows that it fails on this change's code"
                ))
            } else if runs.iter().any(|o| o.passed) {
                Some(format!(
                    "{at} passes on this change's code; a pending test must fail until its story \
                     is implemented, so make it assert what the stubs cannot satisfy (DEC-77)"
                ))
            } else {
                let listed = rows.iter().any(|row| row.names(&t.file, &t.test.path));
                let story = &t.test.story;
                match runs
                    .iter()
                    .find(|o| !names_a_stub(failure_cause(&o.output)))
                {
                    None if listed => Some(format!(
                        "{at} fails at its stub, so its behaviour-only row is not needed; \
                         delete the row, which is how the exception stays as small as it must \
                         be (DEC-137)"
                    )),
                    None => None,
                    Some(_) if listed => {
                        eprintln!(
                            "    pending: {}:{}: `{}` fails on a partly implemented crate's \
                             answer, not at a stub; it is named in {BEHAVIOUR_ONLY_DIR} until {} \
                             lands",
                            t.file, t.test.line, t.test.path, story
                        );
                        None
                    }
                    Some(o) => Some(format!(
                        "{at} fails away from its stub; it must stop at the stub {story} \
                         implements, so its failure must carry that stub's own report, which is \
                         the `Unimplemented` error of its crate, and neither a fixture, parse, or \
                         harness panic nor the test's own story id counts (DEC-137). It panicked \
                         with: {}",
                        first_panic_line(failure_cause(&o.output))
                    )),
                }
            }
        })
        .collect()
}

/// One finished test from nextest's `libtest-json` output, with what a failing one printed: the
/// panic the gate reads to tell a stub failure from any other (DEC-137).
#[derive(Debug, PartialEq)]
struct TestOutcome {
    binary_id: String,
    name: String,
    passed: bool,
    output: String,
}

#[derive(Deserialize)]
struct LibtestEvent {
    #[serde(rename = "type")]
    kind: String,
    event: String,
    name: Option<String>,
    stdout: Option<String>,
    stderr: Option<String>,
    message: Option<String>,
}

/// Test events are named `<binary id>$<test name>`; `started` is not a result, and neither is
/// `ignored`, which is a test the run skipped rather than one that failed (DEC-137).
fn test_outcomes(stdout: &str) -> Vec<TestOutcome> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<LibtestEvent>(line).ok())
        .filter(|e| e.kind == "test" && e.event != "started" && e.event != "ignored")
        .filter_map(|e| {
            let LibtestEvent {
                kind: _,
                event,
                name,
                stdout,
                stderr,
                message,
            } = e;
            let (binary_id, name) = name?
                .split_once('$')
                .map(|(b, n)| (b.to_owned(), n.to_owned()))?;
            Some(TestOutcome {
                binary_id,
                name,
                passed: event == "ok",
                output: [stdout, stderr, message]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("\n"),
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
                if let Some((story, end)) = pending_attribute(&toks, i)
                    && let Some((name, line)) = function_after(&toks, end)
                {
                    let path = mods
                        .iter()
                        .map(|(m, _)| m.as_str())
                        .chain([name.as_str()])
                        .collect::<Vec<_>>()
                        .join("::");
                    found.push(PendingTest { path, story, line });
                }
            }
            _ => {}
        }
        i += 1;
    }
    found
}

/// The story of the `#[ignore = "pending <story>"]` attribute whose `#` is at `start`, and the
/// index just past it.
fn pending_attribute(toks: &[(Token, usize)], start: usize) -> Option<(String, usize)> {
    let end = attribute_end(toks, start)?;
    match toks.get(start + 2..end.checked_sub(1)?).unwrap_or_default() {
        [
            (Token::Ident(ignore), _),
            (Token::Punct('='), _),
            (Token::Str(reason), _),
        ] if ignore == "ignore" && is_pending_reason(reason) => {
            Some((reason.trim_start_matches("pending ").to_owned(), end))
        }
        _ => None,
    }
}

/// The index just past the `]` closing the attribute whose `#` is at `start`.
fn attribute_end(toks: &[(Token, usize)], start: usize) -> Option<usize> {
    delimited_end(toks, start + 1, '[', ']')
}

/// The index just past the delimiter closing the one that opens at `start`.
fn delimited_end(toks: &[(Token, usize)], start: usize, open: char, close: char) -> Option<usize> {
    if toks.get(start).map(|(t, _)| t) != Some(&Token::Punct(open)) {
        return None;
    }
    let mut depth = 0usize;
    for (i, (t, _)) in toks.iter().enumerate().skip(start) {
        match t {
            Token::Punct(c) if *c == open => depth += 1,
            Token::Punct(c) if *c == close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Pending markers the source scan cannot turn into a runnable test name, with why: one inside a
/// `macro_rules!` body, one on a `$`-named function a macro expands, and one the scan cannot attach
/// to a named `fn` at all, which is what a marker forwarded to a macro call
/// (`case!(#[ignore = "pending E6-4"] name)`) looks like. Each leaves the test ungated, which is how
/// 85 of `mandate-risk`'s 105 pending tests went unrun, so each fails `cargo xtask markers`
/// (DEC-110, DEC-137).
fn generated_pending_markers(src: &str) -> Vec<(usize, &'static str)> {
    let toks = tokens(src);
    let tok = |i: usize| toks.get(i).map(|(t, _)| t);
    let mut macro_bodies: Vec<(usize, usize)> = Vec::new();
    for (i, (t, _)) in toks.iter().enumerate() {
        if let Token::Ident(kw) = t
            && kw == "macro_rules"
            && tok(i + 1) == Some(&Token::Punct('!'))
            && let Some(open) =
                (i + 2..i + 4).find(|&j| matches!(tok(j), Some(Token::Punct('{' | '(' | '['))))
            && let Some(end) = match tok(open) {
                Some(Token::Punct('{')) => delimited_end(&toks, open, '{', '}'),
                Some(Token::Punct('(')) => delimited_end(&toks, open, '(', ')'),
                _ => delimited_end(&toks, open, '[', ']'),
            }
        {
            macro_bodies.push((open, end));
        }
    }
    let mut found = Vec::new();
    for (i, (t, line)) in toks.iter().enumerate() {
        if *t != Token::Punct('#') {
            continue;
        }
        let Some((_, end)) = pending_attribute(&toks, i) else {
            continue;
        };
        if macro_bodies.iter().any(|(from, to)| i > *from && i < *to) {
            found.push((*line, "inside a `macro_rules!` body"));
        } else {
            match fn_after(&toks, end) {
                Some(f) if tok(f + 1) == Some(&Token::Punct('$')) => {
                    found.push((*line, "on a `$`-named function"));
                }
                Some(_) => {}
                None => found.push((*line, "on no named function of its own")),
            }
        }
    }
    found
}

/// The name and line of the function an attribute list ending at `start` belongs to.
fn function_after(toks: &[(Token, usize)], start: usize) -> Option<(String, usize)> {
    match toks.get(fn_after(toks, start)?.checked_add(1)?) {
        Some((Token::Ident(name), line)) => Some((name.clone(), *line)),
        _ => None,
    }
}

/// The index of the `fn` an attribute list ending at `start` belongs to, past any further
/// attributes and qualifiers (`pub(crate)`, `async`, `unsafe`, `extern "C"`).
fn fn_after(toks: &[(Token, usize)], start: usize) -> Option<usize> {
    let qualifiers = [
        "pub", "crate", "super", "self", "in", "async", "unsafe", "const", "extern",
    ];
    let mut i = start;
    loop {
        match toks.get(i) {
            Some((Token::Punct('#'), _)) => i = attribute_end(toks, i)?,
            Some((Token::Ident(q), _)) if qualifiers.contains(&q.as_str()) => i += 1,
            Some((Token::Punct('(' | ')') | Token::Str(_), _)) => i += 1,
            Some((Token::Ident(kw), _)) if kw == "fn" => return Some(i),
            _ => return None,
        }
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

/// The commit a change is compared against, from this process's environment: `MANDATE_BASE_REF`
/// (the explicit override) and `GITHUB_EVENT_NAME` (set by GitHub Actions).
fn base_ref() -> Result<Option<String>> {
    base_ref_in(
        Path::new("."),
        env::var("MANDATE_BASE_REF").ok().as_deref(),
        env::var("GITHUB_EVENT_NAME").ok().as_deref(),
    )
}

/// Reads what `choose_base` needs from the repository at `root`.
fn base_ref_in(root: &Path, explicit: Option<&str>, event: Option<&str>) -> Result<Option<String>> {
    let git = |args: &[&str]| {
        output_in(root, "git", args)
            .ok()
            .map(|out| out.trim().to_owned())
    };
    let head = git(&["rev-parse", "HEAD"]).unwrap_or_default();
    let parents = git(&["rev-list", "--parents", "-n", "1", "HEAD"])
        .map(|line| line.split_whitespace().skip(1).map(str::to_owned).collect())
        .unwrap_or_default();
    choose_base(&BaseInputs {
        explicit: explicit.map(str::to_owned),
        event: event.map(str::to_owned),
        head,
        parents,
        main_merge_base: git(&["merge-base", "HEAD", "origin/main"]),
    })
}

/// What decides the commit a change is compared against.
struct BaseInputs {
    /// `MANDATE_BASE_REF`, the explicit override for a local run.
    explicit: Option<String>,
    /// `GITHUB_EVENT_NAME`, the event a GitHub Actions run answers.
    event: Option<String>,
    head: String,
    /// HEAD's parents, first parent first.
    parents: Vec<String>,
    /// `git merge-base HEAD origin/main`, when there is an `origin/main`.
    main_merge_base: Option<String>,
}

/// The commit a change is compared against; the diff-based jobs (`spec-guard`, the trailer check,
/// `mutants`, gitleaks' range) read `<base>...HEAD` or `<base>..HEAD`.
///
/// 1. A non-empty `MANDATE_BASE_REF` wins. An all-zero one is what a forge sends for a branch's
///    first push, meaning there is no base.
/// 2. On a `pull_request` event CI checks out the merge of the PR into main's tip as of the event,
///    so the base is that merge commit's first parent. The PR's `base.sha` is not: it is main as
///    of the PR's opening or last retarget, and against it every later change on main counts as
///    the PR's own (#243, #258).
/// 3. Otherwise the merge base with `origin/main`, and none when that is HEAD itself (a run on
///    main).
///
/// A pull request always has a base, so a `pull_request` run that resolves none is an error, not
/// a gate with nothing to check: a shallow checkout (no parents, no `origin/main`) would otherwise
/// pass every diff-based job having compared nothing.
fn choose_base(inputs: &BaseInputs) -> Result<Option<String>> {
    let pull_request = inputs.event.as_deref() == Some("pull_request");
    let base = if let Some(explicit) = inputs.explicit.as_deref().filter(|base| !base.is_empty()) {
        (!explicit.chars().all(|c| c == '0')).then(|| explicit.to_owned())
    } else if let (true, [main_tip, _pr_head]) = (pull_request, inputs.parents.as_slice()) {
        Some(main_tip.clone())
    } else {
        inputs
            .main_merge_base
            .clone()
            .filter(|merge_base| *merge_base != inputs.head)
    };
    if pull_request && base.is_none() {
        bail!(
            "a pull_request run resolved no diff base: HEAD has {} parent(s) and origin/main gives none; check out with `fetch-depth: 0` so the merge commit keeps both parents",
            inputs.parents.len()
        );
    }
    Ok(base)
}

/// Commits a change adds carry no `Co-authored-by` trailer: Cursor's agent hook fills it with the
/// founder's email, which must not enter the history.
fn commit_trailers() -> Result<()> {
    let Some(base) = base_ref()? else {
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
    let Some(base) = base_ref()? else {
        eprintln!("    spec-guard: HEAD is the base; nothing to check");
        return Ok(());
    };
    let pr_body = env::var("MANDATE_PR_BODY").unwrap_or_default();
    report(
        spec_guard_problems(Path::new("."), &base, &pr_body)?,
        "spec-guard",
    )
}

/// Protected paths (ES-22) changed since `base` need a DEC cited in the PR description or a commit
/// message, and may not ship with code.
fn spec_guard_problems(root: &Path, base: &str, pr_body: &str) -> Result<Vec<String>> {
    spec_guard_problems_for_pr(root, base, pr_body, current_pr_number(root)?)
}

fn spec_guard_problems_for_pr(
    root: &Path,
    base: &str,
    pr_body: &str,
    pr_number: Option<u64>,
) -> Result<Vec<String>> {
    let changed = output_in(
        root,
        "git",
        &["diff", "--name-only", &format!("{base}...HEAD")],
    )?;
    let (protected, code) = classify(changed.lines());
    let mut problems = status_flip_problems(root, base)?;
    if protected.is_empty() {
        eprintln!("    spec-guard: no protected paths changed");
        return Ok(problems);
    }
    let messages = format!(
        "{pr_body}\n{}",
        output_in(
            root,
            "git",
            &["log", "--format=%B", &format!("{base}..HEAD")]
        )?
    );
    if !contains_dec_id(&messages) {
        problems.push(format!(
            "protected paths changed ({}) but no DEC-<n> is cited in the PR description or commits",
            protected.len()
        ));
    }
    if !code.is_empty() {
        if e77_atomic_exception(pr_body, pr_number) {
            eprintln!(
                "    spec-guard: founder-approved E7-7 atomic exception applies to PR #598 (DEC-462)"
            );
        } else {
            problems.push(format!("specs, schemas, or reference cases changed together with code ({} code files); split the change", code.len()));
        }
    }
    Ok(problems)
}

fn current_pr_number(root: &Path) -> Result<Option<u64>> {
    if let Some(value) = env::var(PR_NUMBER)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return value
            .parse()
            .with_context(|| format!("{PR_NUMBER} must be an unsigned integer"))
            .map(Some);
    }
    let branch = output_in(root, "git", &["branch", "--show-current"])?;
    Ok((branch.trim() == E77_ATOMIC_BRANCH).then_some(E77_ATOMIC_PR))
}

fn e77_atomic_exception(pr_body: &str, pr_number: Option<u64>) -> bool {
    pr_number == Some(E77_ATOMIC_PR) && pr_body.lines().any(|line| line.trim() == E77_ATOMIC_MARKER)
}

/// The founder-owned reference-case status file (ADR-0001 ES-11).
const STATUS_TOML: &str = "crates/mandate-refcases/status.toml";

/// ES-11 as DEC-277 amends it: a case `status.toml` marks `passing` at `base` never goes absent, and
/// goes `pending` only when the same change edits that case's entry in `fixtures/refcases/`, which is
/// a spec or reference change to what the case asserts. Compares `base` with the working tree; a case
/// whose entry no fixture names by `id` counts as unchanged, so its flip is refused.
fn status_flip_problems(root: &Path, base: &str) -> Result<Vec<String>> {
    let Ok(before) = output_in(root, "git", &["show", &format!("{base}:{STATUS_TOML}")]) else {
        return Ok(Vec::new());
    };
    let before = statuses(&before).context("status.toml at the base")?;
    let after = match fs::read_to_string(root.join(STATUS_TOML)) {
        Ok(text) => statuses(&text).context("status.toml in the working tree")?,
        Err(_) => BTreeMap::new(),
    };
    let mut problems = Vec::new();
    for ((suite, case), status) in &before {
        if status != "passing" {
            continue;
        }
        match after
            .get(&(suite.clone(), case.clone()))
            .map(String::as_str)
        {
            Some("passing") => {}
            None => problems.push(format!(
                "status.toml: `{suite}::{case}` went from passing to absent; a passing case never \
                 regresses (ES-11, DEC-277)"
            )),
            Some(_) => {
                if !fixture_entry_changed(root, base, suite, case)? {
                    problems.push(format!(
                        "status.toml: `{suite}::{case}` went from passing to pending, but its entry in \
                         fixtures/refcases/ did not change; only a change to what the case asserts may \
                         take it back to pending (ES-11, DEC-277)"
                    ));
                }
            }
        }
    }
    Ok(problems)
}

/// Every case's status, keyed by suite and case ID.
fn statuses(text: &str) -> Result<BTreeMap<(String, String), String>> {
    let table: toml::Table = text.parse()?;
    let mut out = BTreeMap::new();
    for (suite, cases) in &table {
        let cases = cases
            .as_table()
            .with_context(|| format!("`{suite}` must be a table"))?;
        for (case, entry) in cases {
            let status = entry
                .get("status")
                .and_then(toml::Value::as_str)
                .with_context(|| format!("`{suite}::{case}` has no status"))?;
            out.insert((suite.clone(), case.clone()), status.to_owned());
        }
    }
    Ok(out)
}

/// Whether the entry whose `id` is `case`, anywhere in `fixtures/refcases/<suite>.json` (`_` read as
/// `-`), differs between `base` and the working tree. An entry on one side only counts as changed.
fn fixture_entry_changed(root: &Path, base: &str, suite: &str, case: &str) -> Result<bool> {
    let path = format!("fixtures/refcases/{}.json", suite.replace('_', "-"));
    let before: Option<serde_json::Value> =
        output_in(root, "git", &["show", &format!("{base}:{path}")])
            .ok()
            .map(|text| serde_json::from_str(&text))
            .transpose()
            .with_context(|| format!("{path} at the base"))?;
    let after: Option<serde_json::Value> = fs::read_to_string(root.join(&path))
        .ok()
        .map(|text| serde_json::from_str(&text))
        .transpose()
        .with_context(|| format!("{path} in the working tree"))?;
    let entry = |doc: &Option<serde_json::Value>| {
        doc.as_ref().and_then(|d| entry_with_id(d, case)).cloned()
    };
    Ok(entry(&before) != entry(&after))
}

/// The first object, depth first, whose `id` is `id`.
fn entry_with_id<'a>(value: &'a serde_json::Value, id: &str) -> Option<&'a serde_json::Value> {
    match value {
        serde_json::Value::Object(map)
            if map.get("id").and_then(serde_json::Value::as_str) == Some(id) =>
        {
            Some(value)
        }
        serde_json::Value::Object(map) => map.values().find_map(|v| entry_with_id(v, id)),
        serde_json::Value::Array(items) => items.iter().find_map(|v| entry_with_id(v, id)),
        _ => None,
    }
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
    use std::cell::Cell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::env;
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Duration;

    use anyhow::{Context, Result};
    use serde_json::{Value, json};

    use super::{
        BEHAVIOUR_ONLY_DIR, BehaviourOnlyRow, CARGO_TARGET_DIR, CiFile, CratePolicy, Dependency,
        FEATURE_MAP, Layers, MUTANT_BUILD_TIMEOUT, MUTANT_SHARDS, MUTANT_TEST_TIMEOUT, MUTANTS_OUT,
        Metadata, MutantPlan, MutantShard, MutatedCrate, Package, PendingTest, PendingTestRun,
        REFCASES, TestOutcome, actionlint_workflows, backticked_paths, base_ref_in,
        behaviour_only_rows, check_schedule, ci, ci_files, classify, contains_dec_id,
        contains_word, external_oracles, failure_cause, feature_files, feature_map_problems,
        files_by_extension, first_panic_line, forbidden_reached, generated_pending_markers,
        has_pending_tests, is_pending_marker, is_stub_function, layer_problems, lint,
        listed_mutant_counts, live_feature_problems, live_test_counts, metadata_in,
        mutant_verdicts, mutants, mutants_args, mutants_job_cargo, mutants_outcome, mutants_plan,
        mutants_scheduled, mutated_crates, names_a_stub, output_in, pending_problems,
        pending_tests, plain_comment_lines, proptest_seeds_in, repo_root, shellcheck_scripts,
        spec_guard_problems, spec_guard_problems_for_pr, status_flip_problems, test_binary,
        test_outcomes, unjudged_mutants, verdicts, workspace_closure, workspace_packages,
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
    fn mutation_shards_accept_only_zero_based_indices_below_the_total() -> Result<()> {
        assert_eq!(
            MutantShard::parse("0/12")?,
            MutantShard {
                index: 0,
                total: 12
            }
        );
        assert_eq!(
            MutantShard::parse("11/12")?,
            MutantShard {
                index: 11,
                total: 12
            }
        );
        for invalid in ["12/12", "13/12", "0/0", "1", "1/2/3", "a/12"] {
            assert!(
                MutantShard::parse(invalid).is_err(),
                "{invalid} must not select an incomplete or undefined shard"
            );
        }
        Ok(())
    }

    #[test]
    fn a_crate_that_reaches_a_forbidden_crate_is_a_problem_and_one_that_does_not_is_none() {
        let forbidden = ["mandate-alpaca".to_owned(), "mandate-paper".to_owned()];
        let through_a_chain: BTreeSet<String> =
            ["mandate-cli", "mandate-journal", "mandate-alpaca"]
                .into_iter()
                .map(str::to_owned)
                .collect();
        assert_eq!(
            forbidden_reached("mandate-cli", &forbidden, &through_a_chain),
            [
                "`mandate-cli` reaches `mandate-alpaca`, which its forbidden_internal list in \
              xtask/layers.toml rules out (DEC-525)"
            ],
            "a forbidden crate reached through any chain is named, and only the one reached"
        );
        let clear: BTreeSet<String> = ["mandate-cli", "mandate-journal"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert!(forbidden_reached("mandate-cli", &forbidden, &clear).is_empty());
        assert!(
            forbidden_reached("mandate-cli", &[], &through_a_chain).is_empty(),
            "a crate with no forbidden_internal list is held only by its layer"
        );
    }

    /// The policy as written holds the CLI away from the connector, the shell and the paper
    /// binary, and the workspace as built respects it (DEC-525).
    #[test]
    fn the_cli_reaches_no_crate_that_can_place_an_order() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let policy: Layers = toml::from_str(&fs::read_to_string(root.join("xtask/layers.toml"))?)?;
        let cli = policy
            .crates
            .get("mandate-cli")
            .context("mandate-cli has a policy")?;
        assert_eq!(
            cli.forbidden_internal,
            ["mandate-alpaca", "mandate-paper", "mandate-shell"],
            "the CLI may never reach the connector, the paper binary or the shell"
        );
        let meta = metadata_in(&root)?;
        let reached = workspace_closure(&meta);
        let reached = reached
            .get("mandate-cli")
            .context("mandate-cli is a member")?;
        assert!(forbidden_reached("mandate-cli", &cli.forbidden_internal, reached).is_empty());
        let mut stricter = policy;
        let cli = stricter
            .crates
            .get_mut("mandate-cli")
            .context("mandate-cli has a policy")?;
        cli.forbidden_internal.push("mandate-journal".to_owned());
        let problems = layer_problems(&stricter, &meta)?;
        let named = "`mandate-cli` reaches `mandate-journal`";
        assert!(
            problems.iter().any(|p| p.starts_with(named)),
            "the layers check applies the list to the workspace as built: {problems:?}"
        );
        Ok(())
    }

    /// A workspace member named `name` with path dependencies on `deps`, each with its kind
    /// (`None` for a normal one), as `cargo metadata --no-deps` reports them.
    fn member(name: &str, deps: &[(&str, Option<&str>)]) -> Package {
        let dependencies = deps
            .iter()
            .map(|(dep, kind)| Dependency {
                name: (*dep).to_owned(),
                kind: kind.map(str::to_owned),
                path: Some(PathBuf::from(format!("/nowhere/{dep}"))),
                features: Vec::new(),
            })
            .collect();
        Package {
            id: format!("{name} 0.0.0"),
            name: name.to_owned(),
            manifest_path: PathBuf::from(format!("/nowhere/{name}/Cargo.toml")),
            dependencies,
            targets: Vec::new(),
            features: BTreeMap::new(),
        }
    }

    fn workspace(members: Vec<Package>) -> Metadata {
        let workspace_members = members.iter().map(|p| p.id.clone()).collect();
        Metadata {
            packages: members,
            workspace_members,
        }
    }

    /// A policy putting every crate in `layers` at its layer, none safety-critical, with the
    /// `forbidden_internal` lists in `forbidden` and the `planned` crates.
    fn policy(layers: &[(&str, i64)], forbidden: &[(&str, &[&str])], planned: &[&str]) -> Layers {
        let owned = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        let crates = layers
            .iter()
            .map(|(name, layer)| {
                let banned = forbidden
                    .iter()
                    .find(|(f, _)| f == name)
                    .map(|(_, b)| owned(b));
                let policy = CratePolicy {
                    layer: toml::Value::Integer(*layer),
                    safety_critical: false,
                    pure: false,
                    allowed_external: Vec::new(),
                    forbidden_internal: banned.unwrap_or_default(),
                    allowed_dependents: None,
                    dev_only: false,
                    live_feature: false,
                };
                ((*name).to_owned(), policy)
            })
            .collect();
        Layers {
            impure_crates: Vec::new(),
            planned: owned(planned),
            crates,
        }
    }

    /// The closure the guard reads follows every kind of path dependency: normal, dev and build,
    /// and through a dev-dependency of a dependency (DEC-525 item 2).
    #[test]
    fn the_closure_follows_normal_dev_and_build_path_dependencies() {
        let meta = workspace(vec![
            member(
                "a",
                &[("b", None), ("c", Some("dev")), ("d", Some("build"))],
            ),
            member("b", &[("e", Some("dev"))]),
            member("c", &[]),
            member("d", &[]),
            member("e", &[]),
        ]);
        let closure = workspace_closure(&meta);
        let reached: Vec<&str> = closure["a"].iter().map(String::as_str).collect();
        assert_eq!(reached, ["a", "b", "c", "d", "e"]);
    }

    /// A path dependency on a crate outside the workspace, which `cargo metadata --no-deps` does not
    /// walk, is refused, of any kind: it could reach a forbidden crate unseen (#697 review, M1).
    #[test]
    fn a_path_dependency_outside_the_workspace_is_a_problem() -> Result<()> {
        let layers = [("mandate-cli", 9), ("mandate-alpaca", 7)];
        let forbidden: [(&str, &[&str]); 1] = [("mandate-cli", &["mandate-alpaca"])];
        let rules = policy(&layers, &forbidden, &[]);
        let clean = workspace(vec![
            member("mandate-cli", &[]),
            member("mandate-alpaca", &[]),
        ]);
        assert_eq!(layer_problems(&rules, &clean)?, Vec::<String>::new());
        for kind in [None, Some("dev"), Some("build")] {
            let evading = workspace(vec![
                member("mandate-cli", &[("evader", kind)]),
                member("mandate-alpaca", &[]),
            ]);
            let problems = layer_problems(&rules, &evading)?;
            let named = "`mandate-cli` has a path dependency on `evader`, which is not a workspace";
            assert!(
                problems.iter().any(|p| p.starts_with(named)),
                "{kind:?}: {problems:?}"
            );
        }
        Ok(())
    }

    /// A path dependency that names a workspace member but points at another directory, a crate
    /// outside the workspace that took the member's name, is refused of any kind, and the closure
    /// does not follow it as that member (#697 review, round 2).
    #[test]
    fn a_crate_outside_the_workspace_with_a_members_name_is_a_problem() -> Result<()> {
        let layers = [
            ("mandate-cli", 9),
            ("mandate-alpaca", 7),
            ("mandate-liquidity", 1),
        ];
        let forbidden: [(&str, &[&str]); 1] = [("mandate-cli", &["mandate-alpaca"])];
        let rules = policy(&layers, &forbidden, &[]);
        for kind in [None, Some("dev"), Some("build")] {
            let mut cli = member("mandate-cli", &[("mandate-liquidity", kind)]);
            for dep in &mut cli.dependencies {
                dep.path = Some(PathBuf::from("/nowhere/tools/evader"));
            }
            let shadowed = workspace(vec![
                cli,
                member("mandate-alpaca", &[]),
                member("mandate-liquidity", &[]),
            ]);
            let problems = layer_problems(&rules, &shadowed)?;
            let named = "`mandate-cli` has a path dependency on `mandate-liquidity`, which is not a \
                         workspace member at /nowhere/tools/evader";
            assert!(
                problems.iter().any(|p| p.starts_with(named)),
                "{kind:?}: {problems:?}"
            );
            let closure = workspace_closure(&shadowed);
            let reached: Vec<&str> = closure["mandate-cli"].iter().map(String::as_str).collect();
            assert_eq!(
                reached,
                ["mandate-cli"],
                "{kind:?}: the shadow is not the member"
            );
        }
        Ok(())
    }

    /// A `forbidden_internal` name that is neither a member nor planned is a problem, so a typo
    /// cannot leave the guard empty (#697 review, m1).
    #[test]
    fn a_forbidden_name_must_be_a_member_or_planned() -> Result<()> {
        let layers = [("mandate-cli", 9), ("mandate-alpaca", 7)];
        let members = || {
            workspace(vec![
                member("mandate-cli", &[]),
                member("mandate-alpaca", &[]),
            ])
        };
        let typo: [(&str, &[&str]); 1] = [("mandate-cli", &["mandate-alpacca"])];
        let problems = layer_problems(&policy(&layers, &typo, &[]), &members())?;
        let named = "`mandate-cli` forbids `mandate-alpacca`";
        assert!(
            problems.iter().any(|p| p.starts_with(named)),
            "{problems:?}"
        );
        let planned: [(&str, &[&str]); 1] = [("mandate-cli", &["mandate-alpaca", "mandate-paper"])];
        let problems = layer_problems(&policy(&layers, &planned, &["mandate-paper"]), &members())?;
        assert_eq!(problems, Vec::<String>::new());
        Ok(())
    }

    /// A crate with `allowed_dependents` may be depended on, by a normal or a dev-dependency, only
    /// by the crates it names; one it does not name is a problem, and so is a name that is neither
    /// a member nor planned (DEC-642 item 7).
    #[test]
    fn only_an_allowed_dependent_may_depend_on_a_sealed_crate() -> Result<()> {
        let layers = [("seal", 0), ("core", 1), ("kit", 11), ("api", 6)];
        let sealed = |allowed: &[&str]| {
            let mut p = policy(&layers, &[], &["store"]);
            if let Some(seal) = p.crates.get_mut("seal") {
                seal.allowed_dependents = Some(allowed.iter().map(|n| (*n).to_owned()).collect());
            }
            p
        };
        let meta = |api_kind: Option<&'static str>| {
            workspace(vec![
                member("seal", &[]),
                member("core", &[("seal", None)]),
                member("kit", &[("seal", None), ("core", None)]),
                member("api", &[("core", None), ("seal", api_kind)]),
            ])
        };
        for kind in [None, Some("dev")] {
            let problems = layer_problems(&sealed(&["core", "kit", "store"]), &meta(kind))?;
            let named = "`api` depends on `seal`, whose allowed_dependents";
            assert!(
                problems.iter().any(|p| p.starts_with(named)),
                "{kind:?}: {problems:?}"
            );
            assert_eq!(problems.len(), 1, "{kind:?}: {problems:?}");
        }
        let allowed = layer_problems(&sealed(&["core", "kit", "api"]), &meta(None))?;
        assert_eq!(allowed, Vec::<String>::new());
        let typo = layer_problems(&sealed(&["core", "kit", "api", "stor"]), &meta(None))?;
        assert!(
            typo.iter()
                .any(|p| p.starts_with("`seal` allows `stor` as a dependent")),
            "{typo:?}"
        );
        Ok(())
    }

    /// A `dev_only` crate may be depended on only as a dev-dependency (DEC-645).
    #[test]
    fn a_dev_only_crate_is_only_a_dev_dependency() -> Result<()> {
        let layers = [("kit", 11), ("api", 6)];
        let mut p = policy(&layers, &[], &[]);
        if let Some(kit) = p.crates.get_mut("kit") {
            kit.dev_only = true;
        }
        for (kind, refused) in [(None, true), (Some("build"), true), (Some("dev"), false)] {
            let meta = workspace(vec![member("kit", &[]), member("api", &[("kit", kind)])]);
            let problems = layer_problems(&p, &meta)?;
            let named = problems
                .iter()
                .any(|p| p.starts_with("`api` depends on `kit`, which is dev-only"));
            assert_eq!(named, refused, "{kind:?}: {problems:?}");
        }
        Ok(())
    }

    fn declared_closure() -> BTreeMap<String, BTreeSet<String>> {
        [
            (
                REFCASES,
                vec![
                    REFCASES,
                    "mandate-journal",
                    "mandate-risk",
                    "mandate-spec",
                    "mandate-time",
                ],
            ),
            ("mandate-journal", vec!["mandate-journal", "mandate-time"]),
            (
                "mandate-spec",
                vec!["mandate-spec", "mandate-journal", "mandate-time"],
            ),
            ("mandate-risk", vec!["mandate-risk"]),
            ("mandate-time", vec!["mandate-time"]),
            ("mandate-marketdata", vec!["mandate-marketdata"]),
        ]
        .into_iter()
        .map(|(package, built_on)| {
            (
                package.to_owned(),
                built_on.into_iter().map(str::to_owned).collect(),
            )
        })
        .collect()
    }

    #[test]
    fn the_reference_package_runs_whenever_a_crate_it_is_built_on_is_mutated() {
        let closure = declared_closure();

        let mut journal = vec!["mandate-journal"];
        assert!(
            external_oracles(&mut journal, &closure),
            "the suites are built on the journal, so they judge its mutants whether or not the \
             change edits a suite: this is the hole #652's review found in keying on the diff"
        );
        assert_eq!(
            journal,
            ["mandate-journal", REFCASES],
            "the reference package is named so cargo-mutants builds and runs its suites"
        );

        let mut underneath = vec!["mandate-time"];
        assert!(
            external_oracles(&mut underneath, &closure),
            "a crate no suite calls directly is still reached through the ones that do, which is \
             #652 round 2's major and what cargo's graph supplies for free"
        );

        let mut unreached = vec!["mandate-marketdata"];
        assert!(
            !external_oracles(&mut unreached, &closure),
            "a package the suites are not built on cannot be judged by them"
        );
        assert_eq!(
            unreached,
            ["mandate-marketdata"],
            "and costs the run no extra test package"
        );

        let mut already = vec![REFCASES];
        assert!(
            !external_oracles(&mut already, &closure),
            "a change that mutates the reference package itself already runs it"
        );
        assert_eq!(already, [REFCASES], "and names it once");
    }

    /// Over the real graph: every crate the reference suites are built on is judged by them, with
    /// no list of crates to keep true. Rounds 2, 3, and 4 of #652's review each found another
    /// crate a suite exercised and the suite's declaration did not name — `mandate-time` through
    /// the journal's draft parser, the six crates the mandate harness interprets, `mandate-canon`
    /// through `DecStr`, and `mandate-spec` through `validated_mandate` — so the gate reads the
    /// dependency graph instead (DEC-497).
    #[test]
    fn the_reference_suites_judge_every_crate_they_are_built_on() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .context("the workspace root above xtask")?;
        let closure = workspace_closure(&metadata_in(root)?);
        for exercised in [
            "mandate-accounting",
            "mandate-approval",
            "mandate-builder",
            "mandate-canon",
            "mandate-domain",
            "mandate-executor",
            "mandate-journal",
            "mandate-num",
            "mandate-research",
            "mandate-risk",
            "mandate-runtime",
            "mandate-sim",
            "mandate-spec",
            "mandate-time",
        ] {
            let mut mutated = vec![exercised];
            assert!(
                external_oracles(&mut mutated, &closure),
                "the reference suites exercise `{exercised}`, so a mutation of it must run them"
            );
            assert!(
                mutated.contains(&REFCASES),
                "and the run must name the package that holds them: {mutated:?}"
            );
        }
        Ok(())
    }

    /// What makes the dependency closure the suites' exact reach rather than an estimate of it: a
    /// test can only exercise code its package links, and the reference suites link everything
    /// in-process. A suite that spawned a binary would reach a crate the closure does not name,
    /// and DEC-497's whole argument would be an approximation again, so the gate refuses one.
    #[test]
    fn no_reference_suite_reaches_a_crate_by_spawning_it() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .context("the workspace root above xtask")?;
        let mut spawning = Vec::new();
        for source in files_by_extension(&root.join("crates/mandate-refcases"), &["rs"])? {
            let text = fs::read_to_string(&source)?;
            if text.contains("Command::new") || text.contains("CARGO_BIN_EXE") {
                spawning.push(source.display().to_string());
            }
        }
        assert!(
            spawning.is_empty(),
            "the reference suites must reach every crate they judge through their own dependency \
             graph, which `cargo metadata` can see, and not through a subprocess, which it cannot \
             (DEC-497): {spawning:?}"
        );
        Ok(())
    }

    /// The lines of `job` in `workflow`: from its `  <job>:` key, two spaces in under `jobs:`, to the
    /// next key at that depth or the end of the file.
    fn workflow_job<'a>(workflow: &'a str, job: &str) -> Option<Vec<&'a str>> {
        let key = format!("  {job}:");
        let at_job_depth = |line: &str| {
            line.starts_with("  ") && !line.starts_with("   ") && !line.trim().starts_with('#')
        };
        let mut lines = workflow.lines().skip_while(|line| *line != key);
        let first = lines.next()?;
        Some(
            std::iter::once(first)
                .chain(lines.take_while(|line| !at_job_depth(line)))
                .collect(),
        )
    }

    /// DEC-519 item 1: a mutant in code only the Postgres tests reach is judged by them. Those tests
    /// skip without `MANDATE_PG_URL`, so a shard without the database reports every such mutant
    /// missed (#678's `lost_a_race` guard). Each shard gets the database `full-checks` has, from
    /// the same script, before the gate runs, and requires it, so a shard that lost it fails
    /// rather than skipping the tests that judge those mutants.
    #[test]
    fn every_mutation_shard_has_the_database_the_postgres_tests_need() -> Result<()> {
        let workflow = fs::read_to_string(repo_root()?.join(".github/workflows/ci.yml"))?;
        for (job, gate) in [
            ("full-checks", "run: cargo xtask ci full"),
            ("mutants", "run: cargo xtask ci mutants"),
        ] {
            let lines = workflow_job(&workflow, job)
                .with_context(|| format!("ci.yml has a `{job}` job"))?;
            let trimmed: Vec<&str> = lines
                .iter()
                .map(|line| line.trim().trim_start_matches("- "))
                .collect();
            for setting in [
                "MANDATE_PG_URL: postgres://postgres:postgres@localhost:5432/postgres",
                "MANDATE_PG_REQUIRED: \"1\"",
            ] {
                assert!(
                    trimmed.contains(&setting),
                    "`{job}` sets `{setting}`, so its Postgres tests run and cannot skip (DEC-519)"
                );
            }
            let started = trimmed
                .iter()
                .position(|line| *line == "run: .github/scripts/start-postgres.sh");
            let gated = trimmed.iter().position(|line| *line == gate);
            assert!(
                started.is_some() && gated.is_some() && started < gated,
                "`{job}` starts the shared, digest-pinned PostgreSQL before `{gate}` (DEC-519): \
                 start at {started:?}, gate at {gated:?}"
            );
        }
        Ok(())
    }

    /// DEC-519 item 2: every build the mutants job makes, the baseline's and each mutant's under
    /// `cargo mutants` and the pre-flight's test listing, is without debuginfo, whatever the caller
    /// exported, and still out of the caller's target directory. CI's matrix sets the same two
    /// settings in its environment, which is what puts them in rust-cache's key.
    #[test]
    fn the_mutants_job_builds_without_debuginfo() -> Result<()> {
        let workflow = fs::read_to_string(repo_root()?.join(".github/workflows/ci.yml"))?;
        let matrix = workflow_job(&workflow, "mutants").context("ci.yml has a `mutants` job")?;
        for setting in [
            "CARGO_PROFILE_DEV_DEBUG: \"0\"",
            "CARGO_PROFILE_TEST_DEBUG: \"0\"",
        ] {
            assert!(
                matrix.iter().any(|line| line.trim() == setting),
                "the mutants matrix sets `{setting}`, so the dependencies rust-cache restores \
                 were built the way the gate builds (DEC-519)"
            );
        }
        let cargo = mutants_job_cargo(Path::new("."), &["mutants"]);
        let envs: BTreeMap<&OsStr, Option<&OsStr>> = cargo.get_envs().collect();
        for profile in ["CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG"] {
            assert_eq!(
                envs.get(OsStr::new(profile)),
                Some(&Some(OsStr::new("0"))),
                "the mutants job sets {profile}=0 on its building children, which shortens the \
                 build phase `--build-timeout` caps (DEC-519)"
            );
        }
        assert_eq!(
            envs.get(OsStr::new(CARGO_TARGET_DIR)),
            Some(&None),
            "the caller's CARGO_TARGET_DIR is still removed (#419 review, nit 3)"
        );
        Ok(())
    }

    #[test]
    fn mutation_arguments_add_a_shard_once_and_leave_local_runs_complete() {
        let packages = ["mandate-journal", "mandate-refcases"];
        let unsharded = mutants_args("change.diff", None, &packages);
        assert!(
            !unsharded.iter().any(|arg| arg == "--shard"),
            "a local `cargo xtask check` run must cover the complete diff"
        );
        assert!(unsharded.contains(&"--test-package=mandate-refcases".to_owned()));
        assert!(
            !unsharded.iter().any(|arg| arg == "-E"),
            "the reference package runs whole, so no nextest filter narrows it"
        );
        assert_eq!(
            unsharded
                .windows(2)
                .find(|pair| pair[0] == "--timeout")
                .map(|pair| pair[1].as_str()),
            Some(MUTANT_TEST_TIMEOUT),
            "the per-mutant timeout is set here, not derived from a baseline whose packages are \
             the slice's own mutants and so need not include the reference package (DEC-498)"
        );
        assert_eq!(
            unsharded
                .windows(2)
                .find(|pair| pair[0] == "--build-timeout")
                .map(|pair| pair[1].as_str()),
            Some(MUTANT_BUILD_TIMEOUT),
            "a mutated operator in a top-level `const` can grow an array and so a build; this is \
             the third of the three phases DEC-498's shard budget caps, the other two being both \
             test phases under `--timeout`"
        );
        assert_eq!(
            unsharded
                .windows(2)
                .find(|pair| pair[0] == "--jobs")
                .map(|pair| pair[1].as_str()),
            Some("1"),
            "one mutant at a time, so a shard's critical path is its mutants in a row and the \
             timeout keeps the margin DEC-498 measured"
        );

        let sharded = mutants_args(
            "change.diff",
            Some(MutantShard {
                index: 6,
                total: 12,
            }),
            &packages,
        );
        assert_eq!(
            sharded
                .windows(2)
                .filter(|pair| pair[0] == "--shard" && pair[1] == "6/12")
                .count(),
            1
        );
        assert_eq!(
            sharded
                .windows(2)
                .filter(|pair| pair[0] == "--sharding" && pair[1] == "slice")
                .count(),
            1
        );
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

    /// The single-file check as it was before the map became a directory, as an oracle: every
    /// crate named as `` `crate` ``, every fixture by its path, every backticked repository path
    /// existing, all over one text.
    fn single_file_feature_map_problems(
        root: &Path,
        text: &str,
        meta: &Metadata,
    ) -> Result<BTreeSet<String>> {
        let mut problems = BTreeSet::new();
        for pkg in workspace_packages(meta) {
            if !text.contains(&format!("`{}`", pkg.name)) {
                problems.insert(format!("crate `{}` has no entry", pkg.name));
            }
        }
        for entry in fs::read_dir(root.join("fixtures/refcases"))? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if !text.contains(&format!("fixtures/refcases/{name}")) {
                problems.insert(format!("fixtures/refcases/{name} has no entry"));
            }
        }
        for path in backticked_paths(text) {
            if !root.join(path).exists() {
                problems.insert(format!("`{path}`, which does not exist"));
            }
        }
        Ok(problems)
    }

    /// The directory's problems in the oracle's terms: a missing path is named with the file it
    /// is in, which the single file had no need to say, so that prefix is dropped to compare.
    fn without_file_names(problems: Vec<String>) -> BTreeSet<String> {
        problems
            .into_iter()
            .map(|problem| match problem.split_once(" names `") {
                Some((_, rest)) if !problem.starts_with("crate ") => format!("`{rest}"),
                _ => problem,
            })
            .collect()
    }

    /// One way a feature's text drifts from the workspace.
    type Drift = fn(&str) -> String;

    /// The feature map as a directory catches exactly the drift the single `feature-map.md` did,
    /// in a fixture workspace: a crate named nowhere, a reference-case suite named nowhere, and a
    /// path that does not exist, each with the map in one feature file and split across three, and
    /// nothing when the map is complete. A feature file without its `# ` title is refused too.
    #[test]
    fn the_feature_map_directory_catches_the_drift_the_single_file_did() -> Result<()> {
        let dir = env::temp_dir().join(format!("mandate-xtask-feature-map-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        let fx = Fixture(dir);
        fx.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/alpha\", \"crates/beta\"]\nresolver = \"3\"\n",
        )?;
        for name in ["alpha", "beta"] {
            fx.write(
                &format!("crates/{name}/Cargo.toml"),
                &format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n"),
            )?;
            fx.write(&format!("crates/{name}/src/lib.rs"), "")?;
        }
        fx.write("fixtures/refcases/one.json", "{}")?;
        fs::copy(
            repo_root()?.join("rust-toolchain.toml"),
            fx.0.join("rust-toolchain.toml"),
        )?;
        output_in(&fx.0, "cargo", &["generate-lockfile", "--offline"])?;
        let meta = metadata_in(&fx.0)?;

        let sections = [
            "# Alpha\n\n- **Code:** `alpha`: `crates/alpha/src/lib.rs`.\n",
            "# Beta\n\n- **Code:** `beta`: `crates/beta/src/lib.rs`.\n",
            "# Cases\n\n- **Reference cases:** `fixtures/refcases/one.json`.\n",
        ];
        let drifts: [(&str, Drift); 4] = [
            ("complete", |s| s.to_owned()),
            ("a crate named nowhere", |s| s.replace("`beta`", "beta")),
            ("a suite named nowhere", |s| {
                s.replace("`fixtures/refcases/one.json`", "the suite")
            }),
            ("a path that does not exist", |s| {
                s.replace("crates/alpha/src/lib.rs", "crates/alpha/src/gone.rs")
            }),
        ];
        let features = fx.0.join(FEATURE_MAP);
        for (drift, apply) in drifts {
            let drifted: Vec<String> = sections.iter().map(|s| apply(s)).collect();
            let expected = single_file_feature_map_problems(&fx.0, &drifted.join("\n"), &meta)?;
            assert_eq!(
                expected.is_empty(),
                drift == "complete",
                "the oracle sees {drift}: {expected:?}"
            );
            for split in [false, true] {
                if features.exists() {
                    fs::remove_dir_all(&features)?;
                }
                fx.write(&format!("{FEATURE_MAP}/README.md"), "# Feature map\n")?;
                if split {
                    for (k, text) in drifted.iter().enumerate() {
                        fx.write(&format!("{FEATURE_MAP}/f{k}.md"), text)?;
                    }
                } else {
                    let joined = drifted
                        .iter()
                        .map(|s| s.replacen("# ", "## ", 1))
                        .collect::<Vec<_>>()
                        .join("\n");
                    fx.write(
                        &format!("{FEATURE_MAP}/all.md"),
                        &format!("# All\n\n{joined}"),
                    )?;
                }
                assert_eq!(
                    without_file_names(feature_map_problems(&fx.0, &meta)?),
                    expected,
                    "{drift}, the map {}",
                    if split {
                        "split across files"
                    } else {
                        "in one file"
                    }
                );
            }
        }

        fx.write(&format!("{FEATURE_MAP}/untitled.md"), "no title\n")?;
        let untitled = feature_map_problems(&fx.0, &meta)?;
        assert!(
            untitled.contains(&format!(
                "{FEATURE_MAP}/untitled.md does not open with a `# ` title"
            )),
            "a feature file without its title is refused: {untitled:?}"
        );
        fs::remove_dir_all(&features)?;
        fx.write(&format!("{FEATURE_MAP}/README.md"), "# Feature map\n")?;
        assert!(
            feature_map_problems(&fx.0, &meta)?
                .iter()
                .any(|p| p.ends_with("has no feature files")),
            "and a map of nothing but its README is refused"
        );
        fs::remove_dir_all(&fx.0).ok();
        Ok(())
    }

    /// The repository's own map passes, and every feature has a title for the index.
    #[test]
    fn the_repository_feature_map_is_complete() -> Result<()> {
        let root = repo_root()?;
        assert_eq!(
            feature_map_problems(&root, &metadata_in(&root)?)?,
            Vec::<String>::new()
        );
        assert!(feature_files(&root)?.len() > 1);
        Ok(())
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
    fn a_pending_test_a_macro_generates_is_rejected() {
        let src = concat!(
            "macro_rules! case {\n",
            "    ($name:ident, $case:expr) => {\n",
            "        #[test]\n",
            "        #[ignore = \"pending E6-4\"]\n",
            "        fn $name() { run($case); }\n",
            "    };\n",
            "}\n",
            "case!(family_r_01, \"R-01\");\n",
            "#[test]\n",
            "#[ignore = \"pending E6-4\"]\n",
            "fn written_out() { run(\"R-02\"); }\n",
            "#[test]\n",
            "#[ignore = \"pending E6-4\"]\n",
            "fn $generated() {}\n",
            "#[test]\n",
            "#[ignore = \"slow\"]\n",
            "fn not_pending() {}\n",
            "forward!(#[ignore = \"pending E6-4\"] passed_to_a_macro_call);\n",
            "#[ignore = \"pending E6-4\"]\n",
            "struct NotAFunction;\n",
            "proptest! {\n",
            "    #[test]\n",
            "    #[ignore = \"pending E6-4\"]\n",
            "    fn inside_a_macro_call(x in 0..10u8) { prop_assert!(x < 10); }\n",
            "}\n",
        );
        assert_eq!(
            generated_pending_markers(src),
            [
                (4, "inside a `macro_rules!` body"),
                (13, "on a `$`-named function"),
                (18, "on no named function of its own"),
                (19, "on no named function of its own"),
            ],
            "a marker the source scan cannot turn into a test name is a problem; a plain function \
             inside a macro call (`proptest!`) is not"
        );
        assert_eq!(
            found(src),
            expected(&[
                ("written_out", "E6-4", 11),
                ("inside_a_macro_call", "E6-4", 24)
            ]),
            "the scan sees the plain functions and neither macro-written test, which is why the \
             rule exists"
        );
        assert!(
            generated_pending_markers(&fs::read_to_string("xtask/src/main.rs").unwrap_or_default())
                .is_empty()
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
            "{\"type\":\"test\",\"event\":\"failed\",\"name\":\"a::fs$fails\",\"stdout\":\"panicked at x.rs:1:1:\\nUnimplemented\"}\n",
            "{\"type\":\"test\",\"event\":\"failed\",\"name\":\"a::fs$inner::passes\"}\n",
            "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"a$tests::unit\"}\n",
            "{\"type\":\"test\",\"event\":\"failed\",\"name\":\"a::fs$away\",\"stdout\":\"running 1 test\\nthread 'away' panicked at x.rs:9:1:\\nfixture file missing\\nstack backtrace:\"}\n",
            "{\"type\":\"test\",\"event\":\"failed\",\"name\":\"a::fs$own_error\",\"stdout\":\"panicked at x.rs:9:1:\\ncalled `Result::unwrap()` on an `Err` value: Overflow\"}\n",
            "{\"type\":\"test\",\"event\":\"ignored\",\"name\":\"a::fs$skipped\"}\n",
            "{\"type\":\"suite\",\"event\":\"failed\",\"passed\":2,\"failed\":2}\n",
            "not a JSON line\n",
        );
        let outcomes = test_outcomes(stdout);
        let outcome = |binary_id: &str, name: &str, passed, output: &str| TestOutcome {
            binary_id: binary_id.to_owned(),
            name: name.to_owned(),
            passed,
            output: output.to_owned(),
        };
        assert_eq!(
            outcomes,
            [
                outcome("a::fs", "passes", true, ""),
                outcome(
                    "a::fs",
                    "fails",
                    false,
                    "panicked at x.rs:1:1:\nUnimplemented"
                ),
                outcome("a::fs", "inner::passes", false, ""),
                outcome("a", "tests::unit", true, ""),
                outcome(
                    "a::fs",
                    "away",
                    false,
                    "running 1 test\nthread 'away' panicked at x.rs:9:1:\nfixture file \
                     missing\nstack backtrace:"
                ),
                outcome(
                    "a::fs",
                    "own_error",
                    false,
                    "panicked at x.rs:9:1:\ncalled `Result::unwrap()` on an `Err` value: Overflow"
                ),
            ],
            "an ignored test is a test the run skipped, not one that failed"
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
            new(7, Some("a::fs"), "away"),
            new(8, Some("a::fs"), "own_error"),
            new(9, Some("a::fs"), "skipped"),
        ];
        let verdicts = verdicts(&tests, &outcomes, &[]);
        let problems: Vec<String> = verdicts
            .iter()
            .map(|p| p.split([',', ';']).next().unwrap_or_default().to_owned())
            .collect();
        assert!(
            verdicts.iter().any(|p| p.contains(
                "It panicked with: thread 'away' panicked at x.rs:9:1: fixture file missing"
            )),
            "{verdicts:?}"
        );
        assert_eq!(
            problems,
            [
                "f.rs:1: `passes` (pending E1-1) passes on this change's code",
                "f.rs:3: `inner::passes` (pending E1-1) fails away from its stub",
                "f.rs:4: `unit` (pending E1-1) passes on this change's code",
                "f.rs:5: `missing` (pending E1-1) did not run",
                "f.rs:6: `fails` (pending E1-1) did not run",
                "f.rs:7: `away` (pending E1-1) fails away from its stub",
                "f.rs:8: `own_error` (pending E1-1) fails away from its stub",
                "f.rs:9: `skipped` (pending E1-1) did not run",
            ],
            "`fails` stops at an `Unimplemented` and is no problem; an `Overflow` names no stub, \
             so the crate whose stubs returned it was fixed instead of excused"
        );
    }

    #[test]
    fn a_behaviour_only_row_is_used_only_where_the_stub_check_fails() {
        let file = "crates/a/tests/hand.rs";
        let name = "a_listed_test";
        let rows = [BehaviourOnlyRow {
            file: file.to_owned(),
            test: name.to_owned(),
            reason: "fails on the partial answer".to_owned(),
        }];
        let run = |path: &str| PendingTestRun {
            file: file.to_owned(),
            package: "a".to_owned(),
            binary: Some("a::hand".to_owned()),
            test: PendingTest {
                path: path.to_owned(),
                story: "E1-1".to_owned(),
                line: 1,
            },
        };
        let failed = |path: &str, output: &str| TestOutcome {
            binary_id: "a::hand".to_owned(),
            name: path.to_owned(),
            passed: false,
            output: output.to_owned(),
        };
        let at_the_stub = "panicked at x.rs:1:1:\nUnimplemented { story: \"E1-1\" }";
        let on_the_answer = "panicked at x.rs:1:1:\nno cancel where the sequence belongs";

        let unneeded = verdicts(&[run(name)], &[failed(name, at_the_stub)], &rows);
        assert_eq!(
            unneeded.len(),
            1,
            "a listed test that fails at its stub is judged by the stub check, and its row is \
             reported as not needed, so the list cannot grow silently: {unneeded:?}"
        );
        assert!(
            unneeded
                .iter()
                .all(|p| p.contains("row is not needed") && p.contains(name)),
            "{unneeded:?}"
        );

        assert!(
            verdicts(&[run(name)], &[failed(name, on_the_answer)], &rows).is_empty(),
            "a listed test that fails on the partial answer is what the row is for"
        );
        let unlisted = "not_a_row_of_the_list";
        let away = verdicts(&[run(unlisted)], &[failed(unlisted, on_the_answer)], &rows);
        assert!(
            away.len() == 1 && away.iter().all(|p| p.contains("fails away from its stub")),
            "and an unlisted one failing the same way is still a problem: {away:?}"
        );
        assert!(
            verdicts(&[run(unlisted)], &[failed(unlisted, at_the_stub)], &rows).is_empty(),
            "while an unlisted one at its stub is what every pending test must do"
        );
    }

    /// The rows the `BEHAVIOUR_ONLY_TESTS` array held when it moved to one file a row. Each is
    /// still loaded unless its test is no longer pending in its file, the one way a row may go;
    /// at the migration every one was pending, so the loaded set was exactly this one.
    const ROWS_BEFORE_THE_DIRECTORY: [(&str, &str); 21] = [
        (
            "crates/mandate-executor/tests/hand.rs",
            "a_crypto_position_carries_one_stop_limit_for_the_whole_position",
        ),
        (
            "crates/mandate-executor/tests/hand.rs",
            "a_crypto_stop_limit_is_re_placed_for_the_new_net_quantity",
        ),
        ("crates/mandate-shell/tests/tracer.rs", "outlier_close"),
        (
            "crates/mandate-executor/tests/properties.rs",
            "every_unprotected_interval_has_a_journaled_start_and_end",
        ),
        (
            "crates/mandate-executor/tests/properties.rs",
            "protective_sell_quantity_never_exceeds_the_position_in_any_script",
        ),
        (
            "crates/mandate-executor/tests/properties.rs",
            "no_resting_order_is_submitted_inside_an_unprotected_interval",
        ),
        (
            "crates/mandate-executor/tests/hand.rs",
            "an_owner_exit_outside_the_session_prices_from_the_confirmed_bid",
        ),
        (
            "crates/mandate-executor/tests/properties.rs",
            "no_order_is_submitted_while_an_unconfirmed_cancel_is_outstanding",
        ),
        (
            "crates/mandate-executor/tests/properties.rs",
            "no_interval_exceeds_the_limit_without_an_alert",
        ),
        (
            "crates/mandate-executor/tests/hand.rs",
            "a_new_brackets_start_ends_the_awaited_interval_not_the_first_brackets",
        ),
        (
            "crates/mandate-runtime/tests/answer_records.rs",
            "every_answer_record_the_runtime_writes_passes_the_journals_check",
        ),
        (
            "crates/mandate-runtime/tests/answer_records.rs",
            "a_responded_record_carries_its_quorum_only_where_check_7_was_judged",
        ),
        (
            "crates/mandate-runtime/tests/answer_records.rs",
            "decided_by_now_is_null_unless_the_reclassification_asks",
        ),
        (
            "crates/mandate-journal/tests/policy_overlay.rs",
            "every_policy_overlay_valid_draft_parses",
        ),
        (
            "crates/mandate-journal/tests/policy_overlay.rs",
            "every_policy_overlay_invalid_draft_is_refused_with_its_reason_at_its_path",
        ),
        (
            "crates/mandate-executor/tests/hand.rs",
            "an_exits_re_placement_leaves_a_held_brackets_shares_to_its_legs",
        ),
        (
            "crates/mandate-executor/tests/hand.rs",
            "a_re_placement_before_expiry_leaves_a_held_brackets_shares_to_its_legs",
        ),
        (
            "crates/mandate-executor/tests/hand.rs",
            "a_passive_exits_rest_leaves_a_held_brackets_shares_to_its_legs",
        ),
        (
            "crates/mandate-executor/tests/hand.rs",
            "a_re_cover_leaves_a_held_brackets_shares_to_its_legs",
        ),
        (
            "xtask/src/main.rs",
            "tests::ci_files_reads_every_file_that_decides_a_build",
        ),
        (
            "xtask/src/main.rs",
            "tests::the_lint_job_runs_the_live_feature_check_on_its_repository",
        ),
    ];

    #[test]
    fn every_row_of_the_old_array_is_loaded_until_its_test_stops_being_pending() -> Result<()> {
        let root = repo_root()?;
        let rows = behaviour_only_rows(&root)?;
        for (file, test) in ROWS_BEFORE_THE_DIRECTORY {
            let pending = fs::read_to_string(root.join(file))
                .map(|text| pending_tests(&text).iter().any(|t| t.path == test))
                .unwrap_or(false);
            let loaded = rows.iter().any(|row| row.names(file, test));
            assert!(
                loaded || !pending,
                "`{test}` in {file} is still pending, so its row must still be in \
                 {BEHAVIOUR_ONLY_DIR}"
            );
        }
        let mut sorted = rows.clone();
        sorted.sort();
        assert_eq!(rows, sorted, "the rows load in file and test order");
        Ok(())
    }

    /// A row file the gate cannot trust is refused, naming the file: anything but the README and
    /// `.toml` rows, an unknown, missing, or empty field, and a name that is not the row's own.
    #[test]
    fn a_malformed_or_misnamed_row_is_refused() -> Result<()> {
        let fx = Fixture(env::temp_dir().join(format!(
            "mandate-xtask-behaviour-rows-{}",
            std::process::id()
        )));
        if fx.0.exists() {
            fs::remove_dir_all(&fx.0)?;
        }
        assert!(
            behaviour_only_rows(&fx.0)?.is_empty(),
            "no directory, no rows: a row only excuses, so none is the stricter gate"
        );
        let row = |test: &str| {
            format!(
                "file = \"crates/a/tests/hand.rs\"\ntest = \"{test}\"\nreason = \"\"\"\nwhy\n\"\"\"\n"
            )
        };
        let dir = BEHAVIOUR_ONLY_DIR;
        fx.write(&format!("{dir}/README.md"), "# rows\n")?;
        fx.write(&format!("{dir}/a__hand__listed.toml"), &row("listed"))?;
        fx.write(
            &format!("{dir}/a__hand__inner__nested.toml"),
            &row("inner::nested"),
        )?;
        let rows = behaviour_only_rows(&fx.0)?;
        assert_eq!(
            rows.iter().map(|r| r.test.as_str()).collect::<Vec<_>>(),
            ["inner::nested", "listed"],
            "two well-formed rows load, sorted"
        );
        fx.write(
            &format!("{dir}/xtask__main__tests__own.toml"),
            "file = \"xtask/src/main.rs\"\ntest = \"tests::own\"\nreason = \"r\"\n",
        )?;
        let rows = behaviour_only_rows(&fx.0)?;
        assert!(
            rows.iter()
                .any(|r| r.names("xtask/src/main.rs", "tests::own")),
            "a row of xtask's own unit tests loads"
        );
        fs::remove_file(fx.0.join(format!("{dir}/xtask__main__tests__own.toml")))?;

        let bad = [
            (
                "a__hand__other.toml",
                row("listed"),
                "must be named a__hand__listed.toml",
            ),
            ("notes.txt", "x".to_owned(), "is not a row"),
            (
                "a__hand__extra.toml",
                row("extra") + "story = \"E1-1\"\n",
                "is not a row of `file`, `test` and `reason`",
            ),
            (
                "a__hand__short.toml",
                "file = \"crates/a/tests/hand.rs\"\ntest = \"short\"\n".to_owned(),
                "is not a row of `file`, `test` and `reason`",
            ),
            (
                "a__hand__empty.toml",
                "file = \"crates/a/tests/hand.rs\"\ntest = \"empty\"\nreason = \" \"\n".to_owned(),
                "has an empty field",
            ),
            (
                "outside.toml",
                "file = \"tests/x.rs\"\ntest = \"t\"\nreason = \"r\"\n".to_owned(),
                "not a file under `crates/<crate>/`",
            ),
            (
                "web__x__t.toml",
                "file = \"web/x.ts\"\ntest = \"t\"\nreason = \"r\"\n".to_owned(),
                "not a file under `crates/<crate>/`",
            ),
            (
                "xtask__other__tests__t.toml",
                "file = \"xtask/src/other.rs\"\ntest = \"tests::t\"\nreason = \"r\"\n".to_owned(),
                "not a file under `crates/<crate>/`",
            ),
            (
                "xtask__main__t.toml",
                "file = \"xtask/src/main.rs\"\ntest = \"t\"\nreason = \"r\"\n".to_owned(),
                "not a test in xtask's `tests` module",
            ),
        ];
        for (name, text, problem) in bad {
            let path = format!("{dir}/{name}");
            fx.write(&path, &text)?;
            let refused = behaviour_only_rows(&fx.0).expect_err("the row is refused");
            assert_eq!(
                format!("{refused:#}"),
                "behaviour-only: 1 problem(s)",
                "{name}"
            );
            fs::remove_file(fx.0.join(&path))?;
            assert!(
                behaviour_only_rows(&fx.0).is_ok(),
                "and only {name} was the problem ({problem})"
            );
        }
        fs::remove_dir_all(&fx.0).ok();
        Ok(())
    }

    /// The gate end to end, in a fixture repository: a row excuses its test's failure on
    /// behaviour, and once the test is no longer pending the row is named for deletion.
    #[test]
    fn a_row_excuses_its_test_and_expires_with_its_marker() -> Result<()> {
        let fx = Fixture::new("pending-rows")?;
        let away = concat!(
            "#[test]\n#[ignore = \"pending E1-1\"]\n",
            "fn fails_on_behaviour() { assert_eq!(fx::lookup().ok(), Some(42)); }\n",
        );
        fx.write("crates/fx/tests/stubs.rs", away)?;
        fx.commit()?;
        let problems = pending_problems(&fx.0)?;
        assert!(
            problems.len() == 1
                && problems
                    .iter()
                    .all(|p| p.contains("fails away from its stub")),
            "unlisted, the behaviour failure is a problem: {problems:?}"
        );

        fx.write(
            &format!("{BEHAVIOUR_ONLY_DIR}/fx__stubs__fails_on_behaviour.toml"),
            concat!(
                "file = \"crates/fx/tests/stubs.rs\"\n",
                "test = \"fails_on_behaviour\"\n",
                "reason = \"the fixture's partial answer\"\n",
            ),
        )?;
        fx.commit()?;
        assert_eq!(
            pending_problems(&fx.0)?,
            Vec::<String>::new(),
            "with its row, it is excused"
        );

        fx.write(
            "crates/fx/tests/stubs.rs",
            &away.replace("fails_on_behaviour", "renamed"),
        )?;
        fx.commit()?;
        let problems = pending_problems(&fx.0)?;
        assert!(
            problems.iter().any(|p| p.contains(
                "xtask/behaviour-only/fx__stubs__fails_on_behaviour.toml names \
                 `fails_on_behaviour` in crates/fx/tests/stubs.rs, which is no longer a pending \
                 test there"
            )),
            "the row is named for deletion once its test is not pending: {problems:?}"
        );
        Ok(())
    }

    #[test]
    fn the_panic_a_failure_starts_with_is_quoted_back() {
        let panic = concat!(
            "running 1 test\n",
            "\n",
            "thread 'a_case' (1234) panicked at crates/fx/tests/hand.rs:12:9:\n",
            "fixture file missing\n",
            "stack backtrace:\n",
            "   0: __rustc::rust_begin_unwind\n",
        );
        assert_eq!(
            first_panic_line(panic),
            "thread 'a_case' (1234) panicked at crates/fx/tests/hand.rs:12:9: fixture file missing"
        );
        assert_eq!(first_panic_line(""), "no output");
        assert_eq!(
            first_panic_line("no panic here\nsecond line"),
            "no panic here second line"
        );
        assert!(first_panic_line(&"x ".repeat(400)).ends_with("..."));
    }

    #[test]
    fn only_an_unimplemented_body_is_a_stub() {
        let src = concat!(
            "pub fn evaluate(input: &Input) -> Result<Decision, GateError> {\n",
            "    let _ = input;\n",
            "    Err(GateError::Unimplemented(\"evaluate\", \"E6-3\"))\n",
            "}\n",
            "pub fn refuse(n: u32) -> Result<u32, SpecError> {\n",
            "    let _ = n;\n",
            "    Err(SpecError::ClockWentBackwards)\n",
            "}\n",
            "pub fn read_artifact(&self, id: &Id) -> Result<Vec<u8>, ArtifactError> {\n",
            "    let _ = id;\n",
            "    Err(ArtifactError::Missing)\n",
            "}\n",
            "pub fn answer() -> u32 { todo!() }\n",
            "pub fn rejected() -> Result<(), Rejected> {\n",
            "    Err(Rejected::NotEvaluated(SpecError::Unimplemented))\n",
            "}\n",
            "pub fn real(n: u32) -> u32 {\n",
            "    n + 1\n",
            "}\n",
        );
        let stub = |from, to| is_stub_function(src, from, to);
        assert!(stub(1, 4), "an Unimplemented variant is a stub");
        assert!(stub(13, 13), "so is a todo!()");
        assert!(stub(14, 16), "so is a wrapped Unimplemented");
        assert!(
            !stub(5, 8),
            "a body that always returns another error is not a stub (DEC-137)"
        );
        assert!(
            !stub(9, 12),
            "nor is `NoArtifacts::read_artifact`, which is live code"
        );
        assert!(!stub(17, 19), "nor is an implemented function");
    }

    /// A git repository in a temporary directory holding a one-crate workspace laid out like this
    /// one, removed on drop.
    struct Fixture(PathBuf);

    const STUBS: &str = concat!(
        "#[derive(Debug, PartialEq)]\n",
        "pub enum Missing {\n",
        "    Unimplemented,\n",
        "}\n",
        "pub fn answer() -> u32 { todo!() }\n",
        "pub fn lookup() -> Result<u32, Missing> { Err(Missing::Unimplemented) }\n",
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

        /// A repository the whole mutation gate can run in: a two-crate workspace where `probe`'s
        /// only test is pending and `covered`'s runs, its own `xtask/layers.toml` making both
        /// safety-critical product crates, and an `origin/main` at the base commit so
        /// `base_ref_in` reaches its merge-base path (DEC-139).
        fn gated(name: &str) -> Result<Self> {
            let dir = env::temp_dir().join(format!("mandate-xtask-{name}-{}", std::process::id()));
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
            }
            let fixture = Fixture(dir);
            fixture.write(
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/probe\", \"crates/covered\"]\nresolver = \"3\"\n",
            )?;
            fixture.write(
                "xtask/layers.toml",
                concat!(
                    "impure_crates = []\n",
                    "[crates.probe]\nlayer = 0\nsafety_critical = true\npure = true\n",
                    "[crates.covered]\nlayer = 0\nsafety_critical = true\npure = true\n",
                ),
            )?;
            for krate in ["probe", "covered"] {
                fixture.write(
                    &format!("crates/{krate}/Cargo.toml"),
                    &format!(
                        "[package]\nname = \"{krate}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n"
                    ),
                )?;
                fixture.write(&format!("crates/{krate}/src/lib.rs"), "")?;
            }
            fixture.write(
                "crates/covered/tests/covered.rs",
                concat!(
                    "#[test]\n",
                    "fn the_flag_is_negated() {\n",
                    "    assert!(covered::negate(false));\n",
                    "    assert!(!covered::negate(true));\n",
                    "}\n",
                ),
            )?;
            fixture.write(".gitignore", "/target\n/base\n")?;
            fs::copy(
                repo_root()?.join("rust-toolchain.toml"),
                fixture.0.join("rust-toolchain.toml"),
            )?;
            output_in(&fixture.0, "cargo", &["generate-lockfile", "--offline"])?;
            output_in(&fixture.0, "git", &["init", "-q"])?;
            fixture.commit()?;
            let base = output_in(&fixture.0, "git", &["rev-parse", "HEAD"])?;
            output_in(
                &fixture.0,
                "git",
                &["update-ref", "refs/remotes/origin/main", base.trim()],
            )?;
            fs::write(fixture.0.join("base"), base.trim())?;
            fixture.write(
                "crates/probe/src/lib.rs",
                concat!(
                    "#[must_use]\n",
                    "pub fn code(refused: bool) -> &'static str {\n",
                    "    if refused { \"refused\" } else { \"accepted\" }\n",
                    "}\n",
                ),
            )?;
            fixture.write(
                "crates/probe/tests/probe.rs",
                concat!(
                    "#[test]\n",
                    "#[ignore = \"pending E1-1\"]\n",
                    "fn the_code_names_the_refusal() {\n",
                    "    assert_eq!(probe::code(true), \"refused\");\n",
                    "}\n",
                ),
            )?;
            fixture.write(
                "crates/covered/src/lib.rs",
                "#[must_use]\npub fn negate(flag: bool) -> bool {\n    !flag\n}\n",
            )?;
            fixture.commit()?;
            Ok(fixture)
        }

        fn write(&self, path: &str, text: &str) -> Result<()> {
            let file = self.0.join(path);
            fs::create_dir_all(file.parent().context("fixture file without a directory")?)?;
            fs::write(file, text)?;
            Ok(())
        }

        fn commit(&self) -> Result<()> {
            self.commit_with("fixture")
        }

        fn commit_with(&self, message: &str) -> Result<()> {
            output_in(&self.0, "git", &["add", "-A"])?;
            self.git(&["commit", "-q", "-m", message])?;
            Ok(())
        }

        /// `git` in the fixture as a committer with no hooks or signing, trimmed.
        fn git(&self, args: &[&str]) -> Result<String> {
            let mut all = vec![
                "-c",
                "user.name=xtask",
                "-c",
                "user.email=xtask@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ];
            all.extend_from_slice(args);
            Ok(output_in(&self.0, "git", &all)?.trim().to_owned())
        }

        /// A pull request merged onto a main that moved after the PR branched, as a `pull_request`
        /// run checks it out. `old_base` (the PR's `base.sha`) holds a spec and a code file; the PR
        /// branch `pr` changes `pr_files`; main then gains a spec change citing DEC-1 at
        /// `main_tip`; HEAD is the merge of `pr` into `main_tip`. `origin/main` still names
        /// `old_base`, as in a checkout whose remote refs predate main's change, so only the merge
        /// commit's first parent gives `main_tip`.
        fn moved_base(name: &str, pr_files: &[&str]) -> Result<MovedBase> {
            let dir = env::temp_dir().join(format!("mandate-xtask-{name}-{}", std::process::id()));
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
            }
            let fx = Fixture(dir);
            fx.write("docs/specs/s.md", "v1\n")?;
            fx.write("crates/c/src/lib.rs", "")?;
            output_in(&fx.0, "git", &["init", "-q", "-b", "main"])?;
            fx.commit_with("base")?;
            let old_base = fx.git(&["rev-parse", "HEAD"])?;
            fx.git(&["switch", "-q", "-c", "pr"])?;
            for file in pr_files {
                fx.write(file, "changed by the PR\n")?;
            }
            fx.commit_with("the PR's change")?;
            fx.git(&["switch", "-q", "main"])?;
            fx.write("docs/specs/s.md", "v2\n")?;
            fx.commit_with("Spec: DEC-1 changes s")?;
            let main_tip = fx.git(&["rev-parse", "HEAD"])?;
            fx.git(&["update-ref", "refs/remotes/origin/main", &old_base])?;
            fx.git(&["merge", "--no-ff", "-q", "-m", "Merge pr into main", "pr"])?;
            Ok(MovedBase {
                fx,
                old_base,
                main_tip,
            })
        }
    }

    struct MovedBase {
        fx: Fixture,
        old_base: String,
        main_tip: String,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    /// A `cargo mutants --in-diff` run over a crate with one stub and one implemented function,
    /// in the shape `cargo-mutants` 27.1.0 writes to `target/mutants.out/outcomes.json`.
    fn outcomes(stub: &str, real: &str) -> String {
        format!(
            concat!(
                "{{\"outcomes\": [\n",
                "{{\"scenario\": \"Baseline\", \"summary\": \"Success\"}},\n",
                "{{\"scenario\": {{\"Mutant\": {{\"name\": \"src/lib.rs:7:5: replace stubbed with Ok(0)\", ",
                "\"package\": \"fx\", \"file\": \"src/lib.rs\", ",
                "\"function\": {{\"function_name\": \"stubbed\", \"return_type\": \"-> Result<u32, Error>\", ",
                "\"span\": {{\"start\": {{\"line\": 6, \"column\": 1}}, \"end\": {{\"line\": 9, \"column\": 2}}}}}}, ",
                "\"span\": {{\"start\": {{\"line\": 7, \"column\": 5}}, \"end\": {{\"line\": 8, \"column\": 28}}}}, ",
                "\"replacement\": \"Ok(0)\", \"genre\": \"FnValue\"}}}}, \"summary\": \"{stub}\"}},\n",
                "{{\"scenario\": {{\"Mutant\": {{\"name\": \"src/lib.rs:12:5: replace real -> u32 with 0\", ",
                "\"package\": \"fx\", \"file\": \"src/lib.rs\", ",
                "\"function\": {{\"function_name\": \"real\", \"return_type\": \"-> u32\", ",
                "\"span\": {{\"start\": {{\"line\": 11, \"column\": 1}}, \"end\": {{\"line\": 13, \"column\": 2}}}}}}, ",
                "\"span\": {{\"start\": {{\"line\": 12, \"column\": 5}}, \"end\": {{\"line\": 12, \"column\": 10}}}}, ",
                "\"replacement\": \"0\", \"genre\": \"FnValue\"}}}}, \"summary\": \"{real}\"}}\n",
                "]}}\n"
            ),
            stub = stub,
            real = real
        )
    }

    #[test]
    fn a_surviving_mutant_is_skipped_only_in_a_stub_of_a_crate_whose_tests_are_pending()
    -> Result<()> {
        let fx = Fixture::new("mutants")?;
        fx.write(
            "crates/fx/src/lib.rs",
            concat!(
                "#[derive(Debug, PartialEq)]\n",
                "pub enum Error {\n",
                "    Unimplemented,\n",
                "}\n",
                "\n",
                "pub fn stubbed(n: u32) -> Result<u32, Error> {\n",
                "    let _ = n;\n",
                "    Err(Error::Unimplemented)\n",
                "}\n",
                "\n",
                "pub fn real(n: u32) -> u32 {\n",
                "    n + 1\n",
                "}\n",
            ),
        )?;
        let pending = |pending| {
            vec![MutatedCrate {
                package: "fx".to_owned(),
                dir: "crates/fx".to_owned(),
                pending,
            }]
        };
        let verdicts =
            |crates: &[MutatedCrate], json: &str| -> Result<(Vec<String>, Vec<String>)> {
                mutant_verdicts(&fx.0, crates, json)
            };

        let (problems, skipped) =
            verdicts(&pending(true), &outcomes("MissedMutant", "MissedMutant"))?;
        assert_eq!(
            skipped,
            ["src/lib.rs:7:5: replace stubbed with Ok(0)"],
            "a mutant of a stub body no live test can reach is skipped by name"
        );
        assert_eq!(
            problems,
            [
                "src/lib.rs:12:5: replace real -> u32 with 0: MissedMutant, and the function it \
                 mutates is not a stub"
            ],
            "a mutant surviving in implemented code fails the gate, pending tests or not"
        );

        let (problems, skipped) =
            verdicts(&pending(false), &outcomes("MissedMutant", "CaughtMutant"))?;
        assert!(skipped.is_empty());
        assert_eq!(
            problems.len(),
            1,
            "without pending tests nothing is exempt, which is the rule DEC-83 already had"
        );

        let (problems, skipped) =
            verdicts(&pending(true), &outcomes("MissedMutant", "CaughtMutant"))?;
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(skipped.len(), 1);

        let (problems, _) = verdicts(&pending(true), &outcomes("Timeout", "CaughtMutant"))?;
        assert_eq!(
            problems.len(),
            1,
            "a timeout is not a missed mutant, so no stub exempts it"
        );

        let baseline = outcomes("CaughtMutant", "CaughtMutant").replace(
            "\"scenario\": \"Baseline\", \"summary\": \"Success\"",
            "\"scenario\": \"Baseline\", \"summary\": \"Failure\"",
        );
        let (problems, _) = verdicts(&pending(true), &baseline)?;
        assert_eq!(problems, ["the unmutated baseline reports Failure"]);
        Ok(())
    }

    #[test]
    fn only_a_run_that_missed_mutants_may_exempt_a_stub() -> Result<()> {
        let fx = Fixture::new("mutants-status")?;
        let crates = [MutatedCrate {
            package: "fx".to_owned(),
            dir: "crates/fx".to_owned(),
            pending: true,
        }];
        let stub_only = outcomes("MissedMutant", "CaughtMutant");
        fs::create_dir_all(fx.0.join("target/mutants.out"))?;
        fs::write(fx.0.join("target/mutants.out/outcomes.json"), &stub_only)?;
        fx.write(
            "crates/fx/src/lib.rs",
            concat!(
                "#[derive(Debug, PartialEq)]\n",
                "pub enum Error {\n",
                "    Unimplemented,\n",
                "}\n",
                "\n",
                "pub fn stubbed(n: u32) -> Result<u32, Error> {\n",
                "    let _ = n;\n",
                "    Err(Error::Unimplemented)\n",
                "}\n",
                "\n",
                "pub fn real(n: u32) -> u32 {\n",
                "    n + 1\n",
                "}\n",
            ),
        )?;
        let second = Duration::from_secs(1);
        let written = fs::metadata(fx.0.join("target/mutants.out/outcomes.json"))?.modified()?;
        let before = written
            .checked_sub(second)
            .context("a timestamp before the outcomes were written")?;
        let after = written
            .checked_add(second)
            .context("a timestamp after the outcomes were written")?;
        assert!(
            mutants_outcome(&fx.0, &crates, Some(2), before).is_ok(),
            "a run that missed only a stub's mutants passes"
        );
        assert!(mutants_outcome(&fx.0, &crates, Some(0), before).is_ok());
        for code in [Some(1), Some(3), Some(4), None] {
            let failure = mutants_outcome(&fx.0, &crates, code, before)
                .expect_err("a run that did not finish keeps its failure, whatever it left behind");
            assert!(
                failure.to_string().contains("did not finish"),
                "{failure}, for {code:?}"
            );
        }
        let stale = mutants_outcome(&fx.0, &crates, Some(2), after)
            .expect_err("outcomes older than the run are not this run's result");
        assert!(
            stale
                .to_string()
                .contains("older than the diff this run read"),
            "{stale}"
        );
        Ok(())
    }

    /// The whole job, in a fixture repository, so nothing between `cargo xtask ci mutants` and its
    /// verdict can be deleted without a test noticing: `mutants -> Ok(())` and
    /// `live_tests_judge_every_mutant -> Ok(())` both fail here, which is what the independent
    /// review of #180 asked for (finding 2). `probe` has a live function and only a pending test, so
    /// the gate must refuse before it runs; `covered` has a live test, so a run would pass its
    /// baseline and report `probe`'s mutants caught while nothing ran, which is the whole defect.
    ///
    /// Four verdicts, in order: a refusal before the run for a crate no live test judges; a pass
    /// once one live test covers that crate's function, so the refusal is precise rather than a
    /// blanket one; a failure on the run's own missed-mutant status once a live, non-stub mutant
    /// survives; and nothing to do without a base.
    #[test]
    fn the_job_refuses_a_crate_whose_mutants_no_live_test_would_judge() -> Result<()> {
        let fx = Fixture::gated("mutants-job")?;
        let base = fs::read_to_string(fx.0.join("base"))?;
        let base = base.trim();

        let refused = mutants(&fx.0, Some(base)).expect_err(
            "the gate refuses `probe`: it has mutants and no live test, so a run would report them \
             caught while nothing ran",
        );
        let named = format!("{refused:#}");
        assert_eq!(
            named, "mutants: 1 problem(s)",
            "the gate's own verdict on one crate, not a tool that would not start, whose error \
             reads `failed:` instead (the message itself is pinned by \
             a_crate_with_no_live_test_cannot_have_its_mutants_counted_as_caught)"
        );
        assert!(
            !fx.0.join(MUTANTS_OUT).exists(),
            "and it refuses before the run, so the run wrote nothing"
        );

        fx.write(
            "crates/probe/tests/live.rs",
            "#[test]\nfn the_code_names_the_refusal() {\n                 assert_eq!(probe::code(true), \"refused\");\n                 assert_eq!(probe::code(false), \"accepted\");\n}\n",
        )?;
        fx.commit()?;
        mutants(&fx.0, Some(base)).context(
            "with one live test over `code`, the same diff passes: the pre-flight is precise, not a \
             refusal of every crate whose tests are pending",
        )?;
        fx.write(
            "crates/covered/tests/covered.rs",
            "#[test]\nfn the_flag_is_negated() {\n    assert!(covered::negate(false));\n}\n",
        )?;
        fx.commit()?;
        let missed = mutants(&fx.0, Some(base)).expect_err(
            "a live mutant no live test catches is the run's own missed-mutant status, and the \
             function it mutates is not a stub",
        );
        let missed = format!("{missed:#}");
        assert_eq!(missed, "mutants: 1 problem(s)", "one live mutant survived");
        assert!(
            fx.0.join(MUTANTS_OUT).exists(),
            "and this verdict is the run's own, so the run did happen: the pre-flight passed and \
             `cargo mutants` reported the miss"
        );

        assert!(
            mutants(&fx.0, None).is_ok(),
            "with no base there is no diff to judge, so the job has nothing to do"
        );
        Ok(())
    }

    /// Marks the re-run child of `a_callers_cargo_target_dir_cannot_flip_the_mutants_verdict`: the
    /// child is the process whose `cargo` children the gate spawns, and it runs with
    /// `CARGO_TARGET_DIR` in its environment, which the test cannot set in its own process because
    /// `unsafe` environment mutation is forbidden workspace-wide.
    const MUTANTS_TARGET_DIR_CHILD: &str = "XTASK_MUTANTS_TARGET_DIR_CHILD";

    /// A `CARGO_TARGET_DIR` the caller exported must not flip the gate's verdict (#419 review, nit
    /// 3, the backlog row this change lands). The variable has to be in the environment the gate's
    /// `cargo` children inherit, which is this process's own, so the test re-runs itself as a child
    /// with the variable pointing at a directory of its own. The child runs the existing test's own
    /// sequence — every mutant caught once, then the one that survives — and the parent asserts the
    /// child's exit status, and that no build tree of the gate's appears in the caller's directory:
    /// a shared verdict can flip on one run and not the next, but a pass-through always builds
    /// there, so this half cannot pass by luck.
    #[test]
    fn a_callers_cargo_target_dir_cannot_flip_the_mutants_verdict() -> Result<()> {
        if env::var_os(MUTANTS_TARGET_DIR_CHILD).is_none() {
            let dir =
                env::temp_dir().join(format!("mandate-xtask-mutants-env-{}", std::process::id()));
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
            }
            fs::create_dir_all(&dir)?;
            let child = Command::new(env::current_exe().context("finding this test's binary")?)
                .arg("--exact")
                .arg("tests::a_callers_cargo_target_dir_cannot_flip_the_mutants_verdict")
                .env(MUTANTS_TARGET_DIR_CHILD, "1")
                .env(CARGO_TARGET_DIR, &dir)
                .status()
                .context("re-running this test with a caller's CARGO_TARGET_DIR")?;
            let built = fs::read_dir(&dir)?
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .count();
            fs::remove_dir_all(&dir).ok();
            assert!(
                child.success(),
                "the child above ran the gate with CARGO_TARGET_DIR={} and its verdict held",
                dir.display()
            );
            assert_eq!(
                built, 0,
                "the gate builds in the repository it was given, so the caller's directory holds \
                 no build tree of its own (`cargo generate-lockfile`'s `.rustc_info.json` cache \
                 file, which the fixture's own setup writes, is not one)"
            );
            return Ok(());
        }
        assert!(
            env::var_os(CARGO_TARGET_DIR).is_some(),
            "the parent sets the caller's target directory on this child"
        );
        let fx = Fixture::gated("mutants-env")?;
        let base = fs::read_to_string(fx.0.join("base"))?;
        let base = base.trim();
        fx.write(
            "crates/probe/tests/live.rs",
            concat!(
                "#[test]\n",
                "fn the_code_names_the_refusal() {\n",
                "    assert_eq!(probe::code(true), \"refused\");\n",
                "    assert_eq!(probe::code(false), \"accepted\");\n",
                "}\n",
            ),
        )?;
        fx.commit()?;
        mutants(&fx.0, Some(base))
            .context("every mutant caught, with the caller's target directory exported")?;
        fx.write(
            "crates/covered/tests/covered.rs",
            concat!(
                "#[test]\n",
                "fn the_flag_is_negated() {\n",
                "    assert!(covered::negate(false));\n",
                "}\n",
            ),
        )?;
        fx.commit()?;
        let missed = mutants(&fx.0, Some(base)).expect_err(
            "a live mutant no live test catches is still the run's own missed-mutant status, \
             whatever directory the caller exported",
        );
        let missed = format!("{missed:#}");
        assert_eq!(
            missed, "mutants: 1 problem(s)",
            "the caller's CARGO_TARGET_DIR cannot stand in for the mutant's own build"
        );
        assert!(
            fx.0.join(MUTANTS_OUT).exists(),
            "and the verdict is the run's own, so the run did happen"
        );
        Ok(())
    }

    /// `safety_critical = true` governs the gate whatever the crate's layer (DEC-253): a
    /// safety-critical `tool` crate, as `mandate-refcases` is, is mutated on the diff beside the
    /// product crates, and a crate without the flag is not, in either layer.
    #[test]
    fn the_gate_mutates_every_safety_critical_crate_tool_layer_included() -> Result<()> {
        let dir = env::temp_dir().join(format!("mandate-xtask-flagged-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        let fx = Fixture(dir);
        let crates = [
            ("core", "0", true),
            ("plain", "0", false),
            ("harness", "\"tool\"", true),
            ("helper", "\"tool\"", false),
        ];
        let members: Vec<String> = crates
            .iter()
            .map(|(name, _, _)| format!("\"crates/{name}\""))
            .collect();
        fx.write(
            "Cargo.toml",
            &format!(
                "[workspace]\nmembers = [{}]\nresolver = \"3\"\n",
                members.join(", ")
            ),
        )?;
        let mut layers = String::from("impure_crates = []\n");
        for (name, layer, safety_critical) in crates {
            layers.push_str(&format!(
                "[crates.{name}]\nlayer = {layer}\nsafety_critical = {safety_critical}\npure = false\n"
            ));
            fx.write(
                &format!("crates/{name}/Cargo.toml"),
                &format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n"),
            )?;
            fx.write(&format!("crates/{name}/src/lib.rs"), "")?;
        }
        fx.write("xtask/layers.toml", &layers)?;
        fx.write(".gitignore", "/target\n")?;
        fs::copy(
            repo_root()?.join("rust-toolchain.toml"),
            fx.0.join("rust-toolchain.toml"),
        )?;
        output_in(&fx.0, "cargo", &["generate-lockfile", "--offline"])?;
        output_in(&fx.0, "git", &["init", "-q"])?;

        let mut mutated: Vec<(String, String)> = mutated_crates(&fx.0)?
            .into_iter()
            .map(|krate| (krate.package.clone(), krate.src_dir()))
            .collect();
        mutated.sort();
        assert_eq!(
            mutated,
            [
                ("core".to_owned(), "crates/core/src/".to_owned()),
                ("harness".to_owned(), "crates/harness/src/".to_owned()),
            ],
            "the flag decides: the safety-critical tool crate is mutated beside the product crate, \
             and neither unflagged crate is"
        );
        Ok(())
    }

    /// A pending marker anywhere in a crate is what makes a stub body exempt, so the gate must read
    /// each crate's own files and no other's.
    #[test]
    fn a_crates_pending_tests_are_its_own() -> Result<()> {
        let fx = Fixture::gated("pending-tests")?;
        assert!(
            has_pending_tests(&fx.0, "crates/probe")?,
            "`probe`'s only test is marked pending"
        );
        assert!(
            !has_pending_tests(&fx.0, "crates/covered")?,
            "`covered`'s test runs, and `probe`'s marker is not its own"
        );
        Ok(())
    }

    /// The job dispatch refuses a name it does not know, rather than reporting a job it never ran.
    #[test]
    fn an_unknown_ci_job_is_refused() {
        let unknown = ci("no-such-job").expect_err("an unknown job name is an error");
        assert!(unknown.to_string().contains("unknown CI job"), "{unknown}");
    }

    /// `cargo mutants --list --json`, trimmed to the one member the gate reads. Two crates have
    /// mutants: one whose tests all run and one whose only test is `#[ignore]`d.
    const LISTED_MUTANTS: &str = r#"[
        {"package": "mandate-domain", "name": "src/lib.rs:268:5: replace probe -> bool with true"},
        {"package": "mandate-probe", "name": "src/lib.rs:16:5: replace code -> &'static str with \"\""},
        {"package": "mandate-probe", "name": "src/lib.rs:16:5: replace code -> &'static str with \"xyzzy\""}
    ]"#;

    /// `cargo nextest list --message-format json`, trimmed the same way: `mandate-probe`'s one test
    /// is ignored, and `mandate-domain` has a live test beside an ignored one.
    const NEXTEST_LISTING: &str = r#"{
        "rust-suites": {
            "mandate-domain": {
                "package-name": "mandate-domain",
                "testcases": {
                    "the_probe_negates_its_flag": {"ignored": false},
                    "a_pending_one": {"ignored": true}
                }
            },
            "mandate-probe": {
                "package-name": "mandate-probe",
                "testcases": {"the_code_names_the_refusal": {"ignored": true}}
            }
        }
    }"#;

    /// `cargo mutants` tests a mutant with `cargo nextest run --package=<crate>`, and nextest exits
    /// non-zero on `no tests to run`, which it reads as the mutant being caught. So a crate with no
    /// live test must fail the gate before the run rather than have that exit code stand in for
    /// evidence (DEC-139).
    #[test]
    fn a_crate_with_no_live_test_cannot_have_its_mutants_counted_as_caught() -> Result<()> {
        let mutants = listed_mutant_counts(LISTED_MUTANTS)?;
        let live = live_test_counts(NEXTEST_LISTING)?;
        assert_eq!(mutants.get("mandate-probe").copied(), Some(2));
        assert_eq!(mutants.get("mandate-domain").copied(), Some(1));
        assert_eq!(
            live.get("mandate-domain").copied(),
            Some(1),
            "an ignored test beside a live one does not lower the live count"
        );
        assert_eq!(live.get("mandate-probe").copied(), Some(0));

        let problems = unjudged_mutants(&mutants, &live);
        assert_eq!(
            problems.len(),
            1,
            "only the crate with no live test is named: {problems:?}"
        );
        let named = problems.first().context("the one problem")?;
        assert!(
            named.contains("`mandate-probe`") && named.contains("2 mutant(s)"),
            "{named}"
        );
        assert!(
            named.contains("at least one live test"),
            "the advice is a live test, the one thing that helps, and not \"stub the code\": a \
             crate of nothing but stubs is named here too, because DEC-137 item 3 exempts a stub \
             body only on a run that tested it: {named}"
        );

        assert!(
            unjudged_mutants(&mutants, &BTreeMap::new()).len() == 2,
            "a listing that mentions neither crate leaves both unjudged"
        );
        assert!(
            unjudged_mutants(&BTreeMap::new(), &live).is_empty(),
            "a diff with no mutants has nothing to judge"
        );
        let all_live =
            live_test_counts(&NEXTEST_LISTING.replace("\"ignored\": true", "\"ignored\": false"))?;
        assert!(
            unjudged_mutants(&mutants, &all_live).is_empty(),
            "every crate with a live test is judged"
        );
        Ok(())
    }

    /// A property prints a panic for every failing case it tries, and only proptest's own report of
    /// the minimal one is the failure's cause (DEC-164). Here shrinking moves from a case that
    /// stopped at the stub to one that fails the test's own assertion, which is how #230's thirty
    /// properties passed the gate on a stub's report they had shrunk past.
    #[test]
    fn a_property_is_judged_by_its_minimal_failure_alone() {
        let shrunk_past_the_stub = concat!(
            "thread 'p' (7) panicked at crates/fx/tests/props.rs:9:9:\n",
            "called `Result::unwrap()` on an `Err` value: Unimplemented\n",
            "thread 'p' (7) panicked at crates/fx/tests/props.rs:12:5:\n",
            "the prefix's opening must reach the broker\n",
            "thread 'p' (7) panicked at crates/fx/tests/props.rs:4:1:\n",
            "Test failed: the prefix's opening must reach the broker.\n",
            "minimal failing input: n = 1\n",
            "\tsuccesses: 0\n",
            "\tlocal rejects: 0\n",
        );
        assert_eq!(
            failure_cause(shrunk_past_the_stub),
            "Test failed: the prefix's opening must reach the broker"
        );
        let stopped_at_the_stub = concat!(
            "thread 'p' (7) panicked at crates/fx/tests/props.rs:12:5:\n",
            "the prefix's opening must reach the broker\n",
            "thread 'p' (7) panicked at crates/fx/tests/props.rs:4:1:\n",
            "Test failed: called `Result::unwrap()` on an `Err` value: Unimplemented { story: ",
            "\"E7-3\" }.\n",
            "minimal failing input: n = 10\n",
        );
        assert!(names_a_stub(failure_cause(stopped_at_the_stub)));
        let input_names_a_stub = concat!(
            "thread 'p' (7) panicked at crates/fx/tests/props.rs:4:1:\n",
            "Test failed: assertion failed: fills <= 1.\n",
            "minimal failing input: Script { reply: Unimplemented }\n",
        );
        assert!(
            !names_a_stub(failure_cause(input_names_a_stub)),
            "the minimal input's `Debug` is what the test was given, not what it stopped at"
        );
        let aborted = concat!(
            "thread 'p' (7) panicked at crates/fx/tests/props.rs:9:9:\n",
            "called `Result::unwrap()` on an `Err` value: Unimplemented\n",
            "thread 'p' (7) panicked at crates/fx/tests/props.rs:4:1:\n",
            "Test aborted: Too many global rejects\n",
            "\tsuccesses: 0\n",
        );
        assert_eq!(
            failure_cause(aborted),
            "Test aborted: Too many global rejects"
        );
        let plain = "thread 't' panicked at x.rs:1:1:\nnot yet implemented\n";
        assert_eq!(
            failure_cause(plain),
            plain,
            "a test that is not a property fails on one panic, and its whole output is read"
        );

        let run = PendingTestRun {
            file: "f.rs".to_owned(),
            package: "fx".to_owned(),
            binary: Some("fx::props".to_owned()),
            test: PendingTest {
                path: "p".to_owned(),
                story: "E1-1".to_owned(),
                line: 4,
            },
        };
        let failed = |output: &str| TestOutcome {
            binary_id: "fx::props".to_owned(),
            name: "p".to_owned(),
            passed: false,
            output: output.to_owned(),
        };
        let away = verdicts(
            std::slice::from_ref(&run),
            &[failed(shrunk_past_the_stub)],
            &[],
        );
        assert!(
            away.len() == 1
                && away.iter().all(|p| p.contains("fails away from its stub")
                    && p.ends_with(
                        "It panicked with: Test failed: the prefix's opening must reach the broker"
                    )),
            "{away:?}"
        );
        assert!(verdicts(&[run], &[failed(stopped_at_the_stub)], &[]).is_empty());
    }

    /// The whole pending gate over real properties, run three times: one whose shrinking passes
    /// through a stub's report and ends on the test's own assertion is reported every time, one
    /// that stops at its stub never is, and no run leaves a `*.proptest-regressions` file for the
    /// next to replay (DEC-164). Under the whole-output reading the first passed the gate.
    #[test]
    fn a_property_that_shrinks_away_from_its_stub_is_reported_on_every_run() -> Result<()> {
        let fx = Fixture::new("pending-props")?;
        fx.write(
            "crates/fx/Cargo.toml",
            concat!(
                "[package]\nname = \"fx\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
                "[dev-dependencies]\nproptest = \"1\"\n",
            ),
        )?;
        fs::copy(repo_root()?.join("Cargo.lock"), fx.0.join("Cargo.lock"))?;
        output_in(&fx.0, "cargo", &["update", "--offline", "--workspace"])?;
        fx.write(
            "crates/fx/tests/props.rs",
            concat!(
                "use proptest::prelude::*;\n",
                "proptest! {\n",
                "    #[test]\n",
                "    #[ignore = \"pending E1-1\"]\n",
                "    fn shrinks_away_from_the_stub(n in 0u32..1_000_000) {\n",
                "        if n >= 10 {\n",
                "            fx::lookup().unwrap();\n",
                "        }\n",
                "        prop_assert!(n < 1, \"the fixture's own assertion\");\n",
                "    }\n",
                "    #[test]\n",
                "    #[ignore = \"pending E1-1\"]\n",
                "    fn stops_at_the_stub(n in 0u32..1_000_000) {\n",
                "        prop_assert_eq!(fx::lookup().unwrap(), n);\n",
                "    }\n",
                "}\n",
            ),
        )?;
        fx.commit()?;
        for round in 1..=3 {
            let problems = pending_problems(&fx.0)?;
            assert!(
                problems.len() == 1
                    && problems.iter().all(|p| p.starts_with(
                        "crates/fx/tests/props.rs:5: `shrinks_away_from_the_stub` (pending E1-1) \
                         fails away from its stub"
                    ) && p
                        .contains("It panicked with: Test failed: the fixture's own assertion")),
                "run {round}: {problems:?}"
            );
            let saved = proptest_seeds_in(&fx.0)?;
            assert!(
                saved.is_empty(),
                "run {round} saved failures for the next run to replay: {saved:?}"
            );
        }
        Ok(())
    }

    /// A seed of either shape is found whether it is committed, untracked, or ignored, and a file
    /// that only resembles one is not (DEC-381).
    #[test]
    fn a_saved_proptest_seed_of_either_shape_is_found_tracked_or_not() -> Result<()> {
        let fx = Fixture::new("proptest-seeds")?;
        fx.write(".gitignore", "/target\nproptest-regressions/\n")?;
        fx.write("crates/fx/src/regressions.rs", "")?;
        fx.commit()?;
        assert_eq!(proptest_seeds_in(&fx.0)?, Vec::<String>::new());
        fx.write("crates/fx/proptest-regressions/props.txt", "cc 00\n")?;
        output_in(
            &fx.0,
            "git",
            &["add", "-f", "crates/fx/proptest-regressions/props.txt"],
        )?;
        fx.commit()?;
        fx.write("crates/fx/proptest-regressions/score.txt", "cc 01\n")?;
        fx.write("crates/fx/tests/props.proptest-regressions", "cc 02\n")?;
        let mut found = proptest_seeds_in(&fx.0)?;
        found.sort();
        assert_eq!(
            found,
            [
                "crates/fx/proptest-regressions/props.txt",
                "crates/fx/proptest-regressions/score.txt",
                "crates/fx/tests/props.proptest-regressions",
            ]
        );
        Ok(())
    }

    /// `cargo mutants --list --json --in-diff` prints nothing, not `[]`, for a diff with no
    /// mutants, which is every diff that touches only `#[cfg(test)]` code (#227, round-2 delta
    /// review). That is zero mutants and a job with nothing to do; other output that is not a JSON
    /// list is still an error.
    #[test]
    fn an_empty_mutants_listing_is_zero_mutants() -> Result<()> {
        assert!(listed_mutant_counts("")?.is_empty());
        assert!(listed_mutant_counts("\n")?.is_empty());
        assert!(listed_mutant_counts("[]")?.is_empty());
        assert!(listed_mutant_counts("[").is_err());
        assert!(
            listed_mutant_counts("[{").is_err(),
            "a truncated listing is not read as no mutants"
        );
        assert!(listed_mutant_counts("no mutants").is_err());

        let fx = Fixture::gated("mutants-test-only")?;
        fx.write(
            "crates/probe/tests/probe.rs",
            "#[test]\nfn the_code_names_the_refusal() {\n    assert_eq!(probe::code(true), \"refused\");\n    assert_eq!(probe::code(false), \"accepted\");\n}\n",
        )?;
        fx.commit()?;
        let base = output_in(&fx.0, "git", &["rev-parse", "HEAD"])?;
        fx.write(
            "crates/covered/src/lib.rs",
            concat!(
                "#[must_use]\npub fn negate(flag: bool) -> bool {\n    !flag\n}\n",
                "#[cfg(test)]\nmod tests {\n    #[test]\n",
                "    fn negates() {\n        assert!(super::negate(false));\n    }\n}\n",
            ),
        )?;
        fx.commit()?;
        mutants(&fx.0, Some(base.trim()))
            .context("a diff touching only `#[cfg(test)]` code has no mutants to judge")?;
        assert!(
            !fx.0.join(MUTANTS_OUT).exists(),
            "and the job ends at the listing, without a run that could test nothing"
        );
        Ok(())
    }

    #[test]
    fn only_a_stubs_own_report_names_a_stub() {
        assert!(names_a_stub(
            "called `Result::unwrap()` on an `Err` value: Unimplemented"
        ));
        assert!(names_a_stub("left: Err(Unimplemented { story: \"E6-1\" })"));
        assert!(names_a_stub(
            "Test failed: E6-1 has not been implemented yet."
        ));
        assert!(names_a_stub(
            "Test failed: this rule is not implemented yet."
        ));
        assert!(names_a_stub(
            "thread 'x' panicked at s.rs:1:1: not yet implemented"
        ));
        assert!(names_a_stub("refused: unimplemented"));
        assert!(
            !names_a_stub("Test failed: E6-4 fixture: invalid digit found in string"),
            "a story id in the test's own message is not the stub's report"
        );
        assert!(
            !names_a_stub("an `Err` value: Overflow"),
            "an error that names no stub is not one; the crate's stubs are fixed instead"
        );
        assert!(!names_a_stub("assertion failed: `(left == right)`"));
        assert!(
            !names_a_stub("Unimplementedish is a word of its own"),
            "a one-word marker matches as a whole word"
        );
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

        let away = concat!(
            "#[test]\n#[ignore = \"pending E1-1\"]\n",
            "fn fails_on_a_fixture() { panic!(\"fixture file missing\") }\n",
        );
        fx.write(
            "crates/fx/tests/stubs.rs",
            &format!("{live}{failing}{away}"),
        )?;
        fx.commit()?;
        let problems = pending_problems(&fx.0)?;
        let problem = problems.first().map(String::as_str).unwrap_or_default();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problem.starts_with(
                "crates/fx/tests/stubs.rs:16: `fails_on_a_fixture` (pending E1-1) fails away from \
                 its stub"
            ),
            "{problem}"
        );
        assert!(
            problem.contains("It panicked with: thread 'fails_on_a_fixture'"),
            "{problem}"
        );
        assert!(problem.contains("fixture file missing"), "{problem}");

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
                "crates/fx/src/lib.rs:11: `tests::unit_passes` (pending E1-1) passes on this change's code",
                "crates/fx/tests/stubs.rs:16: `passes_on_the_stubs` (pending E1-1) passes on this change's code",
                "crates/fx/tests/stubs.rs:20: `never_compiled` (pending E1-1) did not run",
                "crates/fx/tests/untracked.rs:3: `untracked_and_passing` (pending E1-2) passes on this change's code",
            ]
        );
        Ok(())
    }

    /// A status file with five passing cases and one pending, and a fixture naming all six.
    fn status_fixture(name: &str) -> Result<(Fixture, String)> {
        let dir = env::temp_dir().join(format!("mandate-xtask-{name}-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        let fx = Fixture(dir);
        fx.write(
            "crates/mandate-refcases/status.toml",
            concat!(
                "[mandate]\n",
                "'MC-A' = { status = \"passing\", story = \"E1-1\" }\n",
                "'MC-B' = { status = \"passing\", story = \"E1-1\" }\n",
                "'MC-C' = { status = \"pending\", story = \"E1-1\" }\n",
                "'MC-D' = { status = \"passing\", story = \"E1-1\" }\n",
                "'MC-E' = { status = \"passing\", story = \"E1-1\" }\n",
                "[trading_domain]\n",
                "'TD-A' = { status = \"passing\", story = \"E1-1\" }\n",
            ),
        )?;
        let cases: Vec<Value> = ["MC-A", "MC-B", "MC-C", "MC-D", "MC-E"]
            .iter()
            .map(|id| json!({"id": id, "expect": {"journal": ["Old"]}}))
            .collect();
        fx.write(
            "fixtures/refcases/mandate.json",
            &json!({"cases": cases}).to_string(),
        )?;
        fx.write(
            "fixtures/refcases/trading-domain.json",
            &json!({"cases": [{"id": "TD-A", "expect": {"fill": "1"}}]}).to_string(),
        )?;
        output_in(&fx.0, "git", &["init", "-q", "-b", "main"])?;
        fx.commit_with("base")?;
        let base = fx.git(&["rev-parse", "HEAD"])?;
        Ok((fx, base))
    }

    /// ES-11 as DEC-277 amends it. Planted against one base: a passing case going pending with its
    /// fixture entry unchanged (MC-A), one going absent although its entry changed (MC-B), and one
    /// going pending while only another case's entry changed (MC-E) are refused, naming each; one
    /// going pending with its own entry changed (MC-D, in `trading_domain` too, read from
    /// `trading-domain.json`) and a pending case going passing (MC-C) are allowed. The spec guard
    /// reports the same refusals.
    #[test]
    fn a_passing_case_goes_pending_only_with_its_own_fixture_entry_and_never_absent() -> Result<()>
    {
        let (fx, base) = status_fixture("status-flips")?;
        assert_eq!(status_flip_problems(&fx.0, &base)?, Vec::<String>::new());
        fx.write(
            "crates/mandate-refcases/status.toml",
            concat!(
                "[mandate]\n",
                "'MC-A' = { status = \"pending\", story = \"E1-1\" }\n",
                "'MC-C' = { status = \"passing\", story = \"E1-1\" }\n",
                "'MC-D' = { status = \"pending\", story = \"E1-1\" }\n",
                "'MC-E' = { status = \"pending\", story = \"E1-1\" }\n",
                "[trading_domain]\n",
                "'TD-A' = { status = \"pending\", story = \"E1-1\" }\n",
            ),
        )?;
        let cases: Vec<Value> = ["MC-A", "MC-B", "MC-C", "MC-D", "MC-E"]
            .iter()
            .map(|id| {
                let moved = ["MC-B", "MC-D"].contains(id);
                json!({"id": id, "expect": {"journal": [if moved { "New" } else { "Old" }]}})
            })
            .collect();
        fx.write(
            "fixtures/refcases/mandate.json",
            &json!({"cases": cases}).to_string(),
        )?;
        fx.write(
            "fixtures/refcases/trading-domain.json",
            &json!({"cases": [{"id": "TD-A", "expect": {"fill": "2"}}]}).to_string(),
        )?;
        let problems = status_flip_problems(&fx.0, &base)?;
        let named: Vec<&str> = problems
            .iter()
            .map(|p| p.split('`').nth(1).unwrap_or_default())
            .collect();
        assert_eq!(
            named,
            ["mandate::MC-A", "mandate::MC-B", "mandate::MC-E"],
            "{problems:?}"
        );
        assert!(problems[0].contains("passing to pending"), "{problems:?}");
        assert!(problems[1].contains("passing to absent"), "{problems:?}");
        assert!(problems[2].contains("passing to pending"), "{problems:?}");

        fx.commit_with("flips, citing DEC-9")?;
        let guarded = spec_guard_problems(&fx.0, &base, "")?;
        assert_eq!(
            guarded.len(),
            3,
            "the spec guard refuses the same three: {guarded:?}"
        );
        Ok(())
    }

    /// A status file the base does not have yet has nothing to regress from, and deleting the whole
    /// file takes every passing case to absent.
    #[test]
    fn a_new_status_file_flips_nothing_and_a_deleted_one_drops_every_passing_case() -> Result<()> {
        let (fx, base) = status_fixture("status-deleted")?;
        fs::remove_file(fx.0.join("crates/mandate-refcases/status.toml"))?;
        let problems = status_flip_problems(&fx.0, &base)?;
        assert_eq!(
            problems.len(),
            5,
            "MC-A, MC-B, MC-D, MC-E, TD-A: {problems:?}"
        );
        assert!(
            problems.iter().all(|p| p.contains("passing to absent")),
            "{problems:?}"
        );
        fx.commit_with("no status yet")?;
        let empty = fx.git(&["rev-parse", "HEAD"])?;
        fx.write(
            "crates/mandate-refcases/status.toml",
            "[mandate]\n'MC-A' = { status = \"pending\", story = \"E1-1\" }\n",
        )?;
        assert_eq!(status_flip_problems(&fx.0, &empty)?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_pull_request_is_diffed_against_the_main_it_was_merged_onto() -> Result<()> {
        let MovedBase {
            fx,
            old_base,
            main_tip,
        } = Fixture::moved_base("base-moved", &["crates/c/src/lib.rs"])?;
        let base = base_ref_in(&fx.0, None, Some("pull_request"))?;
        assert_eq!(
            base.as_deref(),
            Some(main_tip.as_str()),
            "the merge commit's first parent, not the PR's base.sha"
        );
        assert_eq!(
            spec_guard_problems(&fx.0, &main_tip, "")?,
            Vec::<String>::new(),
            "main's own spec change is not the PR's"
        );
        let stale = spec_guard_problems(&fx.0, &old_base, "")?;
        assert!(
            stale.iter().any(|p| p.contains("together with code")),
            "the stale base.sha is what flagged #243 and #258: {stale:?}"
        );
        Ok(())
    }

    #[test]
    fn a_pull_request_changing_a_spec_and_code_is_still_flagged() -> Result<()> {
        let MovedBase { fx, main_tip, .. } = Fixture::moved_base(
            "base-mixed",
            &["docs/specs/pr.md", "schemas/pr.json", "crates/c/src/lib.rs"],
        )?;
        let base = base_ref_in(&fx.0, None, Some("pull_request"))?;
        assert_eq!(base.as_deref(), Some(main_tip.as_str()));
        let problems = spec_guard_problems(&fx.0, &main_tip, "")?;
        assert!(
            problems.iter().any(|p| p.contains("together with code")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("no DEC-<n> is cited")),
            "main's DEC-1 commit is not the PR's citation: {problems:?}"
        );
        let cited = spec_guard_problems(&fx.0, &main_tip, "Cites DEC-5.")?;
        assert_eq!(
            cited.len(),
            1,
            "a citation clears only the DEC problem: {cited:?}"
        );
        assert!(cited[0].contains("together with code"), "{cited:?}");
        let spec_only = Fixture::moved_base("base-spec-only", &["docs/specs/pr.md"])?;
        let problems = spec_guard_problems(&spec_only.fx.0, &spec_only.main_tip, "DEC-5")?;
        assert_eq!(
            problems,
            Vec::<String>::new(),
            "a cited spec-only PR passes"
        );
        Ok(())
    }

    #[test]
    fn only_pr_598_with_the_exact_dec_462_marker_may_mix_spec_and_code() -> Result<()> {
        let MovedBase { fx, main_tip, .. } = Fixture::moved_base(
            "base-e77-atomic",
            &["docs/specs/pr.md", "crates/c/src/lib.rs"],
        )?;
        let marker = "ES-22-atomic-exception: E7-7 PR #598 (DEC-462)";
        assert_eq!(
            spec_guard_problems_for_pr(&fx.0, &main_tip, marker, Some(598))?,
            Vec::<String>::new(),
            "the founder-approved E7-7 atomic migration passes only on PR 598"
        );
        for (body, pr_number) in [(marker, Some(599)), ("DEC-462", Some(598)), (marker, None)] {
            let problems = spec_guard_problems_for_pr(&fx.0, &main_tip, body, pr_number)?;
            assert!(
                problems.iter().any(|p| p.contains("together with code")),
                "an adjacent or unmarked change remains guarded: {problems:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn runs_outside_a_pull_request_keep_their_base() -> Result<()> {
        let MovedBase {
            fx,
            old_base,
            main_tip,
        } = Fixture::moved_base("base-push", &["crates/c/src/lib.rs"])?;
        let pr_tip = fx.git(&["rev-parse", "pr"])?;
        assert_eq!(
            base_ref_in(&fx.0, None, Some("push"))?.as_deref(),
            Some(old_base.as_str()),
            "outside a pull_request event the merge base with origin/main is the base"
        );
        fx.git(&["update-ref", "refs/remotes/origin/main", &main_tip])?;
        fx.git(&["switch", "-q", "--detach", &main_tip])?;
        for event in [
            Some("push"),
            Some("schedule"),
            Some("workflow_dispatch"),
            None,
        ] {
            assert_eq!(
                base_ref_in(&fx.0, None, event)?,
                None,
                "a run on main's tip has no base ({event:?})"
            );
        }
        assert!(
            base_ref_in(&fx.0, None, Some("pull_request")).is_err(),
            "a pull_request run with no base is an error, never an empty diff"
        );
        fx.git(&["switch", "-q", "--detach", &pr_tip])?;
        for event in [Some("workflow_dispatch"), Some("pull_request"), None] {
            assert_eq!(
                base_ref_in(&fx.0, None, event)?.as_deref(),
                Some(old_base.as_str()),
                "a branch tip that is no merge commit is compared from its merge base with origin/main ({event:?})"
            );
        }
        Ok(())
    }

    /// DEC-538: the plan's matrix is `min(192, n)` shards, as `GITHUB_OUTPUT` lines CI reads with
    /// `fromJSON`, and an empty list when the diff has no mutant, which is what skips the matrix.
    #[test]
    fn the_plan_sizes_the_matrix_to_the_diff() -> Result<()> {
        assert_eq!(
            MutantPlan::for_mutants(0).outputs(),
            "mutants=0\nshards=[]\ntotal=0\n",
            "no mutants, no shards"
        );
        assert_eq!(
            MutantPlan::for_mutants(10).outputs(),
            "mutants=10\nshards=[0,1,2,3,4,5,6,7,8,9]\ntotal=10\n",
            "ten mutants, one a shard"
        );
        for (mutants, shards) in [(1, 1), (191, 191), (192, 192), (193, 192), (500, 192)] {
            let plan = MutantPlan::for_mutants(mutants);
            assert_eq!(plan.shards, shards, "{mutants} mutants");
            let outputs = plan.outputs();
            let listed = outputs
                .lines()
                .find_map(|line| line.strip_prefix("shards="))
                .context("the plan names its shards")?;
            let indices: Vec<usize> = serde_json::from_str(listed)?;
            assert_eq!(
                indices,
                (0..shards).collect::<Vec<_>>(),
                "the matrix is every index below the plan's total, for {mutants} mutants"
            );
            assert!(
                outputs.contains(&format!("\ntotal={shards}\n")),
                "and each shard names that total: {outputs}"
            );
        }
        Ok(())
    }

    /// The slice shards `cargo-mutants` gives a mutant listing, computed the way its `--sharding
    /// slice` documents them (runs of `ceil(n / N)`), as an oracle independent of the plan.
    fn slices(n: usize, total: usize) -> Vec<std::ops::Range<usize>> {
        let run = n.div_ceil(total);
        (0..total)
            .map(|k| {
                let start = k.saturating_mul(run).min(n);
                start..start.saturating_add(run).min(n)
            })
            .collect()
    }

    /// DEC-538's per-shard load, for every diff size up to well past the ceiling: shard `k` of the
    /// plan's `N` takes exactly the mutants shard `k` of 192 took, so no shard carries more than it
    /// did and a diff under 192 mutants still puts one in each; and every shard of 192 the plan
    /// drops was empty.
    #[test]
    fn no_planned_shard_carries_more_than_its_slice_of_192() {
        for n in 0..=1000 {
            let plan = MutantPlan::for_mutants(n);
            let planned = slices(n, plan.shards.max(1));
            let before = slices(n, MUTANT_SHARDS);
            for (k, slice) in before.iter().enumerate() {
                if k < plan.shards {
                    assert_eq!(
                        planned.get(k),
                        Some(slice),
                        "shard {k} of {} tests what shard {k} of 192 did, for {n} mutants",
                        plan.shards
                    );
                } else {
                    assert!(
                        slice.is_empty(),
                        "shard {k} of 192 the plan drops tested nothing, for {n} mutants"
                    );
                }
            }
            if n > 0 && n < MUTANT_SHARDS {
                assert!(planned.iter().all(|slice| slice.len() == 1), "{n} mutants");
            }
        }
    }

    /// `cargo mutants --list --json` over the whole diff from `base`, for one shard or unsharded,
    /// each mutant as its JSON text. The oracle the plan is compared against: it writes its own
    /// diff and asks the tool itself which mutants each shard tests.
    fn listed_by_cargo_mutants(
        fx: &Fixture,
        diff: &Path,
        shard: Option<(usize, usize)>,
    ) -> Result<Vec<String>> {
        let diff = diff.to_str().context("non-UTF-8 temp path")?;
        let argument = shard.map(|(k, total)| format!("{k}/{total}"));
        let mut args = vec!["mutants", "--list", "--json", "--in-diff", diff];
        if let Some(argument) = argument.as_deref() {
            args.extend(["--shard", argument, "--sharding", "slice"]);
        }
        let listing = output_in(&fx.0, "cargo", &args)?;
        if listing.trim().is_empty() {
            return Ok(Vec::new());
        }
        let listed: Vec<Value> = serde_json::from_str(&listing)?;
        Ok(listed.iter().map(Value::to_string).collect())
    }

    /// DEC-538 in a fixture repository, against `cargo mutants` itself: the plan counts the mutants
    /// the tool lists for the diff; the plan's shards between them test that whole set; shard `k`
    /// of the plan tests what shard `k` of 192 tests; the shards the plan drops test nothing; and a
    /// shard is refused when its listing or its total disagrees with the plan that launched it.
    #[test]
    fn the_planned_shards_test_the_gates_mutants_and_no_more_per_shard() -> Result<()> {
        let fx = Fixture::gated("mutants-plan")?;
        let base = fs::read_to_string(fx.0.join("base"))?;
        let base = base.trim();
        let diff =
            env::temp_dir().join(format!("mandate-xtask-plan-oracle-{}", std::process::id()));
        fs::write(&diff, fx.git(&["diff", &format!("{base}...HEAD")])? + "\n")?;

        let plan = mutants_plan(&fx.0, Some(base))?;
        let mut everything = listed_by_cargo_mutants(&fx, &diff, None)?;
        everything.sort();
        assert!(
            plan.mutants > 1,
            "the fixture's diff has several mutants, so a shard split means something"
        );
        assert_eq!(
            plan,
            MutantPlan::for_mutants(everything.len()),
            "the plan counts what `cargo mutants` lists for the diff"
        );

        let mut union = Vec::new();
        for k in 0..MUTANT_SHARDS {
            let before = listed_by_cargo_mutants(&fx, &diff, Some((k, MUTANT_SHARDS)))?;
            if k < plan.shards {
                let planned = listed_by_cargo_mutants(&fx, &diff, Some((k, plan.shards)))?;
                assert_eq!(
                    planned, before,
                    "shard {k} of {} tests what shard {k} of 192 tests",
                    plan.shards
                );
                assert_eq!(planned.len(), 1, "one mutant a shard, as under 192");
                union.extend(planned);
            } else {
                assert!(
                    before.is_empty(),
                    "shard {k} of 192, which the plan drops, tests nothing: {before:?}"
                );
            }
        }
        union.sort();
        assert_eq!(
            union, everything,
            "the plan's shards together test every mutant"
        );
        fs::remove_file(&diff).ok();

        let wrong_count = mutants_scheduled(&fx.0, Some(base), None, Some(plan.mutants + 1))
            .expect_err("a shard whose listing differs from the plan is refused");
        assert!(wrong_count.to_string().contains("plan"), "{wrong_count}");
        let wrong_total = mutants_scheduled(
            &fx.0,
            Some(base),
            Some(MutantShard::parse(&format!("0/{MUTANT_SHARDS}"))?),
            Some(plan.mutants),
        )
        .expect_err("a shard total other than the plan's is refused");
        assert!(wrong_total.to_string().contains("shard"), "{wrong_total}");
        assert!(
            !fx.0.join(MUTANTS_OUT).exists(),
            "both refusals come before the run"
        );
        Ok(())
    }

    /// DEC-538: the plan reports zero exactly when the gate would test nothing, for each of the
    /// gate's three ways of having nothing to do: no base, no safety-critical source changed, and
    /// changed source that generates no mutant. In each, a shard told the plan's zero passes.
    #[test]
    fn the_plan_counts_zero_exactly_when_the_gate_has_nothing_to_test() -> Result<()> {
        let fx = Fixture::gated("mutants-plan-zero")?;
        let head = fx.git(&["rev-parse", "HEAD"])?;
        assert_eq!(mutants_plan(&fx.0, None)?, MutantPlan::for_mutants(0));
        mutants_scheduled(&fx.0, None, None, Some(0))?;

        fx.write(
            "crates/covered/tests/more.rs",
            "#[test]\nfn negates_true() {\n    assert!(!covered::negate(true));\n}\n",
        )?;
        fx.commit()?;
        assert_eq!(
            mutants_plan(&fx.0, Some(&head))?,
            MutantPlan::for_mutants(0),
            "a tests-only change"
        );
        mutants_scheduled(&fx.0, Some(&head), None, Some(0))?;

        fx.write(
            "crates/covered/src/lib.rs",
            concat!(
                "#[must_use]\npub fn negate(flag: bool) -> bool {\n    !flag\n}\n",
                "\n#[cfg(test)]\nmod unit {\n    #[test]\n    fn negates_false() {\n",
                "        assert!(super::negate(false));\n    }\n}\n",
            ),
        )?;
        fx.commit()?;
        assert_eq!(
            mutants_plan(&fx.0, Some(&head))?,
            MutantPlan::for_mutants(0),
            "changed source that generates no mutant"
        );
        mutants_scheduled(&fx.0, Some(&head), None, Some(0))?;
        assert!(
            !fx.0.join(MUTANTS_OUT).exists(),
            "and the gate never started a run"
        );

        fx.write(
            "crates/covered/src/lib.rs",
            "#[must_use]\npub fn negate(flag: bool) -> bool {\n    flag ^ true\n}\n",
        )?;
        fx.commit()?;
        assert!(
            mutants_plan(&fx.0, Some(&head))?.mutants > 0,
            "and a change that does generate a mutant is counted"
        );
        Ok(())
    }

    #[test]
    fn a_shards_schedule_must_match_its_plan() -> Result<()> {
        check_schedule(5, None, None)?;
        check_schedule(5, Some(MutantShard::parse("3/192")?), None)?;
        check_schedule(5, Some(MutantShard::parse("3/5")?), Some(5))?;
        check_schedule(500, Some(MutantShard::parse("191/192")?), Some(500))?;
        check_schedule(0, None, Some(0))?;
        assert!(check_schedule(4, None, Some(5)).is_err(), "fewer listed");
        assert!(check_schedule(6, None, Some(5)).is_err(), "more listed");
        assert!(
            check_schedule(5, Some(MutantShard::parse("3/192")?), Some(5)).is_err(),
            "a 192-way shard under a plan of five"
        );
        assert!(
            check_schedule(500, Some(MutantShard::parse("3/191")?), Some(500)).is_err(),
            "a total below the ceiling for a plan above it"
        );
        Ok(())
    }

    /// DEC-538 in `ci.yml`: the plan job runs the plan and exposes its three outputs; the matrix
    /// comes from it, is skipped only on a zero count, and passes the plan's total and count to
    /// each shard; and `full` needs all three jobs.
    #[test]
    fn ci_sizes_the_mutation_matrix_from_the_plan() -> Result<()> {
        let workflow = fs::read_to_string(repo_root()?.join(".github/workflows/ci.yml"))?;
        let trimmed = |job: &str| -> Result<Vec<String>> {
            Ok(workflow_job(&workflow, job)
                .with_context(|| format!("ci.yml has a `{job}` job"))?
                .iter()
                .map(|line| line.trim().trim_start_matches("- ").to_owned())
                .collect())
        };
        let plan = trimmed("mutants-plan")?;
        for line in [
            "cargo xtask ci mutants --plan >> \"$GITHUB_OUTPUT\"",
            "mutants: ${{ steps.plan.outputs.mutants }}",
            "shards: ${{ steps.plan.outputs.shards }}",
            "total: ${{ steps.plan.outputs.total }}",
            "printf 'mutants=0\\nshards=[]\\ntotal=0\\n' >> \"$GITHUB_OUTPUT\"",
        ] {
            assert!(
                plan.iter().any(|l| l == line),
                "`mutants-plan` has `{line}`"
            );
        }
        let matrix = trimmed("mutants")?;
        for line in [
            "needs: mutants-plan",
            "if: needs.mutants-plan.outputs.mutants != '0'",
            "shard: ${{ fromJSON(needs.mutants-plan.outputs.shards) }}",
            "MANDATE_MUTANT_SHARD: ${{ matrix.shard }}/${{ needs.mutants-plan.outputs.total }}",
            "MANDATE_MUTANTS_PLANNED: ${{ needs.mutants-plan.outputs.mutants }}",
        ] {
            assert!(matrix.iter().any(|l| l == line), "`mutants` has `{line}`");
        }
        assert!(
            !workflow.contains("/192"),
            "no shard total is fixed in the workflow"
        );
        let full = trimmed("full")?;
        assert!(
            full.iter()
                .any(|l| l == "needs: [full-checks, mutants-plan, mutants]"),
            "`full` needs the plan as well as the matrix"
        );
        Ok(())
    }

    /// The `run:` script of the `full` job, as GitHub runs it.
    fn full_verdict_script(workflow: &str) -> Result<String> {
        let lines = workflow_job(workflow, "full").context("ci.yml has a `full` job")?;
        let start = lines
            .iter()
            .position(|line| line.trim() == "run: |")
            .context("`full` has a `run: |` script")?;
        let script: Vec<&str> = lines
            .iter()
            .skip(start.saturating_add(1))
            .take_while(|line| line.starts_with("          ") || line.trim().is_empty())
            .map(|line| line.get(10..).unwrap_or(""))
            .collect();
        Ok(script.join("\n") + "\n")
    }

    /// DEC-538's `full` verdict, run as GitHub runs it (`bash -e`) over every combination of the
    /// three jobs' results and the plan's count, against a truth table written out here: the
    /// matrix's skip passes only beside a plan that succeeded and counted zero.
    #[test]
    fn full_accepts_a_skipped_matrix_only_beside_a_plan_of_zero() -> Result<()> {
        let workflow = fs::read_to_string(repo_root()?.join(".github/workflows/ci.yml"))?;
        let script = full_verdict_script(&workflow)?;
        let results = ["success", "failure", "cancelled", "skipped"];
        let mut checked = 0usize;
        for checks in results {
            for plan in results {
                for planned in ["0", "1", "10", "500", "", "01", "x", "-1"] {
                    for matrix in results {
                        let expected = checks == "success"
                            && plan == "success"
                            && match planned {
                                "0" => matrix == "skipped",
                                "1" | "10" | "500" => matrix == "success",
                                _ => false,
                            };
                        let status = Command::new("bash")
                            .args(["-e", "-c", &script])
                            .env("FULL_CHECKS_RESULT", checks)
                            .env("PLAN_RESULT", plan)
                            .env("PLANNED_MUTANTS", planned)
                            .env("MUTANTS_RESULT", matrix)
                            .status()?;
                        assert_eq!(
                            status.success(),
                            expected,
                            "full-checks {checks}, plan {plan} counting {planned:?}, mutants {matrix}"
                        );
                        checked = checked.saturating_add(1);
                    }
                }
            }
        }
        assert_eq!(checked, 512);
        Ok(())
    }

    /// DEC-610: a draft runs no `ci` job. `fast`, `full-checks` and the mutation plan are skipped
    /// while the pull request is a draft, the matrix follows its plan, `full` is skipped too rather
    /// than judging skipped jobs, marking a pull request ready starts its run, and a newer push
    /// still cancels the older run of the same ref.
    #[test]
    fn ci_runs_nothing_on_a_draft() -> Result<()> {
        let workflow = fs::read_to_string(repo_root()?.join(".github/workflows/ci.yml"))?;
        assert!(
            workflow.contains(
                "  pull_request:\n    types: [opened, synchronize, reopened, ready_for_review]\n"
            ),
            "marking a pull request ready for review starts the run that judges it"
        );
        assert!(
            workflow.contains(
                "concurrency:\n  group: ci-${{ github.ref }}\n  cancel-in-progress: true\n"
            ),
            "a newer run of the same ref cancels the older one"
        );
        for (job, guard) in [
            ("fast", "if: ${{ !github.event.pull_request.draft }}"),
            ("full-checks", "if: ${{ !github.event.pull_request.draft }}"),
            (
                "mutants-plan",
                "if: ${{ !github.event.pull_request.draft }}",
            ),
            (
                "full",
                "if: ${{ always() && !github.event.pull_request.draft }}",
            ),
            ("mutants", "needs: mutants-plan"),
        ] {
            let lines = workflow_job(&workflow, job)
                .with_context(|| format!("ci.yml has a `{job}` job"))?;
            assert!(
                lines.iter().any(|line| line.trim() == guard),
                "`{job}` has `{guard}`, so a draft runs it not at all"
            );
        }
        Ok(())
    }

    #[test]
    fn ci_leaves_the_base_to_the_event() -> Result<()> {
        let workflow = fs::read_to_string(repo_root()?.join(".github/workflows/ci.yml"))?;
        let setters: Vec<&str> = workflow
            .lines()
            .filter(|line| !line.trim_start().starts_with('#') && line.contains("MANDATE_BASE_REF"))
            .collect();
        assert!(
            setters.is_empty(),
            "CI must not pin MANDATE_BASE_REF (a PR's base.sha misses main's later changes; #243, #258): {setters:?}"
        );
        let mut steps: Vec<Vec<&str>> = Vec::new();
        for line in workflow.lines() {
            if line.trim_start().starts_with("- ") || steps.is_empty() {
                steps.push(Vec::new());
            }
            if let Some(step) = steps.last_mut() {
                step.push(line.trim());
            }
        }
        let checkouts: Vec<&Vec<&str>> = steps
            .iter()
            .filter(|step| {
                step.iter()
                    .any(|line| line.contains("uses: actions/checkout@"))
            })
            .collect();
        assert_eq!(
            checkouts.len(),
            4,
            "one checkout in each source-reading job: `fast`, `full-checks`, the mutation plan, and \
             the mutants matrix"
        );
        for checkout in checkouts {
            assert!(
                checkout.contains(&"fetch-depth: 0"),
                "every checkout keeps the merge commit's parents and origin/main: {checkout:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_shallow_pull_request_checkout_fails_instead_of_checking_nothing() -> Result<()> {
        let MovedBase { fx, .. } =
            Fixture::moved_base("base-shallow-src", &["crates/c/src/lib.rs"])?;
        let script = repo_root()?.join(".github/scripts/base-ref.sh");
        let docs_only = repo_root()?.join(".github/scripts/docs-only.sh");
        let dir =
            env::temp_dir().join(format!("mandate-xtask-base-shallow-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        let url = format!("file://{}", fx.0.display());
        output_in(
            &env::temp_dir(),
            "git",
            &["clone", "-q", "--depth", "1", &url, &dir.to_string_lossy()],
        )?;
        let shallow = Fixture(dir);
        assert_eq!(
            shallow
                .git(&["rev-list", "--parents", "-n", "1", "HEAD"])?
                .split_whitespace()
                .count(),
            1,
            "the shallow merge commit shows no parents"
        );
        for without_origin_main in [false, true] {
            if without_origin_main {
                shallow.git(&["update-ref", "-d", "refs/remotes/origin/main"])?;
            }
            let refused = base_ref_in(&shallow.0, None, Some("pull_request"))
                .expect_err("a pull_request run that resolves no base must fail");
            assert!(
                refused.to_string().contains("fetch-depth: 0"),
                "the error names the fix: {refused}"
            );
            assert_eq!(
                base_ref_in(&shallow.0, None, None)?,
                None,
                "outside a pull_request event a missing base still means nothing to compare"
            );
            for program in [&script, &docs_only] {
                let out = Command::new("bash")
                    .arg(program)
                    .current_dir(&shallow.0)
                    .env_remove("MANDATE_BASE_REF")
                    .env("GITHUB_EVENT_NAME", "pull_request")
                    .output()?;
                assert!(
                    !out.status.success(),
                    "{} must fail on a shallow pull_request checkout: {out:?}",
                    program.display()
                );
                assert!(
                    String::from_utf8_lossy(&out.stderr).contains("fetch-depth: 0"),
                    "{out:?}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn an_explicit_base_overrides_the_event() -> Result<()> {
        let MovedBase {
            fx,
            old_base,
            main_tip,
        } = Fixture::moved_base("base-explicit", &["crates/c/src/lib.rs"])?;
        assert_eq!(
            base_ref_in(&fx.0, Some(&old_base), Some("pull_request"))?.as_deref(),
            Some(old_base.as_str())
        );
        assert_eq!(
            base_ref_in(&fx.0, Some(&"0".repeat(40)), None)?,
            None,
            "an all-zero base is a branch's first push: no base"
        );
        assert!(
            base_ref_in(&fx.0, Some(&"0".repeat(40)), Some("pull_request")).is_err(),
            "but a pull request always has one"
        );
        assert_eq!(
            base_ref_in(&fx.0, Some(""), Some("pull_request"))?.as_deref(),
            Some(main_tip.as_str()),
            "an empty override is no override"
        );
        Ok(())
    }

    #[test]
    fn the_short_path_picks_the_base_xtask_picks() -> Result<()> {
        let MovedBase {
            fx,
            old_base,
            main_tip,
        } = Fixture::moved_base("base-script", &["crates/c/src/lib.rs"])?;
        let script = repo_root()?.join(".github/scripts/base-ref.sh");
        let merge = fx.git(&["rev-parse", "HEAD"])?;
        let pr_tip = fx.git(&["rev-parse", "pr"])?;
        let zeros = "0".repeat(40);
        let mut compared = 0;
        for checkout in [&merge, &main_tip, &pr_tip] {
            fx.git(&["switch", "-q", "--detach", checkout])?;
            for explicit in [
                None,
                Some(""),
                Some(old_base.as_str()),
                Some(zeros.as_str()),
            ] {
                for event in [None, Some("pull_request"), Some("push")] {
                    let mut command = Command::new("bash");
                    command
                        .arg(&script)
                        .current_dir(&fx.0)
                        .env_remove("MANDATE_BASE_REF")
                        .env_remove("GITHUB_EVENT_NAME");
                    if let Some(explicit) = explicit {
                        command.env("MANDATE_BASE_REF", explicit);
                    }
                    if let Some(event) = event {
                        command.env("GITHUB_EVENT_NAME", event);
                    }
                    let out = command.output()?;
                    let printed = out
                        .status
                        .success()
                        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned());
                    let chosen = base_ref_in(&fx.0, explicit, event)
                        .ok()
                        .map(Option::unwrap_or_default);
                    assert_eq!(
                        printed, chosen,
                        "at {checkout} with {explicit:?} and {event:?} (None: the run fails)"
                    );
                    compared += 1;
                }
            }
        }
        assert_eq!(compared, 36);
        Ok(())
    }

    /// A stand-in for `gh` that answers `merge-approved.sh` from canned JSON beside it and appends
    /// a merge's `-f` fields to `merge.log`, so the script's decisions run with no network. It
    /// refuses a workflow-runs query that is not filtered to the pull request's head and to
    /// `pull_request` events, and answers `mergeable: UNKNOWN` while `unknown_reads` counts down.
    /// The label's events and the description's authorship come from `events.json` and
    /// `graphql.json`, and the jobs of `ci`'s latest run, the only run it answers them for, from
    /// `jobs.json`.
    const STUB_GH: &str = r##"#!/usr/bin/env bash
set -euo pipefail
dir=$(dirname "$0")
if [ "$1" = pr ]; then
  left=$(cat "$dir/unknown_reads")
  if [ "$left" -gt 0 ]; then
    echo $((left - 1)) >"$dir/unknown_reads"
    jq '.mergeable = "UNKNOWN"' "$dir/view.json"
  else
    cat "$dir/view.json"
  fi
  exit 0
fi
filter=.
path=
put=
fields=()
while [ $# -gt 0 ]; do
  case "$1" in
    --jq) filter=$2; shift 2 ;;
    -X) if [ "$2" = PUT ]; then put=1; fi; shift 2 ;;
    -f) fields+=("$2"); shift 2 ;;
    -F) shift 2 ;;
    api | --paginate) shift ;;
    *) path=$1; shift ;;
  esac
done
head=$(jq -r .headRefOid "$dir/view.json")
if [ -n "$put" ]; then
  printf '%s\n' "${fields[@]}" >>"$dir/merge.log"
  src="$dir/reply.json"
  echo '{"sha":"merged"}' >"$src"
else
  case "$path" in
    */actions/workflows/*/runs\?*)
      case "&${path#*\?}&" in *"&head_sha=$head&"*) ;; *) echo "runs not filtered to the head: $path" >&2; exit 1 ;; esac
      case "&${path#*\?}&" in *"&event=pull_request&"*) ;; *) echo "runs not filtered to pull_request: $path" >&2; exit 1 ;; esac
      ;;
  esac
  case "$path" in
    */actions/workflows/ci.yml/runs*) src="$dir/ci.json" ;;
    */actions/runs/*/jobs*)
      run=${path#*/actions/runs/}
      run=${run%%/*}
      latest=$(jq '.workflow_runs | max_by(.id) | .id' "$dir/ci.json")
      [ "$run" = "$latest" ] || { echo "jobs asked of run $run, not the latest ci run $latest" >&2; exit 1; }
      src="$dir/jobs.json"
      ;;
    */actions/workflows/web.yml/runs*) src="$dir/web.json" ;;
    */files*) src="$dir/files.json" ;;
    */issues/*/events*) src="$dir/events.json" ;;
    graphql) src="$dir/graphql.json" ;;
    *) echo "unexpected gh api $path" >&2; exit 1 ;;
  esac
fi
jq -r "$filter" "$src"
"##;

    const APPROVED_HEAD: &str = "1111111111111111111111111111111111111111";

    #[test]
    fn merge_workflow_supplies_the_authorized_coordinator_identities() -> Result<()> {
        let workflow = fs::read_to_string(repo_root()?.join(".github/workflows/merge.yml"))?;
        assert!(workflow.contains(
            "MERGE_APPROVERS: ${{ vars.MERGE_APPROVERS || format('{0} kunwar-vp cursor', \
             github.repository_owner) }}"
        ));
        Ok(())
    }

    /// A case's name, the change that makes the script refuse it, and the reason it must print.
    type Refusal<'a> = (&'a str, &'a dyn Fn(&mut MergeCase), &'a str);

    /// What GitHub would answer `merge-approved.sh` about one pull request.
    struct MergeCase {
        view: Value,
        ci: Value,
        /// The jobs of `ci`'s latest run.
        jobs: Value,
        web: Value,
        files: Vec<String>,
        unknown_reads: u32,
        /// Who applied `coordinator-approved`, oldest first; the script trusts the last.
        labeled_by: Vec<&'static str>,
        /// Who opened the pull request, and who last edited its description, if anyone did.
        author: &'static str,
        editor: Option<&'static str>,
    }

    fn workflow_runs(list: &[(u64, &str, Option<&str>)]) -> Value {
        let runs: Vec<Value> = list
            .iter()
            .map(|(id, status, conclusion)| {
                json!({ "id": id, "status": status, "conclusion": conclusion })
            })
            .collect();
        json!({ "workflow_runs": runs })
    }

    fn run_jobs(list: &[(&str, &str)]) -> Value {
        let jobs: Vec<Value> = list
            .iter()
            .map(|(name, conclusion)| json!({ "name": name, "conclusion": conclusion }))
            .collect();
        json!({ "jobs": jobs })
    }

    fn approved_body(approval: &str) -> String {
        format!(
            "<!-- CURSOR_AGENT_PR_BODY_BEGIN -->\r\n## Story\n\nKept.\n\
             Co-authored-by: Someone <someone@example.com>\n\n\
             {approval}\n\
             <!-- CURSOR_AGENT_PR_BODY_END -->\nAgent metadata, dropped."
        )
    }

    impl MergeCase {
        /// Approved at its head, mergeable, and `ci`'s latest run green after an earlier red one.
        fn approved() -> Self {
            Self {
                view: json!({
                    "number": 7,
                    "state": "OPEN",
                    "isDraft": false,
                    "baseRefName": "main",
                    "headRefOid": APPROVED_HEAD,
                    "mergeable": "MERGEABLE",
                    "labels": [{ "name": "coordinator-approved" }],
                    "title": "E1-1: a story",
                    "body": approved_body(&format!("Coordinator-approved-head: {APPROVED_HEAD}")),
                }),
                ci: workflow_runs(&[
                    (1, "completed", Some("failure")),
                    (2, "completed", Some("success")),
                ]),
                jobs: run_jobs(&[
                    ("fast", "success"),
                    ("full-checks", "success"),
                    ("mutants-plan", "success"),
                    ("mutants", "skipped"),
                    ("full", "success"),
                ]),
                web: workflow_runs(&[]),
                files: vec!["crates/c/src/lib.rs".to_owned()],
                unknown_reads: 0,
                labeled_by: vec!["owner"],
                author: "owner",
                editor: None,
            }
        }

        fn with_body(mut self, body: String) -> Self {
            self.view["body"] = json!(body);
            self
        }

        /// Runs the script against this case: its standard output, and the merge's fields when it
        /// merged.
        fn run(&self, name: &str) -> Result<(String, Option<String>)> {
            let dir =
                env::temp_dir().join(format!("mandate-xtask-merge-{name}-{}", std::process::id()));
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
            }
            fs::create_dir_all(&dir)?;
            let files: Vec<Value> = self
                .files
                .iter()
                .map(|f| json!({ "filename": f }))
                .collect();
            fs::write(dir.join("view.json"), self.view.to_string())?;
            fs::write(dir.join("ci.json"), self.ci.to_string())?;
            fs::write(dir.join("jobs.json"), self.jobs.to_string())?;
            fs::write(dir.join("web.json"), self.web.to_string())?;
            fs::write(dir.join("files.json"), Value::from(files).to_string())?;
            fs::write(dir.join("unknown_reads"), self.unknown_reads.to_string())?;
            let mut events = vec![json!({
                "event": "labeled",
                "label": { "name": "needs-review" },
                "actor": { "login": "someone-else" },
            })];
            events.extend(self.labeled_by.iter().map(|login| {
                json!({
                    "event": "labeled",
                    "label": { "name": "coordinator-approved" },
                    "actor": { "login": login },
                })
            }));
            fs::write(dir.join("events.json"), Value::from(events).to_string())?;
            let editor = self.editor.map(|login| json!({ "login": login }));
            fs::write(
                dir.join("graphql.json"),
                json!({ "data": { "repository": { "pullRequest": {
                    "author": { "login": self.author },
                    "editor": editor,
                } } } })
                .to_string(),
            )?;
            let gh = dir.join("gh");
            fs::write(&gh, STUB_GH)?;
            fs::set_permissions(&gh, fs::Permissions::from_mode(0o755))?;
            let path = format!("{}:{}", dir.display(), env::var("PATH")?);
            let out = Command::new("bash")
                .arg(repo_root()?.join(".github/scripts/merge-approved.sh"))
                .arg("7")
                .env("PATH", path)
                .env("GITHUB_REPOSITORY", "owner/repo")
                .env("GH_TOKEN", "unused")
                .env("MERGE_RETRY_S", "0")
                .env_remove("MERGE_DRY_RUN")
                .env_remove("MERGE_APPROVERS")
                .output()?;
            let stdout = String::from_utf8(out.stdout)?;
            assert!(
                out.status.success(),
                "{name}: the script decides without failing: {stdout}{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let merged = fs::read_to_string(dir.join("merge.log")).ok();
            fs::remove_dir_all(&dir)?;
            Ok((stdout, merged))
        }
    }

    #[test]
    fn the_merge_script_merges_an_approved_head_with_its_description_as_the_message() -> Result<()>
    {
        let (out, merged) = MergeCase::approved().run("approved")?;
        let merged = merged.context("an approved head with a green ci run merges")?;
        assert!(out.contains("squash-merging"), "{out}");
        for field in [
            "merge_method=squash".to_owned(),
            format!("sha={APPROVED_HEAD}"),
            "commit_title=E1-1: a story (#7)".to_owned(),
            "## Story".to_owned(),
            "Kept.".to_owned(),
        ] {
            assert!(
                merged.contains(&field),
                "the merge carries {field:?}: {merged}"
            );
        }
        assert!(
            !merged.to_lowercase().contains("co-authored-by"),
            "no trailer reaches main: {merged}"
        );
        assert!(
            !merged.contains("Agent metadata") && !merged.contains('\r'),
            "only the marked body, without carriage returns: {merged}"
        );
        Ok(())
    }

    #[test]
    fn the_merge_script_refuses_anything_short_of_an_approved_green_head() -> Result<()> {
        let stale_head = "2".repeat(40);
        let refusals: [Refusal<'_>; 25] = [
            (
                "closed",
                &|c| c.view["state"] = json!("CLOSED"),
                "is not open",
            ),
            ("draft", &|c| c.view["isDraft"] = json!(true), "is a draft"),
            (
                "base",
                &|c| c.view["baseRefName"] = json!("release"),
                "does not target main",
            ),
            (
                "label",
                &|c| c.view["labels"] = json!([]),
                "does not carry coordinator-approved",
            ),
            (
                "unnamed",
                &|c| c.view["body"] = json!("## Story\n\nNo approval line."),
                "names no Coordinator-approved-head",
            ),
            (
                "trailing",
                &|c| {
                    c.view["body"] = json!(approved_body(&format!(
                        "Coordinator-approved-head: {APPROVED_HEAD} (round 2)"
                    )))
                },
                "names no Coordinator-approved-head",
            ),
            (
                "mid-line",
                &|c| {
                    c.view["body"] = json!(approved_body(&format!(
                        "Earlier rounds: Coordinator-approved-head: {APPROVED_HEAD}"
                    )))
                },
                "names no Coordinator-approved-head",
            ),
            (
                "fenced",
                &|c| {
                    c.view["body"] = json!(approved_body(&format!(
                        "```\nCoordinator-approved-head: {APPROVED_HEAD}\n```"
                    )))
                },
                "names no Coordinator-approved-head",
            ),
            (
                "twice",
                &|c| {
                    c.view["body"] = json!(approved_body(&format!(
                        "Coordinator-approved-head: {APPROVED_HEAD}\n\
                         Coordinator-approved-head: {APPROVED_HEAD}"
                    )))
                },
                "names more than one Coordinator-approved-head",
            ),
            (
                "pushed",
                &|c| c.view["headRefOid"] = json!(stale_head),
                "was approved at 1111111111111111111111111111111111111111, but its head is 2222",
            ),
            (
                "conflict",
                &|c| c.view["mergeable"] = json!("CONFLICTING"),
                "not mergeable yet (CONFLICTING)",
            ),
            (
                "unknown",
                &|c| c.unknown_reads = 3,
                "not mergeable yet (UNKNOWN)",
            ),
            (
                "no-ci",
                &|c| c.ci = workflow_runs(&[]),
                "has ci.yml at missing",
            ),
            (
                "ci-red-last",
                &|c| {
                    c.ci = workflow_runs(&[
                        (1, "completed", Some("success")),
                        (2, "completed", Some("failure")),
                    ])
                },
                "has ci.yml at failure",
            ),
            (
                "ci-running",
                &|c| {
                    c.ci = workflow_runs(&[
                        (1, "completed", Some("success")),
                        (2, "in_progress", None),
                    ])
                },
                "has ci.yml at in_progress",
            ),
            (
                "ci-skipped-on-a-draft",
                &|c| {
                    c.jobs = run_jobs(&[
                        ("fast", "skipped"),
                        ("full-checks", "skipped"),
                        ("mutants-plan", "skipped"),
                        ("mutants", "skipped"),
                        ("full", "skipped"),
                    ])
                },
                "without fast and full both run and green (fast=skipped full=skipped)",
            ),
            (
                "ci-full-skipped",
                &|c| c.jobs = run_jobs(&[("fast", "success"), ("full", "skipped")]),
                "without fast and full both run and green (fast=success full=skipped)",
            ),
            (
                "ci-without-its-required-jobs",
                &|c| c.jobs = run_jobs(&[("mutants-plan", "success")]),
                "without fast and full both run and green (neither found)",
            ),
            (
                "web-red",
                &|c| {
                    c.files = vec![
                        "crates/c/src/lib.rs".to_owned(),
                        "web/app/page.tsx".to_owned(),
                    ];
                    c.web = workflow_runs(&[(3, "completed", Some("failure"))]);
                },
                "has web.yml at failure",
            ),
            (
                "web-red-after-a-long-file-list",
                &|c| {
                    c.files = std::iter::once(".github/workflows/web.yml".to_owned())
                        .chain((0..20_000).map(|i| format!("crates/c/src/f{i}.rs")))
                        .collect();
                    c.web = workflow_runs(&[(3, "completed", Some("failure"))]);
                },
                "has web.yml at failure",
            ),
            (
                "labelled-by-a-collaborator",
                &|c| c.labeled_by = vec!["collaborator"],
                "applied by collaborator, who is not an approver",
            ),
            (
                "relabelled-by-a-collaborator",
                &|c| c.labeled_by = vec!["owner", "collaborator"],
                "applied by collaborator, who is not an approver",
            ),
            (
                "edited-by-a-collaborator",
                &|c| c.editor = Some("collaborator"),
                "last written by collaborator, who is not an approver",
            ),
            (
                "written-by-a-collaborator",
                &|c| c.author = "collaborator",
                "last written by collaborator, who is not an approver",
            ),
            (
                "empty",
                &|c| {
                    c.view["body"] = json!(format!(
                        "Coordinator-approved-head: {APPROVED_HEAD}\n\
                         <!-- CURSOR_AGENT_PR_BODY_BEGIN -->\n<!-- CURSOR_AGENT_PR_BODY_END -->"
                    ))
                },
                "has an empty description",
            ),
        ];
        for (name, change, reason) in refusals {
            let mut case = MergeCase::approved();
            change(&mut case);
            let (out, merged) = case.run(name)?;
            assert!(
                out.contains(reason),
                "{name}: expected {reason:?} in {out:?}"
            );
            assert_eq!(merged, None, "{name}: nothing merges");
        }
        Ok(())
    }

    #[test]
    fn the_merge_script_accepts_the_approval_line_as_people_write_it() -> Result<()> {
        let upper = "ABCDEF".repeat(6) + "ABCD";
        let lower = upper.to_lowercase();
        for (name, approval, head) in [
            (
                "bullet",
                format!("- Coordinator-approved-head: {APPROVED_HEAD}"),
                APPROVED_HEAD,
            ),
            (
                "star",
                format!("* Coordinator-approved-head:  {APPROVED_HEAD}  "),
                APPROVED_HEAD,
            ),
            (
                "upper",
                format!("Coordinator-approved-head: {upper}"),
                lower.as_str(),
            ),
        ] {
            let mut case = MergeCase::approved().with_body(approved_body(&approval));
            case.view["headRefOid"] = json!(head);
            case.unknown_reads = 2;
            let (out, merged) = case.run(name)?;
            assert!(
                merged.is_some(),
                "{name}: {approval:?} approves the head once mergeability settles: {out}"
            );
        }
        Ok(())
    }

    #[test]
    fn the_merge_script_takes_an_approval_only_from_an_approver() -> Result<()> {
        let mut case = MergeCase::approved();
        case.author = "collaborator";
        case.editor = Some("owner");
        case.labeled_by = vec!["collaborator", "owner"];
        let (out, merged) = case.run("collaborators-pr-approved-by-the-owner")?;
        assert!(
            merged.is_some(),
            "a collaborator's pull request the owner labelled and approved merges: {out}"
        );
        Ok(())
    }

    #[test]
    fn the_merge_script_needs_web_only_for_web_and_keeps_a_quoted_marker() -> Result<()> {
        let mut case = MergeCase::approved().with_body(format!(
            "<!-- CURSOR_AGENT_PR_BODY_BEGIN -->\n\
             Quoted: `<!-- CURSOR_AGENT_PR_BODY_BEGIN -->` stays.\n\
             Quoted: `<!-- CURSOR_AGENT_PR_BODY_END -->` stays.\nAfter the quote.\n\
             Coordinator-approved-head: {APPROVED_HEAD}\n<!-- CURSOR_AGENT_PR_BODY_END -->"
        ));
        case.files = vec!["web/app/page.tsx".to_owned()];
        case.web = workflow_runs(&[(4, "completed", Some("success"))]);
        let (out, merged) = case.run("web-green")?;
        let merged = merged.context("a web change with ci and web green merges")?;
        assert!(out.contains("ci.yml web.yml green"), "{out}");
        assert!(
            merged.contains("Quoted: `<!-- CURSOR_AGENT_PR_BODY_BEGIN -->` stays.")
                && merged.contains("After the quote."),
            "a marker quoted inside a line neither starts nor ends the body: {merged}"
        );

        let mut case = MergeCase::approved();
        case.files = vec![
            "crates/web/src/lib.rs".to_owned(),
            "docs/web/notes.md".to_owned(),
        ];
        let (out, merged) = case.run("not-web")?;
        assert!(
            merged.is_some() && out.contains("ci.yml green"),
            "a path that only contains `web/` needs no web run: {out}"
        );
        Ok(())
    }

    /// E1-4's shellcheck oracle (DEC-329): a clean script passes and a planted `SC2086` fails
    /// naming the code, so the check judges the script it is given. This holds the oracle, not
    /// its wiring into the lint job, which
    /// `the_lint_job_fails_on_a_finding_of_either_tool_in_its_repository` holds (DEC-331).
    #[test]
    fn a_shellcheck_error_in_a_fixture_script_fails_lint() -> Result<()> {
        let dir = env::temp_dir().join(format!("mandate-xtask-shellcheck-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(&dir)?;
        fs::write(
            dir.join("clean.sh"),
            "#!/usr/bin/env bash\nset -euo pipefail\nprintf '%s\\n' \"$1\"\n",
        )?;
        shellcheck_scripts(&dir).context("a clean script passes shellcheck")?;
        fs::write(dir.join("planted.sh"), "#!/usr/bin/env bash\nrm $1\n")?;
        let err = shellcheck_scripts(&dir)
            .expect_err("the planted unquoted expansion fails lint as SC2086");
        fs::remove_dir_all(&dir).ok();
        let err = format!("{err:#}");
        assert!(
            err.contains("SC2086"),
            "the failure names the code the fixture planted: {err}"
        );
        Ok(())
    }

    /// E1-4's actionlint oracle (DEC-330): a minimal workflow passes and a workflow with an
    /// unknown key fails naming it, so the check judges the workflow it is given. This holds the
    /// oracle, not its wiring into the lint job, which
    /// `the_lint_job_fails_on_a_finding_of_either_tool_in_its_repository` holds (DEC-331).
    #[test]
    fn an_actionlint_error_in_a_fixture_workflow_fails_lint() -> Result<()> {
        let dir = env::temp_dir().join(format!("mandate-xtask-actionlint-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(&dir)?;
        fs::write(
            dir.join("clean.yml"),
            concat!(
                "on: push\n",
                "jobs:\n",
                "  check:\n",
                "    runs-on: ubuntu-24.04\n",
                "    steps:\n",
                "      - run: echo ok\n",
            ),
        )?;
        actionlint_workflows(&dir).context("a minimal workflow passes actionlint")?;
        fs::write(
            dir.join("planted.yml"),
            concat!(
                "on:\n",
                "  push:\n",
                "    jobs:\n",
                "      x:\n",
                "    badopt: 1\n",
            ),
        )?;
        let err = actionlint_workflows(&dir)
            .expect_err("the planted unknown key fails lint as a syntax error");
        fs::remove_dir_all(&dir).ok();
        let err = format!("{err:#}");
        assert!(
            err.contains("unexpected key \"badopt\""),
            "the failure names the key the fixture planted: {err}"
        );
        Ok(())
    }

    /// E1-4's wiring (DEC-331, the DEC-139 pattern): a fixture repository drives the lint job
    /// `ci lint` runs, with its workspace checks replaced by a recorder. A clean repository passes
    /// and reaches the workspace checks; a planted `SC2086` in its `.github/scripts/` fails the
    /// job naming the code; with the script clean again, an unknown key in its
    /// `.github/workflows/` fails the job naming the key. Dropping either tool's result from the
    /// job, or the workspace checks, fails this test.
    #[test]
    fn the_lint_job_fails_on_a_finding_of_either_tool_in_its_repository() -> Result<()> {
        let root = env::temp_dir().join(format!("mandate-xtask-lint-job-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        fixture_workspace(&root)?;
        let scripts = root.join(".github/scripts");
        let workflows = root.join(".github/workflows");
        let deploy = root.join("deploy");
        fs::create_dir_all(&scripts)?;
        fs::create_dir_all(&workflows)?;
        fs::create_dir_all(&deploy)?;
        for dir in [&scripts, &deploy] {
            fs::write(
                dir.join("clean.sh"),
                "#!/usr/bin/env bash\nset -euo pipefail\nprintf '%s\\n' \"$1\"\n",
            )?;
        }
        fs::write(
            workflows.join("clean.yml"),
            concat!(
                "on: push\n",
                "jobs:\n",
                "  check:\n",
                "    runs-on: ubuntu-24.04\n",
                "    steps:\n",
                "      - run: echo ok\n",
            ),
        )?;
        let reached = Cell::new(false);
        lint(&root, || {
            reached.set(true);
            Ok(())
        })
        .context("a clean fixture repository passes the lint job")?;
        assert!(
            reached.get(),
            "the lint job runs its workspace checks after both tools pass"
        );

        fs::write(scripts.join("planted.sh"), "#!/usr/bin/env bash\nrm $1\n")?;
        let shellcheck = lint(&root, || Ok(()))
            .map_err(|err| format!("{err:#}"))
            .err();
        fs::remove_file(scripts.join("planted.sh"))?;

        fs::write(deploy.join("planted.sh"), "#!/usr/bin/env bash\nrm $1\n")?;
        let deploy_shellcheck = lint(&root, || Ok(()))
            .map_err(|err| format!("{err:#}"))
            .err();
        fs::remove_file(deploy.join("planted.sh"))?;

        fs::write(
            workflows.join("planted.yml"),
            concat!(
                "on:\n",
                "  push:\n",
                "    jobs:\n",
                "      x:\n",
                "    badopt: 1\n",
            ),
        )?;
        let actionlint = lint(&root, || Ok(()))
            .map_err(|err| format!("{err:#}"))
            .err();
        fs::remove_dir_all(&root).ok();

        assert!(
            shellcheck
                .as_deref()
                .is_some_and(|err| err.contains("SC2086")),
            "a planted SC2086 fails the lint job naming the code, got {shellcheck:?}"
        );
        assert!(
            deploy_shellcheck
                .as_deref()
                .is_some_and(|err| err.contains("SC2086")),
            "a planted SC2086 under deploy/ fails the lint job too, got {deploy_shellcheck:?}"
        );
        assert!(
            actionlint
                .as_deref()
                .is_some_and(|err| err.contains("unexpected key \"badopt\"")),
            "a planted unknown workflow key fails the lint job naming it, got {actionlint:?}"
        );
        Ok(())
    }

    /// The runner for the live-feature tests, marked `live_feature` in its policy.
    const RUNNER: &str = "the-runner";

    /// A policy with `the-runner` at layer 9 marked `live_feature`, and `a-lib` and `a-tool` at
    /// layers 5 and 6, none marked.
    fn live_policy() -> Layers {
        let mut policy = policy(&[(RUNNER, 9), ("a-lib", 5), ("a-tool", 6)], &[], &[]);
        if let Some(runner) = policy.crates.get_mut(RUNNER) {
            runner.live_feature = true;
        }
        policy
    }

    /// `pkg` with the `[features]` in `features`.
    fn with_features(mut pkg: Package, features: &[(&str, &[&str])]) -> Package {
        pkg.features = features
            .iter()
            .map(|(name, on)| {
                (
                    (*name).to_owned(),
                    on.iter().map(|f| (*f).to_owned()).collect(),
                )
            })
            .collect();
        pkg
    }

    /// The three crates of [`live_policy`], with no features and no dependencies.
    fn live_workspace() -> Vec<Package> {
        vec![
            member(RUNNER, &[]),
            member("a-lib", &[]),
            member("a-tool", &[]),
        ]
    }

    fn ci_file(path: &str, text: &str) -> CiFile {
        CiFile {
            path: path.to_owned(),
            text: text.to_owned(),
        }
    }

    /// Only the marked runner may declare a `live` feature; any other crate that does is named
    /// (ES-23 as DEC-529 item 3 narrows it).
    #[test]
    fn only_the_marked_runner_may_declare_a_live_feature() -> Result<()> {
        let marked = live_policy();
        let mut crates = live_workspace();
        crates[0] = with_features(member(RUNNER, &[]), &[("live", &[])]);
        assert_eq!(
            live_feature_problems(&marked, &workspace(crates), &[])?,
            Vec::<String>::new(),
            "the marked runner's own `live` feature is allowed"
        );
        let mut crates = live_workspace();
        crates[1] = with_features(member("a-lib", &[]), &[("live", &[])]);
        let problems = live_feature_problems(&marked, &workspace(crates), &[])?;
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("`a-lib`"), "{problems:?}");
        let unmarked = policy(&[(RUNNER, 9), ("a-lib", 5), ("a-tool", 6)], &[], &[]);
        let mut crates = live_workspace();
        crates[0] = with_features(member(RUNNER, &[]), &[("live", &[])]);
        let problems = live_feature_problems(&unmarked, &workspace(crates), &[])?;
        assert_eq!(
            problems.len(),
            1,
            "an unmarked runner is any crate: {problems:?}"
        );
        assert!(problems[0].contains(&format!("`{RUNNER}`")), "{problems:?}");
        Ok(())
    }

    /// At most one crate is marked, and a marked crate must be a workspace member.
    #[test]
    fn at_most_one_member_crate_is_marked() -> Result<()> {
        let mut two = live_policy();
        if let Some(lib) = two.crates.get_mut("a-lib") {
            lib.live_feature = true;
        }
        let problems = live_feature_problems(&two, &workspace(live_workspace()), &[])?;
        assert_eq!(problems.len(), 1, "two marked crates: {problems:?}");
        assert!(
            problems[0].contains(&format!("`{RUNNER}`")) && problems[0].contains("`a-lib`"),
            "both are named: {problems:?}"
        );
        let crates = vec![member("a-lib", &[]), member("a-tool", &[])];
        let problems = live_feature_problems(&live_policy(), &workspace(crates), &[])?;
        assert_eq!(
            problems.len(),
            1,
            "a marked crate that is no member: {problems:?}"
        );
        assert!(problems[0].contains(&format!("`{RUNNER}`")), "{problems:?}");
        Ok(())
    }

    /// Nothing else turns `live` on: no other feature of the runner (`default` included), no
    /// feature of another crate naming `<runner>/live` or `<runner>?/live`, and no dependency
    /// on the runner listing `live` among its features, whatever the dependency's kind.
    #[test]
    fn nothing_but_an_explicit_flag_turns_live_on() -> Result<()> {
        let policy = live_policy();
        let runner = |features: &[(&str, &[&str])]| with_features(member(RUNNER, &[]), features);
        let enabling_live: Vec<Vec<Package>> = vec![
            vec![
                runner(&[("live", &[]), ("default", &["live"])]),
                member("a-lib", &[]),
            ],
            vec![
                runner(&[("live", &[]), ("all", &["live"])]),
                member("a-lib", &[]),
            ],
            vec![
                runner(&[("live", &[])]),
                with_features(member("a-lib", &[]), &[("go", &["the-runner/live"])]),
            ],
            vec![
                runner(&[("live", &[])]),
                with_features(member("a-lib", &[]), &[("go", &["the-runner?/live"])]),
            ],
        ];
        for crates in enabling_live {
            let problems = live_feature_problems(&policy, &workspace(crates), &[])?;
            assert_eq!(problems.len(), 1, "{problems:?}");
        }
        for kind in [None, Some("dev"), Some("build")] {
            let mut dependent = member("a-tool", &[(RUNNER, kind)]);
            dependent.dependencies[0].features = vec!["live".to_owned()];
            let crates = vec![runner(&[("live", &[])]), dependent];
            let problems = live_feature_problems(&policy, &workspace(crates), &[])?;
            assert_eq!(problems.len(), 1, "a {kind:?} dependency: {problems:?}");
            assert!(problems[0].contains("`a-tool`"), "{problems:?}");
        }
        let mut quiet = member("a-tool", &[(RUNNER, Some("dev"))]);
        quiet.dependencies[0].features = vec!["other".to_owned()];
        let crates = vec![runner(&[("live", &[]), ("other", &[])]), quiet];
        assert_eq!(
            live_feature_problems(&policy, &workspace(crates), &[])?,
            Vec::<String>::new(),
            "a dependency on the runner without `live` is allowed"
        );
        Ok(())
    }

    /// CI may pass `live` only in one `cargo check` of the marked runner, so the feature
    /// compiles and nothing live is built, tested or run; `--all-features` is never allowed. Each
    /// refusal names the file and its line.
    #[test]
    fn ci_only_compiles_the_runner_with_live() -> Result<()> {
        let policy = live_policy();
        let meta = || workspace(live_workspace());
        let allowed = [
            "      - run: cargo check -p the-runner --features live",
            "      - run: cargo check --locked -p the-runner --features=live",
            "cargo check -p the-runner -F live",
        ];
        for line in allowed {
            let files = [ci_file(".github/workflows/ci.yml", line)];
            assert_eq!(
                live_feature_problems(&policy, &meta(), &files)?,
                Vec::<String>::new(),
                "{line}"
            );
        }
        let refused = [
            "      - run: cargo build -p the-runner --features live",
            "      - run: cargo test -p the-runner --features live",
            "      - run: cargo nextest run -p the-runner --features live",
            "      - run: cargo run -p the-runner --features a,live",
            "      - run: cargo clippy -p the-runner -F live",
            "      - run: cargo check -p a-lib --features live",
            "      - run: cargo check --workspace --features live",
            "      - run: cargo check --all-features",
            "      - run: cargo test --workspace --all-features",
            "cargo install --path crates/the-runner --features \"live\"",
        ];
        for line in refused {
            let files = [ci_file(
                ".github/scripts/build.sh",
                &format!("set -e\n{line}\n"),
            )];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 1, "{line}: {problems:?}");
            assert!(
                problems[0].contains(".github/scripts/build.sh") && problems[0].contains(":2"),
                "names the file and line: {problems:?}"
            );
        }
        let twice = [
            ci_file(".github/workflows/a.yml", allowed[0]),
            ci_file(".github/workflows/b.yml", allowed[0]),
        ];
        let problems = live_feature_problems(&policy, &meta(), &twice)?;
        assert_eq!(
            problems.len(),
            1,
            "at most one compile-only job: {problems:?}"
        );
        let harmless = [ci_file(
            ".github/workflows/ci.yml",
            "      - run: cargo xtask ci fast\n      # a live host is never compiled here\n",
        )];
        assert_eq!(
            live_feature_problems(&policy, &meta(), &harmless)?,
            Vec::<String>::new(),
            "the word in prose or other commands is not a feature flag"
        );
        Ok(())
    }

    /// The repository as it stands: the policy, the workspace and every file the check reads
    /// pass, with no crate marked and no `live` feature anywhere. Main's files hold the token
    /// `live` only in comments, so the live-token backstop allows them (#738 review, fourth
    /// round).
    #[test]
    fn the_repository_has_no_live_build() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let policy: Layers = toml::from_str(&fs::read_to_string(root.join("xtask/layers.toml"))?)?;
        let problems = live_feature_problems(&policy, &metadata_in(&root)?, &ci_files(&root)?)?;
        assert_eq!(problems, Vec::<String>::new());
        Ok(())
    }

    /// Each way a line can build or run `live` while looking like the compile-only form, or
    /// without naming `live` the plain way, is refused once, naming the file and line (#738
    /// review, finding 1): a second command after `&&`, `||`, `;` or `|` is judged on its own; a
    /// `<crate>/live` or `<crate>?/live` feature; a list split on spaces; `-Flive`; a feature list
    /// the shell expands (`$` or a backtick), which cannot be read; and `--cfg feature="live"`,
    /// which sets the feature without `--features`. A flag right after a list, a lone opening
    /// quote, `--all` and `--workspace` are read too, and a quoted list ends at its closing quote,
    /// so a flag inside it is not read as a feature (#738's mutants).
    ///
    /// A command is read whole, not a physical line at a time (#738 review, second round): a
    /// backslash continuation, or a YAML folded `>` scalar, that carries `--features live` on a
    /// later line is refused; so is `--cfg feature = "live"` with spaces or tabs around the
    /// `=`, in a command, `RUSTFLAGS` or `rustflags`; and so is a cargo command that takes
    /// arguments from a variable, `${...}`, `$(...)` or a backtick (`cargo build $FLAGS`), which
    /// cannot be read. A cargo command in a CI file is
    /// read word by word. Only the separators `|`, `;`, `&&`, `||` and a single `&` end its
    /// words, and a cargo command after one is judged on its own. A redirection (`>`, `>>`,
    /// `2>`, `<`, `2>&1`, `>/dev/null`, `&>`) does not end them: the operator and its one target
    /// word are skipped and the words after it are still checked. A `$` or backtick is allowed
    /// only inside a redirect target word, so `cargo xtask ci mutants --plan >> "$GITHUB_OUTPUT"`
    /// and `cargo build 2> "$LOG" -p x` are allowed, and any other word with one is refused,
    /// because a variable can carry a `--features` or `--cfg` flag past a line-based scan: even
    /// `--target $TARGET`, `--target "$TARGET"`, `--target ${TARGET}` and `> out $FLAGS` are
    /// refused (the coordinator's rulings under DEC-176: they only refuse more).
    ///
    /// The forms #738 handles, pinned so none can be dropped (#738 review, third round): every
    /// redirection form skips just its one target (`>|`, `1>`, `2>>`, `&>>`, `>&`, `<<<`, `<>`,
    /// `>out`, `2>&-`, `>&2`, and a here-doc's `<<EOF`), and a here-doc's body lines are read as
    /// commands; a command after a newline in a `run: |` block, `|&`, `(`, `{`, `!`, `if`, `then`,
    /// `do` or `else` is judged on its own; a cargo command wrapped in `env`, `$(...)`, a backtick
    /// or `bash -c "..."` is read, and one `xargs` feeds cannot be; every `-p` is read; `cargo` is
    /// also named by its path; a cargo configuration array is read item by item; and `deploy/`'s
    /// scripts are checked like CI's. A wrapper over a cargo command without `live`, and a
    /// here-doc without cargo, stay allowed.
    ///
    /// The live-token backstop (#738 review, fourth and fifth rounds, the coordinator's rulings
    /// under DEC-176): beside the word scan, every word of every file the check reads that is not
    /// in a comment, a YAML scalar value included, is refused when its item list holds the token
    /// `live`. The word is read lexed, after its quotes and backslashes are removed (`l\ive`,
    /// `li"ve"`), and a substitution's text is read by the same backstop. Items split on `,`,
    /// `=`, `/`, `?`, whitespace and quotes, and match `live` exactly but in any case, so
    /// `liveness` is another item and `the-runner/live` and `the-runner?/live` hold it. In an
    /// item of the form `-<letters>F<rest>`, a short-flag cluster such as `-qFlive`, the part
    /// after the first `F` is read as an item list too. The one exemption is per word and per
    /// command, never per line: only the feature list of the one allowed compile-only command is
    /// exempt, so another command on its line (`&& make FEATURES=live`) is still read.
    ///
    /// A command whose command word holds a `$` or a backtick (`$c`, `"${CARGO:-cargo}"`,
    /// `$(which cargo)`) may be cargo, so its other words that hold a `$` or a backtick cannot be
    /// read and are refused (`--features $A$B`, `$'\x6cive'`, `$(echo li)ve`); so are the lines
    /// of a here-doc fed to `sh`. This closes what a cargo-word scan cannot see: `-F=live`, a
    /// cluster, a command named through a variable, a `cargo-<tool>` binary, a `with:` input,
    /// `make FEATURES=live`, `LIVE`, and an assignment such as `FLAGS="--features live"`, which is
    /// refused on its own line beside the command that expands it. A quoted list holding
    /// `--features=live` is refused with it. A line is refused once, however many of its words
    /// hold the token. The pins that need no `cargo` word are listed apart from those with one.
    ///
    /// The cargo-word rule closes what a known non-executing command can still run (`awk`'s
    /// `system` or `getline`, `sed`'s `e` flag, a `git -c alias.x=!…`, `tee >(sh)`, a script
    /// written and then run, a command held in a scalar; the coordinator's tenth-round ruling
    /// under DEC-176). In every position, whatever the command, a word, quoted or not, holding
    /// the token `cargo` (a run of letters, digits, `_`, `-` and `.` that is `cargo`, so
    /// `!cargo` holds it and `.cargo` and `cargo-nextest` do not) is refused when it also holds
    /// a `$`, a backtick or a `%`
    /// format directive; when it holds `--all-features`; or when it holds `--features` or `-F`
    /// whose value in the same word is not a complete literal of `[a-z0-9_,-]` without the
    /// token `live` (cut off by a closing quote, empty, or followed by an expansion). A script
    /// fetched or written at run time (`curl … | sh`, a generated file) cannot be read by a
    /// static scan; the cargo-word rule is what leaves such a script no way to carry a computed
    /// feature, and the live-token backstop no way to carry `live` itself.
    ///
    /// The final round (the coordinator's eleventh-round rulings under DEC-176). The value rule
    /// applies to every feature flag, with or
    /// without `cargo`: `--features`, `--features=`, `FEATURES=` and `--all-features` are feature
    /// flags in any word, and `-F` only in a command whose command word is `cargo` or may be
    /// cargo (it holds `$`, a backtick or `${{`, or is a whole-array expansion), or in a re-read
    /// word holding the token `cargo`, so `awk -F:` and `gh api -F owner="$o"` are no feature
    /// flags. A YAML `with:` value has no command word, so it may be cargo: `-F` and a short-flag
    /// cluster holding `F` (`-qF…`) are feature flags there, while `run:` values and scripts keep
    /// the rule above. Outside the one compile-only form, `--all-features` is refused, and so is
    /// a value that is not a complete literal of `[a-z0-9_,-]` without the token `live`, which
    /// any value holding `$`, a backtick or `${{` is not. Out of reach: Rust sources (`xtask/`,
    /// `build.rs`), which code review and the layers check cover, and scripts that exist only at
    /// run time. Where a word is read as a command is pinned by
    /// [`commands_are_read_fail_closed_where_a_word_may_execute`], arrays by
    /// [`array_expansions_are_read_through_their_definitions`] (X1 tests correction 5).
    ///
    /// The backstop's match ignores case, so a bare `LIVE` or `Live` word is refused in a command
    /// without `cargo`, and an empty feature value (`--features ""`, `--features=` with nothing
    /// after it) is no complete literal, so it is refused (X1 tests correction 6, #923 review).
    ///
    /// A command is compile-only only when its subcommand, the word right after its cargo word
    /// and before any `--`, is `check` (X1 tests correction 8, #923 review): a `cargo check` or a
    /// `check` after `--`, which go to the program `run` or `test` builds, and a `check` that is
    /// the value of an option before the subcommand (`cargo --config check run`) leave the command
    /// a build, so its `live` is refused, once, at its line. The compile-only form CI runs stays
    /// allowed.
    #[test]
    fn the_backstop_and_the_feature_value_rules_refuse_ci_bypasses() -> Result<()> {
        let policy = live_policy();
        let meta = || workspace(live_workspace());
        let allowed = [
            "cargo check -p the-runner -Flive",
            "cargo check -p the-runner --features the-runner/live",
            "cargo check -p the-runner --features 'other live'",
            "cargo check -p the-runner --features \"other\" --target x86_64-unknown-linux-gnu",
            "            cargo xtask ci mutants --plan >> \"$GITHUB_OUTPUT\"",
            "cargo build -p x 2> \"$LOG\"",
            "cargo build -p a-lib < \"$IN\" | tee \"$LOG\"",
            "cargo build 2> \"$LOG\" -p x",
            "env RUST_LOG=info cargo test -p x",
            "bash -c \"cargo test -p x\"",
            "cargo build --features paper,sim",
            "echo \"cargo test -p x\"",
            "      - run: cargo check -p the-runner --features live",
            "      - run: cargo check --locked -p the-runner --features=live",
        ];
        for line in allowed {
            let files = [ci_file(".github/workflows/ci.yml", line)];
            assert_eq!(
                live_feature_problems(&policy, &meta(), &files)?,
                Vec::<String>::new(),
                "{line}"
            );
        }
        let refused = [
            "cargo check -p the-runner --features live && cargo run -p the-runner --features live",
            "cargo check -p the-runner --features live || cargo build -p the-runner -F live",
            "cargo check -p the-runner --features live; cargo test -p the-runner --features live",
            "cargo check -p the-runner --features live | cargo run -p the-runner --features=live",
            "cargo run -p a-lib --features the-runner/live",
            "cargo check -p a-lib --features the-runner?/live",
            "cargo build -p the-runner --features \"other live\"",
            "cargo build -p the-runner -Flive",
            "cargo check -p the-runner --features $FEATURES",
            "cargo check -p the-runner --features `cat features.txt`",
            "RUSTFLAGS=\"--cfg feature=\\\"live\\\"\" cargo build -p the-runner",
            "cargo rustc -p the-runner -- --cfg 'feature=\"live\"'",
            "rustflags = [\"--cfg\", 'feature=\"live\"']",
            "ship = \"run -p the-runner --features live\"",
            "cargo build -p the-runner --features other -F live",
            "cargo build -p the-runner --features \" live\"",
            "cargo check -p the-runner --all --features live",
            "cargo check -p the-runner --workspace --features live",
            "ship = [\"run\", \"-p\", \"the-runner\", \"--features\", \"live\"]",
        ];
        for line in refused {
            let files = [ci_file(".cargo/config.toml", &format!("[alias]\n{line}\n"))];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 1, "{line}: {problems:?}");
            assert!(
                problems[0].contains(".cargo/config.toml:2"),
                "names the file and line: {problems:?}"
            );
        }
        let refused_across_lines = [
            (
                ".github/scripts/build.sh",
                "set -e\ncargo build --release \\\n  --features live\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncargo build -p the-runner \\\n  --features \\\n  live\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncargo run -p the-runner \\\n  -F live\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - run: |\n      cargo build --release \\\n        --features live\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - run: >\n      cargo build --release\n      --features live\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - run: >-\n      cargo test -p the-runner\n      --features other,live\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - run: cargo build -p the-runner\n      --features live\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncargo build -p the-runner $EXTRA_ARGS\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncargo build -p the-runner \"$@\"\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncargo build -p the-runner `cat flags.txt`\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncargo build -p the-runner $(cat flags.txt)\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncargo build -p x > out \\\n  $FLAGS\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncargo check -p the-runner --target \"$TARGET\"\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncargo check -p the-runner --target ${TARGET}\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncargo check -p the-runner --features \"other\" --target $TARGET\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncargo build -p x <<EOF $F\nhello\nEOF\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncat <<EOF\ncargo build --features live\nEOF\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - run: |\n      cargo check -p the-runner --features live\n      cargo build $F\n",
            ),
            ("deploy/run.sh", "set -e\ncargo build --features live\n"),
        ];
        for (path, text) in refused_across_lines {
            let files = [ci_file(path, text)];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 1, "{text}: {problems:?}");
            assert!(problems[0].contains(path), "names the file: {problems:?}");
        }
        let refused_by_words_before_an_operator = [
            "cargo build $FLAGS >> out",
            "cargo build --target $T | tee log",
            "cargo build -p a-lib $FLAGS 2> \"$LOG\"",
            "cargo build -p a-lib --target \"$T\" < in",
            "cargo build ; cargo build --features live",
            "cargo build -p a-lib > \"$OUT\" ; cargo run -p the-runner --features live",
            "cargo check -p the-runner --features live && cargo build -p the-runner $FLAGS",
            "cargo check -p the-runner --features live ; cargo build -p the-runner $FLAGS",
            "cargo build -p a-lib || cargo test -p the-runner --features live",
            "cargo build -p a-lib >> \"$OUT\" && cargo build -p the-runner --features live",
            "cargo build -p a-lib > out $FLAGS",
            "cargo build 2>&1 $FLAGS",
            "cargo build < in --target \"$T\"",
            "cargo build >/dev/null $(cat f)",
            "cargo build & cargo build -p the-runner --features live",
            "cargo build -p a-lib &> out $FLAGS",
            "cargo build -p x >| out $F",
            "cargo build -p x 1> out $F",
            "cargo build -p x 2>> out $F",
            "cargo build -p x &>> out $F",
            "cargo build -p x >& out $F",
            "cargo build -p x <<< in $F",
            "cargo build -p x <> f $F",
            "cargo build -p x >out $F",
            "cargo build -p x 2>&- $F",
            "cargo build -p x >&2 $F",
            "cargo build -p x > $X --features live",
            "true |& cargo build $F",
            "( cargo build $F )",
            "{ cargo build $F; }",
            "! cargo build $F",
            "if cargo build $F; then",
            "if true; then cargo build $F; fi",
            "for i in 1; do cargo build $F; done",
            "if true; then :; else cargo build $F; fi",
            "env VAR=1 cargo build $F",
            "xargs cargo build",
            "echo $(cargo build --features live)",
            "echo `cargo build --features live`",
            "bash -c \"cargo build -p x $F\"",
            "cargo build -p a -p b --features live",
            "cargo check -p the-runner -p a-lib --features live",
            "/usr/bin/cargo build --features live",
            "cargo build -p a-lib --features \"other --features=live\"",
            "cargo run -F=live",
            "cargo run -qFlive",
            "cargo run -vFlive",
            "cargo build -p the-runner --features LIVE",
            "cargo build --features \"\"",
            "cargo build --features=",
            "cargo check -p the-runner --features live && make build FEATURES=live",
            "cargo check -p the-runner --features live; c=cargo; $c run -Flive",
        ];
        let refused_with_no_cargo_word = [
            "cargo-nextest nextest run --features live",
            "cargo-mutants mutants --features live",
            "make build FEATURES=live",
            "FEATURES=\"a,live\"",
            "make FEATURES=the-runner/live",
            "make FLAGS=-qFlive",
            "l\\ive",
            "--features=li\"ve\"",
            "--features $(echo live)",
            "echo LIVE",
            "echo Live",
        ];
        let refused_through_a_command_word_that_may_be_cargo = [
            "c=cargo; $c build --features live",
            "\"${CARGO:-cargo}\" build --features live",
            "$(which cargo) build --features live",
            "c=cargo; $c build --features the-runner?/live",
            "$c build -qFthe-runner/live",
            "c=cargo; A=li; B=ve; $c build --features $A$B",
            "$c build --features $'\\x6cive'",
            "$c build --features $(echo li)ve",
        ];
        let refused_by_the_cargo_word_rule = [
            "awk 'BEGIN{system(\"cargo build --features \" a b)}' a=li b=ve",
            "awk 'BEGIN{\"cargo build --features \" a b | getline}' a=li b=ve",
            "sed -e \"s/x/cargo build --features $A$B/e\"",
            "git -c \"alias.b=!cargo build --features $A$B\" b",
            "echo \"cargo build --features $A$B\" | tee >(sh)",
            "printf 'cargo build --features %s%s\\n' $A $B > x.sh; sh x.sh",
            "echo \"cargo build --features $A$B\" > x.sh; bash x.sh",
            "cmd=\"cargo build --features $A$B\"; $cmd",
            "read -r cmd <<< \"cargo build --features $A$B\"; $cmd",
        ];
        let refused_where_check_is_not_the_subcommand = [
            "cargo run -p the-runner --features live -- cargo check",
            "cargo test -p the-runner --features live -- cargo check",
            "cargo build -p the-runner --features live -- check",
            "cargo +nightly run -p the-runner --features live -- cargo check",
            "cargo --locked run -p the-runner --features live -- check",
            "cargo --locked run -p the-runner --features live -- cargo check",
            "cargo run --features live -- cargo check -p the-runner",
            "/usr/bin/cargo run -p the-runner --features live -- /usr/bin/cargo check",
            "cargo --config check run -p the-runner --features live",
        ];
        let refused_at_line_two = refused_with_no_cargo_word
            .iter()
            .chain(&refused_through_a_command_word_that_may_be_cargo)
            .chain(&refused_by_the_cargo_word_rule)
            .chain(&refused_where_check_is_not_the_subcommand);
        for line in refused_at_line_two {
            let files = [ci_file(
                ".github/scripts/build.sh",
                &format!("set -e\n{line}\n"),
            )];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 1, "{line}: {problems:?}");
            assert!(
                problems[0].contains(".github/scripts/build.sh:2"),
                "names the file and line: {problems:?}"
            );
        }
        for line in refused_by_words_before_an_operator {
            let files = [ci_file(
                ".github/scripts/build.sh",
                &format!("set -e\n{line}\n"),
            )];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 1, "{line}: {problems:?}");
            assert!(
                problems[0].contains(".github/scripts/build.sh:2"),
                "names the file and line: {problems:?}"
            );
        }
        let refused_spaced_cfg = [
            "cargo rustc -p the-runner -- --cfg 'feature = \"live\"'",
            "cargo rustc -p the-runner -- --cfg 'feature  =  \"live\"'",
            "cargo rustc -p the-runner -- --cfg \"feature =\\\"live\\\"\"",
            "RUSTFLAGS=\"--cfg feature = \\\"live\\\"\" cargo build -p the-runner",
            "RUSTFLAGS='--cfg feature =\"live\"' cargo build -p the-runner",
            "rustflags = [\"--cfg\", 'feature = \"live\"']",
            "rustflags = [\"--cfg\", \"feature = \\\"live\\\"\"]",
            "rustflags = [\"--cfg\", 'feature\t=\t\"live\"']",
        ];
        let allowed_in_a_script = [
            (
                ".github/scripts/build.sh",
                "set -e\ncat <<EOF\nno build here\nEOF\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\n# the live lane supplies its binary\ncargo build -p a-lib\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  # the live lane supplies its binary\n  - run: cargo build -p a-lib\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\ncurl -fsS http://localhost/liveness\ncargo test -p a-lib liveness\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - name: liveness\n    run: ./probe.sh --liveness\n",
            ),
            (".github/scripts/build.sh", "set -e\nawk -F: '{print $1}'\n"),
            (
                ".github/scripts/build.sh",
                "set -e\ngh api -F owner=\"$o\"\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - uses: an/action@v1\n    with:\n      key: x-${{ runner.os }}\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - uses: an/action@v1\n    with:\n      key: x-${{ runner.os }}-${{ steps.v.outputs.version }}\n",
            ),
        ];
        for (path, text) in allowed_in_a_script {
            let files = [ci_file(path, text)];
            assert_eq!(
                live_feature_problems(&policy, &meta(), &files)?,
                Vec::<String>::new(),
                "{text}"
            );
        }
        let refused_inputs_with_no_cargo_word = [
            (
                ".github/workflows/ci.yml",
                "steps:\n  - uses: an/action@v1\n    with:\n      args: --features live\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - uses: an/action@v1\n    with:\n      command: build --features live\n",
            ),
            (
                ".github/actions/build/action.yml",
                "runs:\n  using: composite\n  steps:\n    - uses: an/action@v1\n      with:\n        args: --features live\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - uses: an/action@v1\n    with:\n      args: --features the-runner/live\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - uses: an/action@v1\n    with:\n      args: -qFlive\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - uses: an/action@v1\n    with:\n      args: --features ${{ matrix.a }}${{ matrix.b }}\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - uses: an/action@v1\n    with:\n      args: build -F ${{ matrix.a }}${{ matrix.b }}\n",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - uses: an/action@v1\n    with:\n      args: -qF${{ matrix.a }}${{ matrix.b }}\n",
            ),
        ];
        for (path, text) in refused_inputs_with_no_cargo_word {
            let files = [ci_file(path, text)];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 1, "{text}: {problems:?}");
            assert!(problems[0].contains(path), "names the file: {problems:?}");
        }
        let refused_on_two_lines = [
            (
                ".github/scripts/build.sh",
                "set -e\nFLAGS=\"--features live\"\ncargo build $FLAGS\n",
                [":2", ":3"],
            ),
            (
                ".github/scripts/build.sh",
                "set -e\nFLAGS='-F live'\ncargo build -p the-runner ${FLAGS}\n",
                [":2", ":3"],
            ),
            (
                ".github/workflows/ci.yml",
                "env:\n  FLAGS: --features live\nsteps:\n  - run: cargo build $FLAGS\n",
                [":2", ":4"],
            ),
            (
                ".github/scripts/build.sh",
                "set -e\nTARGET=\"x86_64-unknown-linux-gnu --features live\"\ncargo check -p the-runner --target $TARGET\n",
                [":2", ":3"],
            ),
        ];
        for (path, text, lines) in refused_on_two_lines {
            let files = [ci_file(path, text)];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 2, "{text}: {problems:?}");
            for (problem, line) in problems.iter().zip(lines) {
                assert!(
                    problem.contains(&format!("{path}{line}")),
                    "the assignment and the expansion are each refused: {problems:?}"
                );
            }
        }
        for line in refused_spaced_cfg {
            let files = [ci_file(".cargo/config.toml", &format!("[build]\n{line}\n"))];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 1, "{line}: {problems:?}");
            assert!(
                problems[0].contains(".cargo/config.toml:2"),
                "names the file and line: {problems:?}"
            );
        }
        Ok(())
    }

    /// Where a word is read as a command fails closed (the coordinator's seventh-round ruling
    /// under DEC-176), split out of the bypass test so X1's step 3b un-ignores it (X1 tests
    /// correction 5).
    ///
    /// A known non-executing command is one of `echo`, `printf`, `cat`, `jq`, `awk`, `sed`, `grep`,
    /// `tr`, `cut`, `sort`, `uniq`, `head`, `tail`, `tee`, `wc`, `test`, `[`, `true`, `false`,
    /// `read`, `basename`, `dirname`, `date`, `mkdir`, `rm`, `cp`, `mv`, `ls`, `chmod`, `curl`,
    /// `git`, and `gh api` alone, which main's `merge-approved.sh` needs for its multi-line GraphQL
    /// query; every other command executes, any other `gh` subcommand (`gh alias set --shell`),
    /// unknown ones, `source`, `.`, `exec`, `watch`, `parallel`, every shell and every wrapper
    /// included. A quoted word holding a space, `cargo` or a `$` is re-read as a command line
    /// unless its command is known non-executing, every later stage of its pipeline is too, and it
    /// is not inside a `$( … )` `<( … )` or `>( … )` whose consumer executes (`echo "…" | sh`,
    /// `source <(echo "…")`, `tee >(sh)`); a bare assignment captures its `$( … )` and does not
    /// execute it. A here-doc's lines and a here-string's (`<<<`) target are read as commands
    /// unless their command is known non-executing, so `cat <<EOF` holding `$F` is allowed.
    /// `$(( … ))` is arithmetic and `${#x[@]}` a length, neither read as a command; `if`, `then`, `else`,
    /// `elif`, `do`, `while`, `until`, `!` and `time` are skipped when finding the command word,
    /// and the word after one is still read (`if cargo build …; then`). The lines inside a
    /// single-quoted string that spans lines are not read as commands only when its command is
    /// known non-executing and nothing pipes it onward. A word holding `${{ … }}` in command
    /// position is possibly cargo inside a `run:` value only (`run: ${{ inputs.cmd }} build $F`),
    /// and not in a job name, an `env:` value, a `with:` input or a cache key. The live-token
    /// backstop still reads every word of every line. An `awk` word holding `system(` or `getline`,
    /// a `sed` word with an `e` command or an `/e` flag, and a `git` with `-c alias.*=!` or a `!`
    /// alias make that command executing, so its words are re-read; an executing `awk` or `sed`
    /// program builds a command no static reading can follow, so it is refused outright (round
    /// eleven).
    ///
    /// The pins of `refused_only_by_reading_fail_closed` hold no `cargo` token, no `live` and no
    /// feature flag, so only this reading refuses them, never the backstop or the cargo-word rule
    /// (X1 tests correction 7, step 3b's planted bugs).
    #[test]
    #[ignore = "pending E7-26"]
    fn commands_are_read_fail_closed_where_a_word_may_execute() -> Result<()> {
        let policy = live_policy();
        let meta = || workspace(live_workspace());
        let refused_through_an_executing_command_at_line_two = [
            "echo \"cargo build --features $A$B\" | sh",
            "printf '%s' \"c=cargo; $c build --features $A$B\" | bash",
            "source <(echo \"cargo build --features $A$B\")",
            ". <(printf '%s\\n' \"cargo build --features $A$B\")",
            "sh <<< \"cargo build --features $A$B\"",
            "watch \"cargo build --features $A$B\"",
            "parallel \"cargo build --features $A$B\" ::: x",
            "frobnicate \"cargo build $F\"",
            "if cargo build --features \"$A$B\"; then",
            "gh alias set --shell x \"cargo build --features $A$B\"",
            "ssh h \"cargo build $F\"",
            "sudo sh -c \"cargo build $F\"",
            "awk 'BEGIN{system(\"c\" \"argo build --features \" a b)}' a=li b=ve",
            "echo a | sed \"s/a/c&rgo build --features $A$B/e\"",
            "git -c 'alias.b=!sh -c \"$0\"' b \"$c $A\"",
            "make build FEATURES=$A$B",
            "$C build -F \"$A$B\"",
        ];
        let refused_only_by_reading_fail_closed = [
            "awk 'BEGIN{system(c)}' c=\"$RUN\"",
            "echo \"$X\" | sed e",
            "sed 's/^/x/e' \"$F\"",
            "sh <(echo \"$C $A\")",
            "\"$(printf %s \"$C $A\")\" x",
            "while $C \"$A\"; do :; done",
            "until $C \"$A\"; do :; done",
            "! $C \"$A\"",
            "time $C \"$A\"",
            "for i in 1; do $C \"$A\"; done",
            "echo \"$C $A\" | xargs sh",
            "echo \"$C $A\" | env sh",
        ];
        for line in refused_through_an_executing_command_at_line_two
            .iter()
            .chain(&refused_only_by_reading_fail_closed)
        {
            let files = [ci_file(
                ".github/scripts/build.sh",
                &format!("set -e\n{line}\n"),
            )];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 1, "{line}: {problems:?}");
            assert!(
                problems[0].contains(".github/scripts/build.sh:2"),
                "names the file and line: {problems:?}"
            );
        }
        let here_doc_fed_to_sh = [ci_file(
            ".github/scripts/build.sh",
            "set -e\nsh <<'EOF'\nc=cargo; A=li; B=ve; $c build --features $A$B\nEOF\n",
        )];
        let problems = live_feature_problems(&policy, &meta(), &here_doc_fed_to_sh)?;
        assert_eq!(problems.len(), 1, "a here-doc fed to sh: {problems:?}");
        assert!(
            problems[0].contains(".github/scripts/build.sh:3"),
            "names the file and the here-doc's line: {problems:?}"
        );
        let allowed_in_a_script = [
            (".github/scripts/build.sh", "set -e\necho \"$a $b\"\n"),
            (
                ".github/scripts/build.sh",
                "set -e\ncat <<EOF\ncargo build $F\nEOF\n",
            ),
            (".github/scripts/build.sh", "set -e\njq -n '\n$a $b\n'\n"),
        ];
        for (path, text) in allowed_in_a_script {
            let files = [ci_file(path, text)];
            assert_eq!(
                live_feature_problems(&policy, &meta(), &files)?,
                Vec::<String>::new(),
                "{text}"
            );
        }
        let refused_through_an_executing_command = [
            (
                ".github/scripts/build.sh",
                "set -e\nsetsid sh <<'EOF'\ncargo build --features $A$B\nEOF\n",
                ":3",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\necho '\ncargo build $F\n' | sh\n",
                ":3",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\nbash -c '\ncargo build $F\n'\n",
                ":3",
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - run: ${{ inputs.cmd }} build $F\n",
                ":2",
            ),
        ];
        for (path, text, line) in refused_through_an_executing_command {
            let files = [ci_file(path, text)];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 1, "{text}: {problems:?}");
            assert!(
                problems[0].contains(&format!("{path}{line}")),
                "names the file and line: {problems:?}"
            );
        }
        Ok(())
    }

    /// Arrays (the coordinator's eighth- and ninth-round rulings under DEC-176).
    ///
    /// Defining an array (`x=( … )`, one word) is never refused beyond the live-token backstop and
    /// the cargo-word rule, so `a=(cargo build --features "$A$B")` is refused on its own line
    /// beside `run_it "${a[@]}"`; a command word that expands a whole array (`"${x[@]}"`,
    /// `${x[*]}`) is refused when any definition of that array in the file holds `cargo` or an
    /// expansion, or the file defines no such array (eighth round). A re-read word that is exactly one whole-array expansion in argument position is
    /// read as the array's defined elements, which stay arguments of the outer command
    /// (`parse_flags "${flags[@]}"`), unless a definition of that array, `+=` appends included,
    /// holds the literal `cargo` or the token `live`, which is refused (ninth round). Split out of
    /// the bypass test so X1's step 3c un-ignores it (X1 tests correction 5).
    #[test]
    #[ignore = "pending E7-26"]
    fn array_expansions_are_read_through_their_definitions() -> Result<()> {
        let policy = live_policy();
        let meta = || workspace(live_workspace());
        let refused_at_line_two = [
            "x=(cargo build --features \"$A$B\"); \"${x[@]}\"",
            "x=($C build --features $A$B); \"${x[@]}\"",
            "\"${undefined[@]}\" build",
        ];
        for line in refused_at_line_two {
            let files = [ci_file(
                ".github/scripts/build.sh",
                &format!("set -e\n{line}\n"),
            )];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 1, "{line}: {problems:?}");
            assert!(
                problems[0].contains(".github/scripts/build.sh:2"),
                "names the file and line: {problems:?}"
            );
        }
        let allowed_in_a_script = [
            (
                ".github/scripts/build.sh",
                "set -e\nflags=()\nfor arg in \"$@\"; do flags+=(\"$arg\"); done\nparse_flags \"${flags[@]}\"\n",
            ),
            (
                ".github/scripts/build.sh",
                "set -e\nshards=(\"$A\" \"$A/tmp\")\n",
            ),
        ];
        for (path, text) in allowed_in_a_script {
            let files = [ci_file(path, text)];
            assert_eq!(
                live_feature_problems(&policy, &meta(), &files)?,
                Vec::<String>::new(),
                "{text}"
            );
        }
        let refused_on_two_lines = [(
            ".github/scripts/build.sh",
            "set -e\na=(cargo build --features \"$A$B\")\nrun_it \"${a[@]}\"\n",
            [":2", ":3"],
        )];
        for (path, text, lines) in refused_on_two_lines {
            let files = [ci_file(path, text)];
            let problems = live_feature_problems(&policy, &meta(), &files)?;
            assert_eq!(problems.len(), 2, "{text}: {problems:?}");
            for (problem, line) in problems.iter().zip(lines) {
                assert!(
                    problem.contains(&format!("{path}{line}")),
                    "the assignment and the expansion are each refused: {problems:?}"
                );
            }
        }
        Ok(())
    }

    /// A marked crate that is not a workspace member is no runner, so even the compile-only form
    /// naming it is refused, beside the membership problem (#738 review, finding 2: the
    /// membership test of the runner).
    #[test]
    fn a_marked_crate_outside_the_workspace_compiles_nothing() -> Result<()> {
        let crates = vec![member("a-lib", &[]), member("a-tool", &[])];
        let files = [ci_file(
            ".github/workflows/ci.yml",
            "      - run: cargo check -p the-runner --features live",
        )];
        let problems = live_feature_problems(&live_policy(), &workspace(crates), &files)?;
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems
                .iter()
                .any(|p| p.contains(".github/workflows/ci.yml:1")),
            "the compile-only line is refused: {problems:?}"
        );
        Ok(())
    }

    /// Only the runner's own features are read for a feature that turns `live` on: another
    /// crate's feature naming its own `live` is not counted twice beside its declaration, and a
    /// runner feature that enables something other than `live` is no problem (#738 review,
    /// finding 2: the two conditions of the runner's own feature check).
    #[test]
    fn only_the_runner_s_own_features_turn_its_live_on() -> Result<()> {
        let policy = live_policy();
        let crates = vec![
            with_features(member(RUNNER, &[]), &[("live", &[])]),
            with_features(member("a-lib", &[]), &[("live", &[]), ("go", &["live"])]),
        ];
        let problems = live_feature_problems(&policy, &workspace(crates), &[])?;
        assert_eq!(problems.len(), 1, "only the declaration: {problems:?}");
        let crates = vec![with_features(
            member(RUNNER, &[]),
            &[("live", &["a-tool/fast"]), ("other", &["a-tool/fast"])],
        )];
        assert_eq!(
            live_feature_problems(&policy, &workspace(crates), &[])?,
            Vec::<String>::new(),
            "runner features that enable something else"
        );
        Ok(())
    }

    /// The files the check reads are every file that decides a build: workflows (`.yml` and
    /// `.yaml`), scripts, composite actions under `.github/actions`, `deploy/`'s shell scripts
    /// (the demo host's runbook, DEC-822), and `.cargo/config.toml` (aliases and `rustflags`); a
    /// repository without the optional ones reads the rest (#738 review, finding 4).
    #[test]
    fn ci_files_reads_every_file_that_decides_a_build() -> Result<()> {
        let root = env::temp_dir().join(format!("mandate-xtask-ci-files-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        let write = |path: &str| -> Result<()> {
            let file = root.join(path);
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(file, "x\n")?;
            Ok(())
        };
        write(".github/workflows/a.yml")?;
        let only_workflows = ci_files(&root)
            .map(|files| files.into_iter().map(|file| file.path).collect::<Vec<_>>());
        for path in [
            ".github/workflows/b.yaml",
            ".github/scripts/c.sh",
            ".github/actions/d/action.yml",
            ".github/actions/e/action.yaml",
            ".cargo/config.toml",
            ".github/pull_request_template.md",
            "deploy/f.sh",
            "deploy/README.md",
        ] {
            write(path)?;
        }
        let mut read: Vec<String> = ci_files(&root)?.into_iter().map(|file| file.path).collect();
        fs::remove_dir_all(&root).ok();
        read.sort();
        assert_eq!(
            only_workflows?,
            [".github/workflows/a.yml"],
            "the optional directories may be absent"
        );
        assert_eq!(
            read,
            [
                ".cargo/config.toml",
                ".github/actions/d/action.yml",
                ".github/actions/e/action.yaml",
                ".github/scripts/c.sh",
                ".github/workflows/a.yml",
                ".github/workflows/b.yaml",
                "deploy/f.sh",
            ]
        );
        Ok(())
    }

    /// The build files the check reads (the coordinator's eleventh-round ruling under DEC-176):
    /// `Makefile`, `*.mk`, `justfile`, `Dockerfile*` and `docker-compose*.yml` anywhere in the
    /// repository, beside every file [`ci_files_reads_every_file_that_decides_a_build`] names.
    /// Split out so X1's step 3c un-ignores it (X1 tests correction 5).
    #[test]
    #[ignore = "pending E7-26"]
    fn ci_files_reads_the_build_files_anywhere_in_the_repository() -> Result<()> {
        let root =
            env::temp_dir().join(format!("mandate-xtask-build-files-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        let write = |path: &str| -> Result<()> {
            let file = root.join(path);
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(file, "x\n")?;
            Ok(())
        };
        write(".github/workflows/a.yml")?;
        let only_workflows = ci_files(&root)
            .map(|files| files.into_iter().map(|file| file.path).collect::<Vec<_>>());
        for path in [
            ".github/workflows/b.yaml",
            ".github/scripts/c.sh",
            ".github/actions/d/action.yml",
            ".github/actions/e/action.yaml",
            ".cargo/config.toml",
            ".github/pull_request_template.md",
            "deploy/f.sh",
            "deploy/README.md",
            "Makefile",
            "tools/build.mk",
            "justfile",
            "docker/Dockerfile.ci",
            "docker-compose.ci.yml",
        ] {
            write(path)?;
        }
        let mut read: Vec<String> = ci_files(&root)?.into_iter().map(|file| file.path).collect();
        fs::remove_dir_all(&root).ok();
        read.sort();
        assert_eq!(
            only_workflows?,
            [".github/workflows/a.yml"],
            "the optional directories may be absent"
        );
        assert_eq!(
            read,
            [
                ".cargo/config.toml",
                ".github/actions/d/action.yml",
                ".github/actions/e/action.yaml",
                ".github/scripts/c.sh",
                ".github/workflows/a.yml",
                ".github/workflows/b.yaml",
                "Makefile",
                "deploy/f.sh",
                "docker-compose.ci.yml",
                "docker/Dockerfile.ci",
                "justfile",
                "tools/build.mk",
            ]
        );
        Ok(())
    }

    /// A minimal Cargo workspace with one crate, `a`, and its layering policy, under `root`, so
    /// the lint job's checks that read the workspace can run there.
    fn fixture_workspace(root: &Path) -> Result<()> {
        fs::create_dir_all(root.join("a/src"))?;
        fs::create_dir_all(root.join("xtask"))?;
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\"]\nresolver = \"3\"\n",
        )?;
        fs::write(
            root.join("a/Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )?;
        fs::write(root.join("a/src/lib.rs"), "//! A fixture crate.\n")?;
        fs::write(
            root.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.0.0\"\n",
        )?;
        fs::write(
            root.join("xtask/layers.toml"),
            concat!(
                "impure_crates = []\n\n",
                "[crates.a]\nlayer = 1\nsafety_critical = false\npure = false\n",
            ),
        )?;
        Ok(())
    }

    /// The lint job runs the live-feature check over its own repository (#738 review, finding
    /// 2): a fixture workspace with a clean workflow passes, and a workflow that runs `live`
    /// fails the job naming the check, before the workspace checks run. The fixture has the
    /// `deploy/` directory main's lint job runs ShellCheck over (#795), with one clean script.
    #[test]
    fn the_lint_job_runs_the_live_feature_check_on_its_repository() -> Result<()> {
        let root = env::temp_dir().join(format!("mandate-xtask-lint-live-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        fixture_workspace(&root)?;
        let workflows = root.join(".github/workflows");
        fs::create_dir_all(&workflows)?;
        fs::create_dir_all(root.join(".github/scripts"))?;
        fs::create_dir_all(root.join("deploy"))?;
        fs::write(
            root.join("deploy/clean.sh"),
            "#!/usr/bin/env bash\nset -euo pipefail\necho \"deployed\"\n",
        )?;
        let workflow = |run: &str| {
            format!(
                "on: push\njobs:\n  check:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: {run}\n"
            )
        };
        fs::write(workflows.join("ci.yml"), workflow("cargo check -p a"))?;
        let clean = lint(&root, || Ok(())).map_err(|err| format!("{err:#}"));
        fs::write(
            workflows.join("ci.yml"),
            workflow("cargo run -p a --features live"),
        )?;
        let reached = Cell::new(false);
        let planted = lint(&root, || {
            reached.set(true);
            Ok(())
        })
        .map_err(|err| format!("{err:#}"))
        .err();
        fs::remove_dir_all(&root).ok();
        assert_eq!(clean, Ok(()), "a clean fixture workspace passes");
        assert!(
            planted
                .as_deref()
                .is_some_and(|err| err.contains("live-feature")),
            "a workflow that runs `live` fails the lint job naming the check, got {planted:?}"
        );
        assert!(
            !reached.get(),
            "the workspace checks run only after it passes"
        );
        Ok(())
    }
}
