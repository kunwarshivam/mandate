//! `cargo xtask release ca-roots`: the release fails unless every published process that reaches a
//! public TLS host is given CA roots (E10-25, DEC-2374). Each release image is a bare binary with
//! none (E21-35), so a process whose egress names a public TLS destination builds no HTTP client
//! without the bundle the workspace chart copies into its pod (DEC-2371). This check reads the
//! release's own binaries, their processes in `processes.json`, and the chart as it renders, so a
//! binary published with public egress and no bundle fails the release rather than staging.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::cell::render_with;
use crate::promote::chart_image_keys;

/// The egress destinations reached over public TLS (DEC-2371 item 1), written out here rather than
/// read from the chart's `mandate.publicTls`, so the chart cannot narrow what this checks.
pub const PUBLIC_TLS: [&str; 7] = [
    "identity-provider",
    "broker",
    "market-data",
    "token-host",
    "model-providers",
    "object-storage",
    "timestamping",
];

/// The bundle a process trusts, its directory, and the system bundle its init container copies.
const CA_BUNDLE: &str = "/run/ca-bundle/ca-certificates.crt";
const CA_BUNDLE_DIR: &str = "/run/ca-bundle";
const COPY: &str = "cp /etc/ssl/certs/ca-certificates.crt /run/ca-bundle/ca-certificates.crt";

/// The ConfigMap whose `<process>.json` entries are the pods the deployment manager starts.
const TEMPLATES: &str = "deploy-manager-templates";

/// The processes of `processes` (`processes.json`'s list) whose image is one of `published` and
/// whose egress names a [`PUBLIC_TLS`] destination.
fn public_tls(processes: &[Value], published: &[&str]) -> Vec<String> {
    processes
        .iter()
        .filter(|p| {
            p["image"]
                .as_str()
                .is_some_and(|image| published.contains(&image))
        })
        .filter(|p| {
            p["egress"].as_array().is_some_and(|egress| {
                egress
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|e| PUBLIC_TLS.contains(&e))
            })
        })
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect()
}

/// Each pod `docs` render for `process`, by where it renders: a Deployment's or Job's template,
/// or the deployment manager's `<process>.json` template.
fn pods(docs: &[Value], process: &str) -> Result<Vec<(String, Value)>> {
    let mut found = Vec::new();
    for doc in docs {
        let kind = doc["kind"].as_str().unwrap_or_default();
        if matches!(kind, "Deployment" | "Job")
            && doc["spec"]["template"]["metadata"]["labels"]["mandate.dev/process"] == process
        {
            found.push((format!("{kind} {process}"), doc["spec"]["template"].clone()));
        }
        if kind == "ConfigMap"
            && doc["metadata"]["name"] == TEMPLATES
            && let Some(text) = doc["data"][format!("{process}.json")].as_str()
        {
            let pod: Value = serde_json::from_str(text)
                .with_context(|| format!("the manager's template for {process}"))?;
            found.push((format!("the manager's {process} template"), pod));
        }
    }
    Ok(found)
}

/// What `pod`, named `at`, lacks of the CA bundle: its first container naming it in
/// `SSL_CERT_FILE` and mounting its directory read-only, and an init container copying it.
fn lacks(at: &str, pod: &Value) -> Vec<String> {
    let spec = &pod["spec"];
    let first = &spec["containers"][0];
    let mut problems = Vec::new();
    let named = first["env"].as_array().is_some_and(|env| {
        env.iter()
            .any(|e| e["name"] == "SSL_CERT_FILE" && e["value"] == CA_BUNDLE)
    });
    if !named {
        problems.push(format!("{at} does not name {CA_BUNDLE} in SSL_CERT_FILE"));
    }
    let mounted = first["volumeMounts"].as_array().is_some_and(|mounts| {
        mounts
            .iter()
            .any(|m| m["mountPath"] == CA_BUNDLE_DIR && m["readOnly"] == true)
    });
    if !mounted {
        problems.push(format!("{at} does not mount {CA_BUNDLE_DIR} read-only"));
    }
    let copied = spec["initContainers"].as_array().is_some_and(|inits| {
        inits.iter().any(|i| {
            i["command"].as_array().is_some_and(|words| {
                let joined: Vec<&str> = words.iter().filter_map(Value::as_str).collect();
                joined.join(" ").contains(COPY)
            })
        })
    });
    if !copied {
        problems.push(format!("{at} has no init container copying the CA bundle"));
    }
    problems
}

/// What the renders `renders` lack for the public-TLS processes of `published`: each must render
/// somewhere, and wherever it renders, have the bundle (DEC-2374 item 1).
pub fn problems(
    processes: &[Value],
    published: &[&str],
    renders: &[Vec<Value>],
) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    for process in public_tls(processes, published) {
        let mut rendered = 0usize;
        for docs in renders {
            for (at, pod) in pods(docs, &process)? {
                rendered = rendered.saturating_add(1);
                problems.extend(lacks(&at, &pod));
            }
        }
        if rendered == 0 {
            problems.push(format!(
                "{process} reaches a public TLS host but renders nowhere this check reads"
            ));
        }
    }
    Ok(problems)
}

/// `cargo xtask release ca-roots`: renders the workspace chart with every published image, with
/// the deployment manager and a control run and without the manager, and fails on any problem.
pub fn check(root: &Path) -> Result<()> {
    let chart = root.join("deploy/cell/charts/workspace");
    let text =
        std::fs::read_to_string(chart.join("processes.json")).context("reading processes.json")?;
    let processes: Value = serde_json::from_str(&text).context("parsing processes.json")?;
    let processes = processes["processes"]
        .as_array()
        .context("processes.json has no processes")?;
    let published = chart_image_keys();
    let mut base = vec![
        "template".to_owned(),
        "cell".to_owned(),
        chart.display().to_string(),
        "--namespace".to_owned(),
        "ws-ws1".to_owned(),
        "-f".to_owned(),
        root.join("deploy/cell/values/k3d.yaml")
            .display()
            .to_string(),
    ];
    for key in &published {
        base.push("--set".to_owned());
        base.push(format!(
            "images.{key}=ghcr.io/release/{key}@sha256:{}",
            "0".repeat(64)
        ));
    }
    let mut managed = base.clone();
    managed.extend([
        "--set".to_owned(),
        "run.id=caroots".to_owned(),
        "--set-json".to_owned(),
        r#"run.args=["workspace","open","--workspace","ws1"]"#.to_owned(),
    ]);
    let mut unmanaged = base;
    unmanaged.extend(["--set".to_owned(), "images.deploy-manager=".to_owned()]);
    let renders = [managed, unmanaged]
        .iter()
        .map(|args| render_with(args))
        .collect::<Result<Vec<_>>>()?;
    let found = problems(processes, &published, &renders)?;
    if !found.is_empty() {
        bail!(
            "a published process reaches a public TLS host without CA roots (DEC-2374):\n  {}",
            found.join("\n  ")
        );
    }
    eprintln!("    ca-roots: every published public-TLS process has the CA bundle (DEC-2374)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn processes() -> Vec<Value> {
        vec![
            json!({"name": "token-exchange", "image": "token-exchange", "egress": ["vault", "token-host"]}),
            json!({"name": "executor", "image": "executor", "egress": ["postgres", "broker"]}),
            json!({"name": "connect-check", "image": "executor", "egress": ["postgres"]}),
            json!({"name": "data", "image": "data", "egress": ["market-data"]}),
        ]
    }

    /// A pod with the bundle wired as #2420 wires it, or with `drop` taken out of it.
    fn pod(process: &str, drop: Option<&str>) -> Value {
        let mut pod = json!({
            "metadata": {"labels": {"mandate.dev/process": process}},
            "spec": {
                "initContainers": [{"name": "ca-bundle", "command": ["cp", "/etc/ssl/certs/ca-certificates.crt", CA_BUNDLE]}],
                "containers": [{
                    "name": process,
                    "env": [{"name": "SSL_CERT_FILE", "value": CA_BUNDLE}],
                    "volumeMounts": [{"name": "ca-bundle", "mountPath": CA_BUNDLE_DIR, "readOnly": true}],
                }],
            },
        });
        match drop {
            Some("env") => pod["spec"]["containers"][0]["env"] = json!([]),
            Some("writable") => {
                pod["spec"]["containers"][0]["volumeMounts"][0]["readOnly"] = json!(false)
            }
            Some("copier") => pod["spec"]["initContainers"] = json!([]),
            Some("sidecar") => {
                let wired = pod["spec"]["containers"][0].clone();
                pod["spec"]["containers"] = json!([{"name": "sidecar"}, wired]);
            }
            _ => {}
        }
        pod
    }

    fn deployment(process: &str, drop: Option<&str>) -> Value {
        json!({"kind": "Deployment", "metadata": {"name": process}, "spec": {"template": pod(process, drop)}})
    }

    fn templates(entries: &[(&str, Value)]) -> Value {
        let data: serde_json::Map<String, Value> = entries
            .iter()
            .map(|(name, pod)| (format!("{name}.json"), Value::String(pod.to_string())))
            .collect();
        json!({"kind": "ConfigMap", "metadata": {"name": TEMPLATES}, "data": data})
    }

    const PUBLISHED: [&str; 2] = ["token-exchange", "executor"];

    #[test]
    fn a_wired_release_has_no_problem_and_the_unpublished_and_private_are_not_checked() -> Result<()>
    {
        let managed = vec![templates(&[
            ("token-exchange", pod("token-exchange", None)),
            ("executor", pod("executor", None)),
        ])];
        let unmanaged = vec![
            deployment("token-exchange", None),
            deployment("executor", None),
        ];
        assert_eq!(
            problems(&processes(), &PUBLISHED, &[managed, unmanaged])?,
            Vec::<String>::new()
        );
        assert_eq!(
            public_tls(&processes(), &PUBLISHED),
            ["token-exchange", "executor"],
            "connect-check reaches no public host, and data is not published"
        );
        Ok(())
    }

    /// DEC-2374 item 1: a public-TLS process that lacks any part of the bundle, in either render,
    /// or renders nowhere, fails the release.
    #[test]
    fn each_missing_part_of_the_bundle_fails_the_release() -> Result<()> {
        for drop in ["env", "writable", "copier", "sidecar"] {
            let managed = vec![templates(&[
                ("token-exchange", pod("token-exchange", Some(drop))),
                ("executor", pod("executor", None)),
            ])];
            let found = problems(&processes(), &PUBLISHED, &[managed])?;
            assert!(
                !found.is_empty() && found.iter().all(|p| p.contains("token-exchange")),
                "{drop}: {found:?}"
            );
            let unmanaged = vec![
                deployment("token-exchange", None),
                deployment("executor", Some(drop)),
            ];
            let found = problems(&processes(), &PUBLISHED, &[unmanaged])?;
            assert!(
                !found.is_empty() && found.iter().all(|p| p.contains("executor")),
                "{drop}: {found:?}"
            );
        }
        let none = problems(
            &processes(),
            &PUBLISHED,
            &[vec![deployment("token-exchange", None)]],
        )?;
        assert_eq!(
            none,
            ["executor reaches a public TLS host but renders nowhere this check reads"]
        );
        Ok(())
    }

    /// The API's copier is a combined shell line (api.yaml), which the check reads by substring.
    #[test]
    fn a_copier_inside_a_longer_command_counts() {
        let mut combined = pod("api", None);
        combined["spec"]["initContainers"] = json!([{"name": "credentials", "command": ["bash", "-c", format!("head -c 32 /dev/urandom > /x && {COPY}")]}]);
        assert!(lacks("api", &combined).is_empty());
    }
}
