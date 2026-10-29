//! `cargo xtask gitops promote`: moving the hosted clusters to new signed digests (E21-58,
//! DEC-2016).
//!
//! Staging follows `main`. `promote staging <dir>` reads a `release.yml` run on `main`'s `CHARTS`
//! (the signed charts) and `PUBLISHED` (the signed multi-arch image indexes) from `<dir>`, and
//! rewrites the chart digests in `deploy/gitops/clusters/staging/charts.yaml` and the images in its
//! `images.yaml`. Production moves only by `promote production`, which copies staging's two files'
//! digests as they stand. `deploy/tests/test_gitops.py` refuses a production digest that staging
//! has not run on `main`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use crate::gitops::Environment;

/// The published binaries promotion pins in a hosted cell, each with the chart's `images` key it
/// fills; the tests' `RELEASED_NOT_PROMOTED` names each one released but held back, with the overlay value it lacks.
/// A binary not named here (`mandate-agentd`, `mandate-rh-sim`) is not a chart process yet, and
/// the simulator never is. A lane that publishes a process's binary adds its row here.
const CHART_IMAGE_FOR_BINARY: [(&str, &str); 11] = [
    ("mandate", "mandate"),
    ("mandate-api-server", "api"),
    ("mandate-deployd", "deploy-manager"),
    ("mandate-daemon", "runtime"),
    ("mandate-executord", "executor"),
    ("mandate-provisionerd", "role-provisioner"),
    ("mandate-token-exchange", "token-exchange"),
    ("mandate-scheduler", "scheduler"),
    ("mandate-dispatcher", "dispatcher"),
    ("mandate-datad", "data"),
    ("mandate-gatewayd", "model-gateway"),
];

/// The charts `release.yml` publishes (`HOSTED_CHARTS`), by the name `CHARTS` lists. An overlay
/// pins those it installs; promotion rewrites exactly those.
const PUBLISHED_CHARTS: [&str; 4] = [
    "mandate-backup-plugin-tls",
    "mandate-journal-db",
    "mandate-vault-server",
    "mandate-workspace",
];

/// The registry path every published image lives under.
const PUBLISHED_PREFIX: &str = "ghcr.io/kunwarshivam/mandate/";

/// The registry path every published chart lives under.
const CHART_PREFIX: &str = "ghcr.io/kunwarshivam/mandate/charts/";

/// One `<repository>@sha256:<64 hex>  <name>` line of `PUBLISHED` or `CHARTS`, as `(name,
/// repository, digest)`, its repository under `prefix` and ending in its name. Refuses anything else.
fn entry(line: &str, prefix: &str) -> Result<(String, String, String)> {
    let mut fields = line.split_whitespace();
    let reference = fields.next().context("an empty line")?;
    let name = fields.next().context("a reference with no name")?;
    let (repository, digest) = reference
        .split_once('@')
        .with_context(|| format!("{reference} is not pinned by digest"))?;
    let hex = digest
        .strip_prefix("sha256:")
        .with_context(|| format!("{reference} is not a SHA-256 digest"))?;
    ensure!(
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "{reference}: a malformed digest"
    );
    ensure!(
        repository.strip_prefix(prefix) == Some(name),
        "{reference} is not {name} under {prefix}"
    );
    Ok((name.to_owned(), repository.to_owned(), digest.to_owned()))
}

/// The chart's `images` from `PUBLISHED`: each [`CHART_IMAGE_FOR_BINARY`] binary's reference,
/// which must be there exactly once.
fn chart_images(published: &str) -> Result<BTreeMap<String, String>> {
    let mut by_binary = BTreeMap::new();
    for line in published.lines().filter(|line| !line.trim().is_empty()) {
        let (name, repository, digest) = entry(line, PUBLISHED_PREFIX)?;
        ensure!(
            by_binary
                .insert(name.clone(), format!("{repository}@{digest}"))
                .is_none(),
            "PUBLISHED lists {name} twice"
        );
    }
    let mut images = BTreeMap::new();
    for (binary, key) in CHART_IMAGE_FOR_BINARY {
        let reference = by_binary
            .get(binary)
            .with_context(|| format!("PUBLISHED has no {binary}"))?;
        images.insert(key.to_owned(), reference.clone());
    }
    Ok(images)
}

/// Each published chart's digest from `CHARTS`: every one of [`PUBLISHED_CHARTS`], exactly once.
fn chart_digests(charts: &str) -> Result<BTreeMap<String, String>> {
    let mut digests = BTreeMap::new();
    for line in charts.lines().filter(|line| !line.trim().is_empty()) {
        let (name, _, digest) = entry(line, CHART_PREFIX)?;
        ensure!(
            PUBLISHED_CHARTS.contains(&name.as_str()),
            "CHARTS lists an unknown chart {name}"
        );
        ensure!(
            digests.insert(name.clone(), digest).is_none(),
            "CHARTS lists {name} twice"
        );
    }
    for chart in PUBLISHED_CHARTS {
        ensure!(digests.contains_key(chart), "CHARTS has no {chart}");
    }
    Ok(digests)
}

/// An environment's `images.yaml`: its header, its tier, then each image of `images` in key order.
fn images_yaml(environment: Environment, images: &BTreeMap<String, String>) -> String {
    let mut out = format!(
        "# The {name} workspace's tier and images, values for the workspace chart. Each image is a\n\
         # signed index release.yml published from `main`, pinned by digest. Written by `cargo xtask\n\
         # gitops promote` (DEC-2016); change it only through a promotion.\n\
         deploymentTier: {name}\n",
        name = environment.name()
    );
    if !images.is_empty() {
        out.push_str("images:\n");
        for (key, reference) in images {
            out.push_str(&format!("  {key}: {reference}\n"));
        }
    }
    out
}

/// The images an `images.yaml` holds, refusing any line [`images_yaml`] would not write for
/// `environment`.
fn read_images(text: &str, environment: Environment) -> Result<BTreeMap<String, String>> {
    let tier = format!("deploymentTier: {}", environment.name());
    let mut images = BTreeMap::new();
    let mut inside = false;
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        match line {
            "images:" => inside = true,
            "" => {}
            line if line == tier && !inside => {}
            entry if inside && entry.starts_with("  ") => {
                let (key, reference) = entry
                    .trim_start()
                    .split_once(": ")
                    .with_context(|| format!("images.yaml: an unexpected line {entry:?}"))?;
                ensure!(
                    images
                        .insert(key.to_owned(), reference.to_owned())
                        .is_none(),
                    "images.yaml: {key} twice"
                );
            }
            other => bail!("images.yaml: an unexpected line {other:?}"),
        }
    }
    Ok(images)
}

/// The chart digests a `charts.yaml` pins, by the name in each OCIRepository's `metadata`.
fn read_chart_digests(charts: &str) -> Result<BTreeMap<String, String>> {
    let mut digests = BTreeMap::new();
    let mut current: Option<&str> = None;
    for line in charts.lines() {
        if line == "---" {
            current = None;
        } else if let Some(name) = line.strip_prefix("  name: ") {
            current = Some(name.trim());
        } else if let Some(digest) = line.strip_prefix("    digest: ") {
            let name = current.context("charts.yaml: a digest before its name")?;
            ensure!(
                digests
                    .insert(name.to_owned(), digest.trim().to_owned())
                    .is_none(),
                "charts.yaml pins {name} twice"
            );
        }
    }
    Ok(digests)
}

/// `charts`, an environment's `charts.yaml`, with each OCIRepository's digest set from `digests`.
/// Every chart it pins must be one `digests` holds; a published chart it does not pin, such as
/// one whose release it does not run yet, is left out.
fn rewrite_charts(charts: &str, digests: &BTreeMap<String, String>) -> Result<String> {
    ensure!(
        !read_chart_digests(charts)?.is_empty(),
        "charts.yaml pins no chart"
    );
    let mut out = String::with_capacity(charts.len());
    let mut current: Option<&str> = None;
    for line in charts.lines() {
        if line == "---" {
            current = None;
        } else if let Some(name) = line.strip_prefix("  name: ") {
            current = Some(name.trim());
        }
        match line.strip_prefix("    digest: ") {
            Some(_) => {
                let name = current.context("charts.yaml: a digest before its name")?;
                let digest = digests
                    .get(name)
                    .with_context(|| format!("no digest for {name}"))?;
                out.push_str(&format!("    digest: {digest}\n"));
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

fn cluster_dir(root: &Path, environment: Environment) -> PathBuf {
    root.join("deploy/gitops/clusters").join(environment.name())
}

/// Writes each file beside its target first and renames it into place only once every one is on
/// disk, so a failed write leaves the overlay as it was, with no file half written and no stray
/// sibling (DEC-2048 item 3). The renames run one after another, so a rename that fails can still
/// leave one file replaced and the other not; the next promotion rewrites both.
fn write_all(files: &[(PathBuf, String)]) -> Result<()> {
    let mut written: Vec<(PathBuf, &Path)> = Vec::new();
    for (path, contents) in files {
        let beside = path.with_extension("yaml.promoting");
        if let Err(error) = fs::write(&beside, contents) {
            for (left, _) in &written {
                fs::remove_file(left).ok();
            }
            return Err(error).with_context(|| format!("writing {}", beside.display()));
        }
        written.push((beside, path));
    }
    for (beside, path) in written {
        fs::rename(&beside, path).with_context(|| format!("replacing {}", path.display()))?;
    }
    Ok(())
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// `cargo xtask gitops promote staging <dir>`: staging's two files from a release run's `CHARTS`
/// and `PUBLISHED` in `dir`. `cargo xtask gitops promote production`: production's from staging's,
/// as they stand.
pub(crate) fn promote(
    root: &Path,
    environment: Environment,
    published: Option<&Path>,
) -> Result<()> {
    let (images, digests) = match (environment, published) {
        (Environment::Staging, Some(dir)) => (
            chart_images(&read(&dir.join("PUBLISHED"))?)?,
            chart_digests(&read(&dir.join("CHARTS"))?)?,
        ),
        (Environment::Production, None) => {
            let staging = cluster_dir(root, Environment::Staging);
            (
                read_images(&read(&staging.join("images.yaml"))?, Environment::Staging)?,
                read_chart_digests(&read(&staging.join("charts.yaml"))?)?,
            )
        }
        (Environment::Staging, None) => {
            bail!("staging is promoted from a release run's CHARTS and PUBLISHED")
        }
        (Environment::Production, Some(_)) => {
            bail!("production is promoted only from staging, never from a release run (DEC-2016)")
        }
    };
    let dir = cluster_dir(root, environment);
    let charts = rewrite_charts(&read(&dir.join("charts.yaml"))?, &digests)?;
    write_all(&[
        (dir.join("charts.yaml"), charts),
        (dir.join("images.yaml"), images_yaml(environment, &images)),
    ])?;
    eprintln!(
        "    gitops: wrote {}'s charts.yaml and images.yaml",
        environment.name()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chart processes a release publishes but promotion does not yet pin in a hosted cell, each
    /// with the value its overlay lacks: the chart refuses to render the process without it, so
    /// promoting the image alone would stop the whole workspace release from reconciling
    /// (DEC-2079 item 3). A process moves to [`CHART_IMAGE_FOR_BINARY`] in the change that gives the
    /// overlays its value.
    const RELEASED_NOT_PROMOTED: [(&str, &str, &str); 1] = [(
        "mandate-news-reader",
        "news-reader",
        "news.follows in staging's overlay, platform/news-data-key and vault-approle-news-reader in the staging vault, which the founder's apply writes (FOUNDER-STEPS M), and the cell's run of the reader against a recorded news fixture (E2-10c): the chart refuses the reader without followed agents, without its key it is never ready and holds the whole workspace release unready, and without the cell's run staging would be the first real run of the chart and the binary together, so it is promoted in the change after all three (DEC-2084 item 8)",
    )];

    fn digest(c: char) -> String {
        format!("sha256:{}", c.to_string().repeat(64))
    }

    fn published() -> String {
        format!(
            "ghcr.io/kunwarshivam/mandate/mandate@{}  mandate\n\
             ghcr.io/kunwarshivam/mandate/mandate-agentd@{}  mandate-agentd\n\
             ghcr.io/kunwarshivam/mandate/mandate-rh-sim@{}  mandate-rh-sim\n\
             ghcr.io/kunwarshivam/mandate/mandate-api-server@{}  mandate-api-server\n\
             ghcr.io/kunwarshivam/mandate/mandate-deployd@{}  mandate-deployd\n\
             ghcr.io/kunwarshivam/mandate/mandate-daemon@{}  mandate-daemon\n\
             ghcr.io/kunwarshivam/mandate/mandate-executord@{}  mandate-executord\n\
             ghcr.io/kunwarshivam/mandate/mandate-provisionerd@{}  mandate-provisionerd\n\
             ghcr.io/kunwarshivam/mandate/mandate-token-exchange@{}  mandate-token-exchange\n\
             ghcr.io/kunwarshivam/mandate/mandate-scheduler@{}  mandate-scheduler\n\
             ghcr.io/kunwarshivam/mandate/mandate-dispatcher@{}  mandate-dispatcher\n\
             ghcr.io/kunwarshivam/mandate/mandate-datad@{}  mandate-datad\n\
             ghcr.io/kunwarshivam/mandate/mandate-gatewayd@{}  mandate-gatewayd\n\
             ghcr.io/kunwarshivam/mandate/mandate-news-reader@{}  mandate-news-reader\n",
            digest('a'),
            digest('b'),
            digest('c'),
            digest('d'),
            digest('e'),
            digest('f'),
            digest('9'),
            digest('8'),
            digest('7'),
            digest('6'),
            digest('5'),
            digest('0'),
            digest('2'),
            digest('3')
        )
    }

    fn charts_listing() -> String {
        PUBLISHED_CHARTS
            .iter()
            .zip(['1', '2', '3', '4'])
            .map(|(chart, c)| {
                format!(
                    "ghcr.io/kunwarshivam/mandate/charts/{chart}@{}  {chart}  0.1.0+g545f4852774c\n",
                    digest(c)
                )
            })
            .collect()
    }

    fn overlay(environment: Environment, file: &str) -> Result<String> {
        Ok(fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../deploy/gitops/clusters")
                .join(environment.name())
                .join(file),
        )?)
    }

    /// A temporary copy of both overlays' `charts.yaml` and `images.yaml`, under a repository root.
    fn scratch_root(name: &str) -> Result<PathBuf> {
        let root =
            std::env::temp_dir().join(format!("xtask-promote-{name}-{}", std::process::id()));
        for environment in [Environment::Staging, Environment::Production] {
            let dir = cluster_dir(&root, environment);
            fs::create_dir_all(&dir)?;
            for file in ["charts.yaml", "images.yaml"] {
                fs::write(dir.join(file), overlay(environment, file)?)?;
            }
        }
        Ok(root)
    }

    /// A write that fails partway leaves every file as it was and no sibling behind.
    #[test]
    fn a_failed_write_changes_nothing() -> Result<()> {
        let root = scratch_root("atomic")?;
        let dir = cluster_dir(&root, Environment::Staging);
        let charts = dir.join("charts.yaml");
        let before = read(&charts)?;
        let missing = dir.join("absent").join("images.yaml");
        assert!(
            write_all(&[
                (charts.clone(), "changed\n".to_owned()),
                (missing, String::new())
            ])
            .is_err()
        );
        assert_eq!(read(&charts)?, before);
        assert!(!charts.with_extension("yaml.promoting").exists());
        write_all(&[(charts.clone(), "changed\n".to_owned())])?;
        assert_eq!(read(&charts)?, "changed\n");
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn staging_takes_each_charted_binary_and_nothing_else() -> Result<()> {
        let images = chart_images(&published())?;
        assert_eq!(
            images,
            BTreeMap::from([
                (
                    "api".to_owned(),
                    format!(
                        "ghcr.io/kunwarshivam/mandate/mandate-api-server@{}",
                        digest('d')
                    )
                ),
                (
                    "data".to_owned(),
                    format!("ghcr.io/kunwarshivam/mandate/mandate-datad@{}", digest('0'))
                ),
                (
                    "model-gateway".to_owned(),
                    format!(
                        "ghcr.io/kunwarshivam/mandate/mandate-gatewayd@{}",
                        digest('2')
                    )
                ),
                (
                    "deploy-manager".to_owned(),
                    format!(
                        "ghcr.io/kunwarshivam/mandate/mandate-deployd@{}",
                        digest('e')
                    )
                ),
                (
                    "executor".to_owned(),
                    format!(
                        "ghcr.io/kunwarshivam/mandate/mandate-executord@{}",
                        digest('9')
                    )
                ),
                (
                    "mandate".to_owned(),
                    format!("ghcr.io/kunwarshivam/mandate/mandate@{}", digest('a'))
                ),
                (
                    "role-provisioner".to_owned(),
                    format!(
                        "ghcr.io/kunwarshivam/mandate/mandate-provisionerd@{}",
                        digest('8')
                    )
                ),
                (
                    "runtime".to_owned(),
                    format!(
                        "ghcr.io/kunwarshivam/mandate/mandate-daemon@{}",
                        digest('f')
                    )
                ),
                (
                    "token-exchange".to_owned(),
                    format!(
                        "ghcr.io/kunwarshivam/mandate/mandate-token-exchange@{}",
                        digest('7')
                    )
                ),
                (
                    "scheduler".to_owned(),
                    format!(
                        "ghcr.io/kunwarshivam/mandate/mandate-scheduler@{}",
                        digest('6')
                    )
                ),
                (
                    "dispatcher".to_owned(),
                    format!(
                        "ghcr.io/kunwarshivam/mandate/mandate-dispatcher@{}",
                        digest('5')
                    )
                ),
            ])
        );
        assert!(chart_images(&published().replace("mandate@", "mandate:main@")).is_err());
        assert!(chart_images(&published().replace("sha256:", "sha512:")).is_err());
        assert!(chart_images(&format!("{}{}", published(), published())).is_err());
        assert!(
            chart_images(&published().replace("ghcr.io/kunwarshivam", "ghcr.io/someone")).is_err()
        );
        assert!(chart_images(&published().replace("mandate@", "other/mandate@")).is_err());
        for binary in [
            "mandate",
            "mandate-api-server",
            "mandate-deployd",
            "mandate-daemon",
            "mandate-executord",
            "mandate-provisionerd",
            "mandate-token-exchange",
        ] {
            let without: String = published()
                .lines()
                .filter(|line| !line.ends_with(&format!("  {binary}")))
                .map(|line| format!("{line}\n"))
                .collect();
            assert!(chart_images(&without).is_err(), "{binary}");
        }
        Ok(())
    }

    /// Every binary the chart runs from a release is one the release builds, signs and publishes,
    /// so a hosted cell can pin each of its processes to a signed digest; the API server's image
    /// is `images.api` (DEC-2026).
    #[test]
    fn every_charted_binary_is_released() {
        let released: Vec<&str> = crate::release::BINARIES
            .iter()
            .map(|(_, binary)| *binary)
            .collect();
        for (binary, key) in CHART_IMAGE_FOR_BINARY {
            assert!(
                released.contains(&binary),
                "{binary} ({key}) is not released"
            );
        }
        for row in [
            ("mandate-api-server", "api"),
            ("mandate-deployd", "deploy-manager"),
            ("mandate-daemon", "runtime"),
            ("mandate-executord", "executor"),
            ("mandate-provisionerd", "role-provisioner"),
            ("mandate-token-exchange", "token-exchange"),
            ("mandate-datad", "data"),
            ("mandate-gatewayd", "model-gateway"),
        ] {
            assert!(
                CHART_IMAGE_FOR_BINARY.contains(&row),
                "{row:?} is not charted"
            );
        }
    }

    /// Every process the workspace chart builds an image for runs one this map promotes, so a hosted
    /// cell can pin each of them to a signed digest: staging ran with only `api` and `mandate` from
    /// 2026-10-19, because the scheduler, the dispatcher, the model gateway and the data process had
    /// no row here and none was released. `postgres` is a third-party image the overlays pin
    /// themselves (DEC-2017), and a process the chart does not build yet has no image to promote.
    #[test]
    fn every_built_chart_process_is_promoted() -> Result<()> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../deploy/cell/charts/workspace/processes.json");
        let listed: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        let processes = listed["processes"]
            .as_array()
            .context("processes.json holds a list of processes")?;
        let charted: Vec<&str> = CHART_IMAGE_FOR_BINARY.iter().map(|(_, key)| *key).collect();
        let held: Vec<&str> = RELEASED_NOT_PROMOTED
            .iter()
            .map(|(_, key, _)| *key)
            .collect();
        let mut missing = Vec::new();
        for process in processes {
            let image = process["image"]
                .as_str()
                .context("a process names its image")?;
            if process["built"] == true
                && image != "postgres"
                && !charted.contains(&image)
                && !held.contains(&image)
            {
                missing.push(format!(
                    "{} ({image})",
                    process["name"].as_str().unwrap_or("?")
                ));
            }
        }
        assert!(missing.is_empty(), "built but never promoted: {missing:?}");
        Ok(())
    }

    /// A held process is released, so its signed digest is ready the day its overlay value lands,
    /// and is not also promoted, which would pin an image the chart refuses to render.
    #[test]
    fn every_held_process_is_released_and_not_promoted() {
        let released: Vec<&str> = crate::release::BINARIES
            .iter()
            .map(|(_, binary)| *binary)
            .collect();
        for (binary, key, lacks) in RELEASED_NOT_PROMOTED {
            assert!(
                released.contains(&binary),
                "{binary} ({key}) is not released"
            );
            assert!(
                !CHART_IMAGE_FOR_BINARY
                    .iter()
                    .any(|(b, k)| *b == binary || *k == key),
                "{binary} ({key}) is both held and promoted"
            );
            assert!(!lacks.is_empty(), "{binary} names no reason");
        }
    }

    #[test]
    fn every_published_chart_is_listed_once() -> Result<()> {
        let digests = chart_digests(&charts_listing())?;
        assert_eq!(digests.len(), PUBLISHED_CHARTS.len());
        assert_eq!(digests["mandate-workspace"], digest('4'));
        let missing: String = charts_listing()
            .lines()
            .filter(|line| !line.contains("mandate-vault-server"))
            .map(|line| format!("{line}\n"))
            .collect();
        assert!(chart_digests(&missing).is_err());
        assert!(chart_digests(&format!("{}{}", charts_listing(), charts_listing())).is_err());
        assert!(
            chart_digests(&charts_listing().replace("mandate-workspace", "mandate-other")).is_err()
        );
        assert!(chart_digests(&charts_listing().replace("/charts/", "/")).is_err());
        Ok(())
    }

    #[test]
    fn each_pinned_chart_is_rewritten_and_nothing_else_changes() -> Result<()> {
        let digests = chart_digests(&charts_listing())?;
        let before = overlay(Environment::Staging, "charts.yaml")?;
        let after = rewrite_charts(&before, &digests)?;
        let pinned = read_chart_digests(&before)?;
        let rewritten = read_chart_digests(&after)?;
        assert_eq!(
            rewritten.keys().collect::<Vec<_>>(),
            pinned.keys().collect::<Vec<_>>()
        );
        for (name, value) in &rewritten {
            assert_eq!(value, &digests[name]);
        }
        let unchanged = |text: &str| -> Vec<String> {
            text.lines()
                .filter(|line| !line.starts_with("    digest: "))
                .map(str::to_owned)
                .collect()
        };
        assert_eq!(unchanged(&before), unchanged(&after));
        let mut fewer = digests.clone();
        fewer.remove("mandate-workspace");
        assert!(rewrite_charts(&before, &fewer).is_err());
        assert!(rewrite_charts("---\n", &digests).is_err());
        Ok(())
    }

    #[test]
    fn images_yaml_reads_back_what_it_wrote_and_keeps_the_tier() -> Result<()> {
        let images = chart_images(&published())?;
        for environment in [Environment::Staging, Environment::Production] {
            let text = images_yaml(environment, &images);
            assert!(text.contains(&format!("\ndeploymentTier: {}\n", environment.name())));
            assert_eq!(read_images(&text, environment)?, images);
            assert_eq!(
                read_images(&images_yaml(environment, &BTreeMap::new()), environment)?,
                BTreeMap::new()
            );
            let pinned = read_images(&overlay(environment, "images.yaml")?, environment)?;
            assert!(
                pinned
                    .values()
                    .all(|reference| reference.starts_with(PUBLISHED_PREFIX)),
                "{pinned:?}"
            );
        }
        assert!(
            read_images(
                &images_yaml(Environment::Staging, &images),
                Environment::Production
            )
            .is_err()
        );
        assert!(read_images("images:\nmandate: x\n", Environment::Staging).is_err());
        assert!(read_images("deploymentTier: ci\n", Environment::Staging).is_err());
        Ok(())
    }

    #[test]
    fn staging_moves_to_a_release_and_production_copies_staging() -> Result<()> {
        let root = scratch_root("flow")?;
        let release = root.join("release");
        fs::create_dir_all(&release)?;
        fs::write(release.join("PUBLISHED"), published())?;
        fs::write(release.join("CHARTS"), charts_listing())?;
        let production_before =
            read(&cluster_dir(&root, Environment::Production).join("charts.yaml"))?;

        promote(&root, Environment::Staging, Some(&release))?;
        let staging = cluster_dir(&root, Environment::Staging);
        let staged = read_chart_digests(&read(&staging.join("charts.yaml"))?)?;
        assert_eq!(staged["mandate-workspace"], digest('4'));
        assert_eq!(
            read_images(&read(&staging.join("images.yaml"))?, Environment::Staging)?,
            chart_images(&published())?
        );
        assert_eq!(
            read(&cluster_dir(&root, Environment::Production).join("charts.yaml"))?,
            production_before,
            "promoting staging never moves production"
        );

        promote(&root, Environment::Production, None)?;
        let production = cluster_dir(&root, Environment::Production);
        let mut copied = staged.clone();
        copied.remove("mandate-vault-server");
        assert_eq!(
            read_chart_digests(&read(&production.join("charts.yaml"))?)?,
            copied,
            "production copies every chart it pins from staging, and never gains the vault, which \
             staging alone runs (DEC-2051)"
        );
        let production_images = read(&production.join("images.yaml"))?;
        assert!(production_images.contains("\ndeploymentTier: production\n"));
        assert_eq!(
            read_images(&production_images, Environment::Production)?,
            chart_images(&published())?
        );

        assert!(promote(&root, Environment::Production, Some(&release)).is_err());
        assert!(promote(&root, Environment::Staging, None).is_err());
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    /// Every chart key a binary fills is one the workspace chart's values name.
    #[test]
    fn every_charted_binary_fills_a_key_the_chart_has() -> Result<()> {
        let values = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../deploy/cell/charts/workspace/values.yaml"),
        )?;
        let images = values
            .split_once("\nimages:\n")
            .and_then(|(_, rest)| rest.split_once("\n\n"))
            .map(|(images, _)| images)
            .context("the chart's values have no images block")?;
        for (_, key) in CHART_IMAGE_FOR_BINARY {
            assert!(
                images
                    .lines()
                    .any(|line| line.starts_with(&format!("  {key}: "))),
                "{key}"
            );
        }
        Ok(())
    }

    /// The charts promotion knows are the ones `release.yml` publishes, by each chart's own name.
    #[test]
    fn the_published_charts_are_releases() -> Result<()> {
        let charts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../deploy/cell/charts");
        let mut hosted = Vec::new();
        for chart in crate::release::HOSTED_CHARTS {
            let manifest = fs::read_to_string(charts.join(chart).join("Chart.yaml"))?;
            let name = manifest
                .lines()
                .find_map(|line| line.strip_prefix("name: "))
                .with_context(|| format!("{chart} has no name"))?;
            hosted.push(name.trim().to_owned());
        }
        hosted.sort();
        assert_eq!(hosted, PUBLISHED_CHARTS.map(str::to_owned));
        Ok(())
    }
}
