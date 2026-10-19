//! Staging's per-run control-plane window and the bootstrap it serves (E21-56 F2, DEC-2028; the
//! founder: DEC-2605 row 11). `cargo xtask infra apply` reads this runner's public address, masks
//! it, and opens the cluster's API to that one `/32` in the plan the founder approves. After the
//! apply it fetches a kubeconfig that expires in 900 seconds into `$RUNNER_TEMP` at mode 0600,
//! hands it and the workload secrets to lane GO's bootstrap, and deletes it. The window is closed
//! however the run ends, by the same command and again by `infra.yml`'s last step.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::Ipv4Addr;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use crate::infra_preflight::{self, DO_API};
use crate::mask;

/// The echo service the runner asks for its own public address (DEC-2028 item 4).
const ADDRESS_ECHO: &str = "https://api.ipify.org";
/// How long the bootstrap's kubeconfig lives, in seconds.
const KUBECONFIG_TTL_SECONDS: u32 = 900;
/// What the control plane admits when neither the founder's CIDRs nor a window are set: TEST-NET-1,
/// which no internet host holds, as the `doks` module's fallback (DEC-2030 item 3).
const ADMITS_NOBODY: &str = "192.0.2.1/32";

/// The cluster `infra` opens a window to, by environment: its DOKS name and its kubeconfig context.
pub(crate) fn cluster_names(environment: &str) -> (String, String) {
    (
        format!("mandate-{environment}"),
        format!("do-nyc3-mandate-{environment}"),
    )
}

/// The runner's window as a `/32`, from the echo service's answer, if that answer is one public
/// IPv4 address: the rule the `doks` module's `bootstrap_runner_cidr` validates, and the
/// documentation ranges too, which no runner holds. Anything else is refused, so a lying or broken
/// echo opens nothing it should not.
pub(crate) fn runner_cidr(answer: &str) -> Result<String> {
    let text = answer.trim();
    let address: Ipv4Addr = text
        .parse()
        .ok()
        .filter(|_| {
            text.split('.')
                .all(|octet| octet == "0" || !octet.starts_with('0'))
        })
        .context("the address echo did not answer one IPv4 address")?;
    let [first, second, third, _] = address.octets();
    let shared = first == 100 && (64..=127).contains(&second);
    let documentation = matches!(
        (first, second, third),
        (192, 0, 2) | (198, 51, 100) | (203, 0, 113)
    );
    ensure!(
        !(address.is_private()
            || address.is_loopback()
            || address.is_link_local()
            || address.is_unspecified()
            || address.is_multicast()
            || address.is_broadcast()
            || first == 0
            || first >= 224
            || shared
            || documentation),
        "the address echo answered an address no GitHub runner holds"
    );
    Ok(format!("{address}/32"))
}

/// Reads this runner's public address, masks it and its `/32` before anything else can print
/// either, and returns the window.
pub(crate) fn open_window() -> Result<String> {
    let answer = infra_preflight::get_public(ADDRESS_ECHO)?;
    ensure!(answer.ok(), "the address echo did not answer");
    let address = answer.body.trim().to_owned();
    mask::add_mask(&address);
    let cidr = runner_cidr(&address)?;
    mask::add_mask(&cidr);
    Ok(cidr)
}

/// What the control plane admits once the window is closed: the founder's CIDRs from
/// `KUBE_API_ALLOWED_CIDRS` as `infra.yml` hands them on, or [`ADMITS_NOBODY`] when there are
/// none, as the `doks` module computes it, so a closed window leaves no drift for the next plan.
pub(crate) fn closed_addresses(founder: Option<&str>) -> Result<Vec<String>> {
    if crate::names_nothing(founder) {
        return Ok(vec![ADMITS_NOBODY.to_owned()]);
    }
    let listed: Vec<String> = serde_json::from_str(founder.unwrap_or_default())
        .ok()
        .context("KUBE_API_ALLOWED_CIDRS is not a JSON list of strings")?;
    Ok(listed)
}

/// The update DigitalOcean's API takes to set `cluster`'s control-plane firewall to `addresses`
/// alone, carrying the cluster's own name, user tags and upgrade settings so nothing else changes.
/// DigitalOcean's own `k8s` tags are left to it.
pub(crate) fn close_body(cluster: &Value, addresses: &[String]) -> Result<String> {
    let name = cluster
        .get("name")
        .and_then(Value::as_str)
        .context("the cluster has no name")?;
    let tags: Vec<&str> = cluster
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|tag| *tag != "k8s" && !tag.starts_with("k8s:"))
        .collect();
    let flag = |key: &str| cluster.get(key).and_then(Value::as_bool).unwrap_or(false);
    Ok(json!({
        "name": name,
        "tags": tags,
        "auto_upgrade": flag("auto_upgrade"),
        "surge_upgrade": flag("surge_upgrade"),
        "ha": flag("ha"),
        "control_plane_firewall": { "enabled": true, "allowed_addresses": addresses },
    })
    .to_string())
}

/// Closes the window on `environment`'s cluster (DEC-2028 item 3): its control plane is set back to
/// [`closed_addresses`] through DigitalOcean's API, so closing needs only the DigitalOcean token and
/// the founder's CIDRs, never OpenTofu's state. A cluster that does not exist, or already admits
/// exactly those addresses, is left as it is. Returns whether the cluster exists.
pub(crate) fn close_window(token: &str, environment: &str, founder: Option<&str>) -> Result<bool> {
    let addresses = closed_addresses(founder)?;
    for address in &addresses {
        mask::add_mask(address);
    }
    let (name, _) = cluster_names(environment);
    let listing = infra_preflight::api(
        "GET",
        &format!("{DO_API}/kubernetes/clusters?per_page=200"),
        token,
        None,
    )?;
    ensure!(
        listing.ok(),
        "DigitalOcean refused to list clusters (DIGITAL_OCEAN_API_KEY needs Kubernetes read)"
    );
    let listing: Value = serde_json::from_str(&listing.body).unwrap_or_default();
    let Some(cluster) = listing
        .get("kubernetes_clusters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|cluster| cluster.get("name").and_then(Value::as_str) == Some(name.as_str()))
    else {
        return Ok(false);
    };
    let current: Vec<&str> = cluster
        .get("control_plane_firewall")
        .and_then(|firewall| firewall.get("allowed_addresses"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if current == addresses.iter().map(String::as_str).collect::<Vec<_>>() {
        return Ok(true);
    }
    let id = cluster
        .get("id")
        .and_then(Value::as_str)
        .context("the cluster has no id")?;
    let answer = infra_preflight::api(
        "PUT",
        &format!("{DO_API}/kubernetes/clusters/{id}"),
        token,
        Some(&close_body(cluster, &addresses)?),
    )?;
    ensure!(
        answer.ok(),
        "DigitalOcean refused to close the control plane's window (status {}); the next run, or \
         an apply with no window, closes it",
        answer.status
    );
    Ok(true)
}

/// Every `token:` value in a kubeconfig, which the bootstrap's log must never show.
pub(crate) fn kubeconfig_secrets(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("token:"))
        .map(|value| value.trim().trim_matches('"').to_owned())
        .filter(|value| !value.is_empty())
        .collect()
}

/// A kubeconfig on disk, readable by this user alone, deleted when dropped: on success, an error
/// and a panic alike.
pub(crate) struct Kubeconfig {
    path: PathBuf,
}

impl Kubeconfig {
    /// Writes `text` to a new file `name` in `dir`, at mode 0600 from its creation, refusing a file
    /// that already exists.
    pub(crate) fn write(dir: &Path, name: &str, text: &str) -> Result<Self> {
        let path = dir.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("creating the kubeconfig in {}", dir.display()))?;
        let kubeconfig = Self { path };
        file.write_all(text.as_bytes())
            .context("writing the kubeconfig")?;
        Ok(kubeconfig)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Kubeconfig {
    fn drop(&mut self) {
        if fs::remove_file(&self.path).is_err() {
            let _ = writeln!(
                std::io::stderr(),
                "    the bootstrap's kubeconfig was not deleted; it expires {KUBECONFIG_TTL_SECONDS} \
                 seconds after it was made"
            );
        }
    }
}

/// Fetches `cluster_id`'s kubeconfig, valid for [`KUBECONFIG_TTL_SECONDS`], masks its tokens and the
/// whole text, and writes it into `dir` for this run alone.
pub(crate) fn fetch_kubeconfig(
    token: &str,
    cluster_id: &str,
    dir: &Path,
    run: &str,
) -> Result<Kubeconfig> {
    let answer = infra_preflight::api(
        "GET",
        &format!(
            "{DO_API}/kubernetes/clusters/{cluster_id}/kubeconfig?expiry_seconds={KUBECONFIG_TTL_SECONDS}"
        ),
        token,
        None,
    )?;
    if !answer.ok() {
        bail!(
            "DigitalOcean refused the cluster's kubeconfig (status {})",
            answer.status
        );
    }
    for secret in kubeconfig_secrets(&answer.body) {
        mask::add_mask(&secret);
    }
    mask::add_mask(&answer.body);
    Kubeconfig::write(dir, &format!("mandate-kubeconfig-{run}"), &answer.body)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::os::unix::fs::PermissionsExt;

    /// DEC-2028 items 1 and 4: only one public IPv4 address opens a window, as its `/32`.
    #[test]
    fn only_a_public_address_opens_a_window() -> Result<()> {
        assert_eq!(runner_cidr("20.42.7.9\n")?, "20.42.7.9/32");
        for refused in [
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "0.1.2.3",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "192.0.2.7",
            "198.51.100.7",
            "203.0.113.7",
            "20.042.7.9",
            "20.42.7.9/32",
            "2001:db8::1",
            "20.42.7.9 20.42.7.10",
            "",
            "<html>",
        ] {
            assert!(runner_cidr(refused).is_err(), "{refused:?} opens nothing");
        }
        Ok(())
    }

    /// DEC-2028 item 3: a closed window leaves the founder's CIDRs, or TEST-NET-1 when there are
    /// none, as the `doks` module computes it.
    #[test]
    fn a_closed_window_leaves_what_the_module_would() -> Result<()> {
        for nothing in [None, Some(""), Some("[]"), Some(" [ ] ")] {
            assert_eq!(closed_addresses(nothing)?, ["192.0.2.1/32"], "{nothing:?}");
        }
        assert_eq!(
            closed_addresses(Some("[\"203.0.113.7/32\", \"198.51.100.0/24\"]"))?,
            ["203.0.113.7/32", "198.51.100.0/24"]
        );
        assert!(closed_addresses(Some("203.0.113.7/32")).is_err());
        Ok(())
    }

    /// The close keeps the cluster's name, user tags and upgrade settings, drops DigitalOcean's
    /// own `k8s` tags, and sets the firewall on with the given addresses alone.
    #[test]
    fn the_close_changes_only_the_firewall() -> Result<()> {
        let cluster = json!({
            "id": "c0ffee",
            "name": "mandate-staging",
            "tags": ["k8s", "k8s:c0ffee", "mandate-staging-nodes"],
            "auto_upgrade": false,
            "surge_upgrade": true,
            "ha": false,
            "control_plane_firewall": { "enabled": true, "allowed_addresses": ["20.42.7.9/32"] },
        });
        let body: Value =
            serde_json::from_str(&close_body(&cluster, &["192.0.2.1/32".to_owned()])?)?;
        assert_eq!(
            body,
            json!({
                "name": "mandate-staging",
                "tags": ["mandate-staging-nodes"],
                "auto_upgrade": false,
                "surge_upgrade": true,
                "ha": false,
                "control_plane_firewall": { "enabled": true, "allowed_addresses": ["192.0.2.1/32"] },
            })
        );
        assert!(
            close_body(&json!({}), &[]).is_err(),
            "a cluster with no name is refused"
        );
        Ok(())
    }

    /// Every token in a kubeconfig is found, so it is masked before the file is written.
    #[test]
    fn every_kubeconfig_token_is_found() {
        let text = "apiVersion: v1\nusers:\n- name: a\n  user:\n    token: dop_v1_abc\n- name: b\n  user:\n    token: \"quoted\"\n";
        assert_eq!(kubeconfig_secrets(text), ["dop_v1_abc", "quoted"]);
        assert!(kubeconfig_secrets("apiVersion: v1\n").is_empty());
    }

    /// The kubeconfig is created at 0600, refuses an existing file, and is deleted when dropped.
    #[test]
    fn the_kubeconfig_is_private_and_deleted() -> Result<()> {
        let dir =
            std::env::temp_dir().join(format!("mandate-kubeconfig-test-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let path = {
            let kubeconfig = Kubeconfig::write(&dir, "k", "token: x\n")?;
            let mode = fs::metadata(kubeconfig.path())?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "readable by this user alone");
            assert!(
                Kubeconfig::write(&dir, "k", "again").is_err(),
                "an existing file is refused"
            );
            kubeconfig.path().to_path_buf()
        };
        assert!(!path.exists(), "dropping the guard deletes the file");
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    /// Each environment's cluster and context are the ones lane GO's bootstrap confirms.
    #[test]
    fn the_cluster_names_match_the_roots() {
        assert_eq!(
            cluster_names("staging"),
            (
                "mandate-staging".to_owned(),
                "do-nyc3-mandate-staging".to_owned()
            )
        );
    }
}
