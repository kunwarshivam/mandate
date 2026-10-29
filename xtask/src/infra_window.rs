//! Staging's per-run control-plane window and the bootstrap it serves (E21-56 F2, DEC-2028; the
//! founder: DEC-2605 row 11). `cargo xtask infra apply` reads this runner's public address, masks
//! it, and opens the cluster's API to that one `/32` in the plan the founder approves. After the
//! apply it fetches credentials that expire within 900 seconds, writes their kubeconfig into
//! `$RUNNER_TEMP` at mode 0600, hands it and the workload secrets to lane GO's bootstrap, and
//! deletes it. The window is closed by the same command and again by `infra.yml`'s last step, and
//! each close is read back; a runner that dies before both is DEC-2028 item 3's residual.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::Ipv4Addr;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use crate::infra_preflight::{self, Answer, DO_API};
use crate::mask;

/// The echo service the runner asks for its own public address (DEC-2028 item 4).
const ADDRESS_ECHO: &str = "https://api.ipify.org";
/// How long the bootstrap's kubeconfig lives, in seconds.
const KUBECONFIG_TTL_SECONDS: u32 = 900;
/// What the control plane admits when neither the founder's CIDRs nor a window are set: TEST-NET-1,
/// which no internet host holds, as the `doks` module's fallback (DEC-2030 item 3).
const ADMITS_NOBODY: &str = "192.0.2.1/32";
/// The clusters DigitalOcean lists on one page; a listing must fit in one.
const CLUSTER_PAGE: u32 = 200;
/// How many times the close reads the cluster back before it reports the window may be open.
const CLOSE_READS: u32 = 6;
/// The wait between those reads.
const CLOSE_POLL_SECONDS: u64 = 5;
/// How far DigitalOcean's clock may run ahead of the runner's when the credentials' expiry is
/// checked.
const CLOCK_SKEW_SECONDS: i64 = 120;

/// Each environment's cluster region, as its root's `region` local sets it: the founder put all of
/// the infrastructure in the US West, so both run in sfo2 (DEC-2047).
fn cluster_region(_environment: &str) -> &'static str {
    "sfo2"
}

/// The cluster `infra` opens a window to, by environment: its DOKS name and its kubeconfig context,
/// named as `doctl` names it.
pub(crate) fn cluster_names(environment: &str) -> (String, String) {
    (
        format!("mandate-{environment}"),
        format!("do-{}-mandate-{environment}", cluster_region(environment)),
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

/// The HTTP layer the window and the kubeconfig go through, so tests can drive them against a fake
/// DigitalOcean.
pub(crate) trait Http {
    /// A request to a token-authorised JSON API.
    fn api(&self, method: &str, url: &str, token: &str, body: Option<&str>) -> Result<Answer>;
    /// A `GET` that takes no credential.
    fn get_public(&self, url: &str) -> Result<Answer>;
    /// Waits before the close's next read of the cluster.
    fn pause(&self);
    /// The current time, in Unix seconds.
    fn now(&self) -> Result<i64>;
}

/// The real HTTP layer: `curl`, with every credential on its standard input.
pub(crate) struct Curl;

impl Http for Curl {
    fn api(&self, method: &str, url: &str, token: &str, body: Option<&str>) -> Result<Answer> {
        infra_preflight::api(method, url, token, body)
    }

    fn get_public(&self, url: &str) -> Result<Answer> {
        infra_preflight::get_public(url)
    }

    fn pause(&self) {
        std::thread::sleep(Duration::from_secs(CLOSE_POLL_SECONDS));
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the real HTTP layer is the shell's clock; tests inject theirs through `Http`"
    )]
    fn now(&self) -> Result<i64> {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("the clock is before 1970")?;
        Ok(i64::try_from(since.as_secs())?)
    }
}

/// Reads this runner's public address, masks it and its `/32` before anything else can print
/// either, and returns the window.
pub(crate) fn open_window(http: &dyn Http) -> Result<String> {
    let answer = http.get_public(ADDRESS_ECHO)?;
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

/// What a close found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Window {
    /// The window was open, and the cluster now admits exactly the closed addresses.
    Closed,
    /// The cluster already admitted exactly the closed addresses, so nothing was sent.
    AlreadyClosed,
    /// The account holds no such cluster, so no window can be open on it.
    NoCluster,
}

/// Whether `cluster`'s control-plane firewall is on and admits exactly `addresses`, in any order.
fn admits_only(cluster: &Value, addresses: &[String]) -> bool {
    let firewall = cluster.get("control_plane_firewall");
    let enabled = firewall
        .and_then(|firewall| firewall.get("enabled"))
        .and_then(Value::as_bool)
        == Some(true);
    let mut current: Vec<&str> = firewall
        .and_then(|firewall| firewall.get("allowed_addresses"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut wanted: Vec<&str> = addresses.iter().map(String::as_str).collect();
    current.sort_unstable();
    wanted.sort_unstable();
    enabled && current == wanted
}

/// `name`'s cluster from DigitalOcean's listing, which must be one readable page that holds every
/// cluster, so a cluster on a later page or an unreadable answer is never taken for no cluster.
fn find_cluster(http: &dyn Http, token: &str, name: &str) -> Result<Option<Value>> {
    let listing = http.api(
        "GET",
        &format!("{DO_API}/kubernetes/clusters?per_page={CLUSTER_PAGE}"),
        token,
        None,
    )?;
    ensure!(
        listing.ok(),
        "DigitalOcean refused to list clusters (status {}; DIGITAL_OCEAN_API_KEY needs Kubernetes \
         read)",
        listing.status
    );
    let listing: Value = serde_json::from_str(&listing.body)
        .ok()
        .context("DigitalOcean's cluster listing is not JSON")?;
    let clusters = listing
        .get("kubernetes_clusters")
        .and_then(Value::as_array)
        .context("DigitalOcean's cluster listing holds no cluster list")?;
    let total = listing
        .get("meta")
        .and_then(|meta| meta.get("total"))
        .and_then(Value::as_u64)
        .context("DigitalOcean's cluster listing gives no total")?;
    ensure!(
        u64::try_from(clusters.len()).ok() == Some(total),
        "DigitalOcean's cluster listing holds {} of {total} clusters, so it cannot show the \
         cluster is absent",
        clusters.len()
    );
    Ok(clusters
        .iter()
        .find(|cluster| cluster.get("name").and_then(Value::as_str) == Some(name))
        .cloned())
}

/// Closes the window on `environment`'s cluster (DEC-2028 item 3): its control plane is set back to
/// [`closed_addresses`] through DigitalOcean's API, so closing needs only the DigitalOcean token and
/// the founder's CIDRs, never OpenTofu's state. The update is then read back until the cluster
/// admits exactly those addresses, and anything else is an error, never a success.
pub(crate) fn close_window(
    http: &dyn Http,
    token: &str,
    environment: &str,
    founder: Option<&str>,
) -> Result<Window> {
    let addresses = closed_addresses(founder)?;
    for address in &addresses {
        mask::add_mask(address);
    }
    let (name, _) = cluster_names(environment);
    let Some(cluster) = find_cluster(http, token, &name)? else {
        return Ok(Window::NoCluster);
    };
    if admits_only(&cluster, &addresses) {
        return Ok(Window::AlreadyClosed);
    }
    let id = cluster
        .get("id")
        .and_then(Value::as_str)
        .context("the cluster has no id")?;
    let url = format!("{DO_API}/kubernetes/clusters/{id}");
    let answer = http.api("PUT", &url, token, Some(&close_body(&cluster, &addresses)?))?;
    ensure!(
        answer.ok(),
        "DigitalOcean refused to close the control plane's window (status {}); the next run, or \
         an apply with no window, closes it",
        answer.status
    );
    for read in 0..CLOSE_READS {
        if read > 0 {
            http.pause();
        }
        let answer = http.api("GET", &url, token, None)?;
        let cluster: Option<Value> = answer
            .ok()
            .then(|| serde_json::from_str::<Value>(&answer.body).ok())
            .flatten()
            .and_then(|body| body.get("kubernetes_cluster").cloned());
        if cluster.is_some_and(|cluster| admits_only(&cluster, &addresses)) {
            return Ok(Window::Closed);
        }
    }
    bail!(
        "DigitalOcean accepted the close, but the control plane still does not admit exactly the \
         closed addresses; the window may be open, and the next run, or an apply with no window, \
         closes it"
    )
}

/// Opens the window on `environment`'s cluster outside a plan, for `infra.yml`'s `step-up` action
/// (DEC-2087), which runs no OpenTofu: the control plane is set to [`closed_addresses`] and the
/// runner's `cidr` through DigitalOcean's API, as [`close_window`] sets it back, and read back until
/// it admits exactly those. Answers the cluster's id, whose credentials the action fetches. A
/// missing cluster, a refused update, or one never applied is an error, and the action's close
/// runs however this ends.
pub(crate) fn open_window_now(
    http: &dyn Http,
    token: &str,
    environment: &str,
    founder: Option<&str>,
    cidr: &str,
) -> Result<String> {
    let mut addresses = closed_addresses(founder)?;
    for address in &addresses {
        mask::add_mask(address);
    }
    addresses.push(runner_cidr(cidr.strip_suffix("/32").unwrap_or(cidr))?);
    let (name, _) = cluster_names(environment);
    let cluster = find_cluster(http, token, &name)?
        .with_context(|| format!("DigitalOcean lists no cluster {name}, so no window can open"))?;
    let id = cluster
        .get("id")
        .and_then(Value::as_str)
        .context("the cluster has no id")?
        .to_owned();
    if admits_only(&cluster, &addresses) {
        return Ok(id);
    }
    let url = format!("{DO_API}/kubernetes/clusters/{id}");
    let answer = http.api("PUT", &url, token, Some(&close_body(&cluster, &addresses)?))?;
    ensure!(
        answer.ok(),
        "DigitalOcean refused to open the control plane's window (status {})",
        answer.status
    );
    for read in 0..CLOSE_READS {
        if read > 0 {
            http.pause();
        }
        let answer = http.api("GET", &url, token, None)?;
        let cluster: Option<Value> = answer
            .ok()
            .then(|| serde_json::from_str::<Value>(&answer.body).ok())
            .flatten()
            .and_then(|body| body.get("kubernetes_cluster").cloned());
        if cluster.is_some_and(|cluster| admits_only(&cluster, &addresses)) {
            return Ok(id);
        }
    }
    bail!("DigitalOcean accepted the window, but the control plane never admitted this runner")
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

/// Seconds since the Unix epoch for an RFC 3339 UTC time such as `2026-10-19T04:20:00.1Z`, its
/// fraction dropped; anything else is refused.
fn unix_seconds(text: &str) -> Result<i64> {
    let digits = |range: std::ops::Range<usize>| -> Result<i64> {
        let part = text.get(range).context("the time is too short")?;
        ensure!(
            part.chars().all(|c| c.is_ascii_digit()),
            "the time is not RFC 3339"
        );
        Ok(part.parse()?)
    };
    let shape = text.as_bytes();
    ensure!(
        shape.len() >= 20
            && shape[4] == b'-'
            && shape[7] == b'-'
            && shape[10] == b'T'
            && shape[13] == b':'
            && shape[16] == b':'
            && text.ends_with('Z'),
        "the time is not RFC 3339 in UTC"
    );
    let rest = &text[19..text.len() - 1];
    ensure!(
        rest.is_empty()
            || (rest.starts_with('.')
                && rest.len() > 1
                && rest[1..].chars().all(|c| c.is_ascii_digit())),
        "the time is not RFC 3339 in UTC"
    );
    let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    ensure!(
        (1..=12).contains(&month)
            && (1..=31).contains(&day)
            && hour < 24
            && minute < 60
            && second < 60,
        "the time is out of range"
    );
    let shifted = if month <= 2 { year - 1 } else { year };
    let era = shifted.div_euclid(400);
    let of_era = shifted - era * 400;
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    let days = era * 146_097 + of_cycle - 719_468;
    Ok(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// The kubeconfig for DigitalOcean's `credentials` answer: masks the token and any client key the
/// moment they are read, refuses a credential that lives longer than [`KUBECONFIG_TTL_SECONDS`]
/// past `now` (Unix seconds) or has already expired, and returns the kubeconfig as JSON, which
/// `kubectl` reads, with `context` as its current context.
pub(crate) fn kubeconfig_from_credentials(body: &str, context: &str, now: i64) -> Result<String> {
    let credentials: Value = serde_json::from_str(body)
        .ok()
        .context("DigitalOcean's cluster credentials are not JSON")?;
    let text = |key: &str| {
        credentials
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    };
    let token = text("token");
    let key = text("client_key_data");
    for secret in [token, key].into_iter().flatten() {
        mask::add_mask(secret);
    }
    let expires = unix_seconds(text("expires_at").context("the credentials give no expiry")?)?;
    ensure!(
        expires > now && expires - now <= i64::from(KUBECONFIG_TTL_SECONDS) + CLOCK_SKEW_SECONDS,
        "DigitalOcean's cluster credentials do not expire within {KUBECONFIG_TTL_SECONDS} seconds"
    );
    let server = text("server")
        .filter(|server| server.starts_with("https://"))
        .context("the credentials give no https server")?;
    let authority = text("certificate_authority_data")
        .context("the credentials give no certificate authority")?;
    let mut user = serde_json::Map::new();
    if let Some(token) = token {
        user.insert("token".to_owned(), json!(token));
    }
    if let (Some(certificate), Some(key)) = (text("client_certificate_data"), key) {
        user.insert("client-certificate-data".to_owned(), json!(certificate));
        user.insert("client-key-data".to_owned(), json!(key));
    }
    ensure!(
        !user.is_empty(),
        "the credentials give no token or client key"
    );
    Ok(json!({
        "apiVersion": "v1",
        "kind": "Config",
        "clusters": [{
            "name": context,
            "cluster": { "server": server, "certificate-authority-data": authority },
        }],
        "users": [{ "name": context, "user": user }],
        "contexts": [{ "name": context, "context": { "cluster": context, "user": context } }],
        "current-context": context,
    })
    .to_string())
}

/// Fetches credentials for `cluster_id` that expire in [`KUBECONFIG_TTL_SECONDS`], masks them, and
/// writes the kubeconfig for `context` into `dir` for this run alone.
pub(crate) fn fetch_kubeconfig(
    http: &dyn Http,
    token: &str,
    cluster_id: &str,
    context: &str,
    dir: &Path,
    run: &str,
) -> Result<Kubeconfig> {
    let answer = http.api(
        "GET",
        &format!(
            "{DO_API}/kubernetes/clusters/{cluster_id}/credentials?expiry_seconds={KUBECONFIG_TTL_SECONDS}"
        ),
        token,
        None,
    )?;
    ensure!(
        answer.ok(),
        "DigitalOcean refused the cluster's credentials (status {})",
        answer.status
    );
    let text = kubeconfig_from_credentials(&answer.body, context, http.now()?)?;
    Kubeconfig::write(dir, &format!("mandate-kubeconfig-{run}"), &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::os::unix::fs::PermissionsExt;

    /// DEC-2028 items 1 and 4: only one public IPv4 address opens a window, as its `/32`.
    #[test]
    fn only_a_public_address_opens_a_window() -> Result<()> {
        assert_eq!(runner_cidr("20.42.7.9\n")?, "20.42.7.9/32");
        for public in ["20.0.7.9", "100.63.1.1", "100.128.0.1", "20.64.0.1"] {
            assert_eq!(
                runner_cidr(public)?,
                format!("{public}/32"),
                "{public} is public"
            );
        }
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

    /// A fake DigitalOcean: each request must be the next one scripted, and gets its answer.
    struct Fake {
        script: RefCell<VecDeque<(&'static str, String, u16, String)>>,
        bodies: RefCell<Vec<String>>,
        pauses: Cell<u32>,
    }

    impl Fake {
        fn new(script: Vec<(&'static str, &str, u16, String)>) -> Self {
            Self {
                script: RefCell::new(
                    script
                        .into_iter()
                        .map(|(method, url, status, body)| (method, url.to_owned(), status, body))
                        .collect(),
                ),
                bodies: RefCell::new(Vec::new()),
                pauses: Cell::new(0),
            }
        }

        fn next(&self, method: &str, url: &str) -> Result<Answer> {
            let (expected, at, status, body) = self
                .script
                .borrow_mut()
                .pop_front()
                .context("an unscripted request")?;
            assert_eq!((method, url), (expected, at.as_str()), "the next request");
            Ok(Answer { status, body })
        }

        fn done(&self) -> bool {
            self.script.borrow().is_empty()
        }
    }

    impl Http for Fake {
        fn api(&self, method: &str, url: &str, token: &str, body: Option<&str>) -> Result<Answer> {
            assert_eq!(token, "dop_v1_t", "the token is passed on");
            self.bodies.borrow_mut().extend(body.map(str::to_owned));
            self.next(method, url)
        }

        fn get_public(&self, url: &str) -> Result<Answer> {
            self.next("GET", url)
        }

        fn pause(&self) {
            self.pauses.set(self.pauses.get() + 1);
        }

        fn now(&self) -> Result<i64> {
            Ok(NOW)
        }
    }

    const LIST: &str = "https://api.digitalocean.com/v2/kubernetes/clusters?per_page=200";
    const ONE: &str = "https://api.digitalocean.com/v2/kubernetes/clusters/c0ffee";

    fn cluster(enabled: bool, addresses: &[&str]) -> Value {
        json!({
            "id": "c0ffee",
            "name": "mandate-staging",
            "tags": [],
            "control_plane_firewall": { "enabled": enabled, "allowed_addresses": addresses },
        })
    }

    fn listing(clusters: &[Value]) -> String {
        json!({ "kubernetes_clusters": clusters, "meta": { "total": clusters.len() } }).to_string()
    }

    fn one(cluster: &Value) -> String {
        json!({ "kubernetes_cluster": cluster }).to_string()
    }

    /// The real HTTP layer reads the real clock and waits between the close's reads.
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "the test times the real layer's wait against the real clock"
    )]
    fn the_real_layer_reads_the_clock_and_waits() -> Result<()> {
        assert!(Curl.now()? > 1_767_225_600, "after 2026-01-01");
        let started = std::time::Instant::now();
        Curl.pause();
        assert!(started.elapsed() >= Duration::from_secs(CLOSE_POLL_SECONDS));
        Ok(())
    }

    /// The review's B3: the window opens only on the echo's public address, as its `/32`.
    #[test]
    fn the_window_opens_on_the_echoed_address() -> Result<()> {
        let echo =
            |status, body: &str| Fake::new(vec![("GET", ADDRESS_ECHO, status, body.to_owned())]);
        assert_eq!(open_window(&echo(200, "20.42.7.9\n"))?, "20.42.7.9/32");
        assert!(
            open_window(&echo(503, "20.42.7.9")).is_err(),
            "a failed echo opens nothing"
        );
        assert!(
            open_window(&echo(200, "10.0.0.1")).is_err(),
            "a private address opens nothing"
        );
        Ok(())
    }

    /// The review's B2 and B3: an open window is closed and read back; one already closed is left.
    #[test]
    fn an_open_window_is_closed_and_read_back() -> Result<()> {
        let open = cluster(true, &["20.42.7.9/32", "192.0.2.1/32"]);
        let closed = cluster(true, &["192.0.2.1/32"]);
        let fake = Fake::new(vec![
            ("GET", LIST, 200, listing(std::slice::from_ref(&open))),
            ("PUT", ONE, 202, String::new()),
            ("GET", ONE, 200, one(&open)),
            ("GET", ONE, 200, one(&closed)),
        ]);
        assert_eq!(
            close_window(&fake, "dop_v1_t", "staging", None)?,
            Window::Closed
        );
        assert!(
            fake.done(),
            "the close read the cluster back until it was closed"
        );
        assert_eq!(fake.pauses.get(), 1, "it waited between the reads");
        let sent: Value = serde_json::from_str(&fake.bodies.borrow()[0])?;
        assert_eq!(
            sent["control_plane_firewall"],
            json!({ "enabled": true, "allowed_addresses": ["192.0.2.1/32"] })
        );
        let founder = cluster(true, &["198.51.100.0/24", "203.0.113.7/32"]);
        let fake = Fake::new(vec![("GET", LIST, 200, listing(&[founder]))]);
        assert_eq!(
            close_window(
                &fake,
                "dop_v1_t",
                "staging",
                Some("[\"203.0.113.7/32\", \"198.51.100.0/24\"]")
            )?,
            Window::AlreadyClosed,
            "the same addresses in another order are already closed"
        );
        assert!(fake.done(), "nothing was sent");
        Ok(())
    }

    /// The step-up action's window: the closed addresses and the runner's `/32`, read back; an
    /// already open one is left; a missing cluster or an update never applied is an error.
    #[test]
    fn the_step_up_window_opens_on_the_runner_and_is_read_back() -> Result<()> {
        let closed = cluster(true, &["192.0.2.1/32"]);
        let open = cluster(true, &["192.0.2.1/32", "20.42.7.9/32"]);
        let fake = Fake::new(vec![
            ("GET", LIST, 200, listing(std::slice::from_ref(&closed))),
            ("PUT", ONE, 202, String::new()),
            ("GET", ONE, 200, one(&closed)),
            ("GET", ONE, 200, one(&open)),
        ]);
        assert_eq!(
            open_window_now(&fake, "dop_v1_t", "staging", None, "20.42.7.9/32")?,
            "c0ffee"
        );
        assert!(fake.done());
        let sent: Value = serde_json::from_str(&fake.bodies.borrow()[0])?;
        assert_eq!(
            sent["control_plane_firewall"],
            json!({ "enabled": true, "allowed_addresses": ["192.0.2.1/32", "20.42.7.9/32"] })
        );
        let already = Fake::new(vec![(
            "GET",
            LIST,
            200,
            listing(std::slice::from_ref(&open)),
        )]);
        assert_eq!(
            open_window_now(&already, "dop_v1_t", "staging", None, "20.42.7.9/32")?,
            "c0ffee"
        );
        assert!(already.done(), "nothing was sent");
        let none = Fake::new(vec![("GET", LIST, 200, listing(&[]))]);
        assert!(open_window_now(&none, "dop_v1_t", "staging", None, "20.42.7.9/32").is_err());
        let mut script = vec![
            ("GET", LIST, 200, listing(std::slice::from_ref(&closed))),
            ("PUT", ONE, 202, String::new()),
        ];
        script.extend((0..CLOSE_READS).map(|_| ("GET", ONE, 200, one(&closed))));
        let ignored = Fake::new(script);
        assert!(open_window_now(&ignored, "dop_v1_t", "staging", None, "20.42.7.9/32").is_err());
        assert!(ignored.done());
        assert!(
            open_window_now(&Fake::new(vec![]), "t", "staging", None, "10.0.0.1/32").is_err(),
            "a private address opens nothing"
        );
        Ok(())
    }

    /// The review's B3: a disabled firewall is never taken as closed, even with the right addresses.
    #[test]
    fn a_disabled_firewall_is_not_closed() -> Result<()> {
        let disabled = cluster(false, &["192.0.2.1/32"]);
        let fake = Fake::new(vec![
            ("GET", LIST, 200, listing(&[disabled])),
            ("PUT", ONE, 200, String::new()),
            ("GET", ONE, 200, one(&cluster(true, &["192.0.2.1/32"]))),
        ]);
        assert_eq!(
            close_window(&fake, "dop_v1_t", "staging", None)?,
            Window::Closed
        );
        assert!(fake.done());
        Ok(())
    }

    /// The review's B3: a refused update, or one DigitalOcean accepts but never applies, is an
    /// error.
    #[test]
    fn an_unapplied_close_is_an_error() {
        let open = cluster(true, &["20.42.7.9/32", "192.0.2.1/32"]);
        let refused = Fake::new(vec![
            ("GET", LIST, 200, listing(std::slice::from_ref(&open))),
            ("PUT", ONE, 422, String::new()),
        ]);
        assert!(close_window(&refused, "dop_v1_t", "staging", None).is_err());
        assert!(refused.done());
        let mut script = vec![
            ("GET", LIST, 200, listing(std::slice::from_ref(&open))),
            ("PUT", ONE, 202, String::new()),
            ("GET", ONE, 500, one(&cluster(true, &["192.0.2.1/32"]))),
            ("GET", ONE, 200, "not json".to_owned()),
        ];
        script.extend((2..CLOSE_READS).map(|_| ("GET", ONE, 200, one(&open))));
        let ignored = Fake::new(script);
        assert!(close_window(&ignored, "dop_v1_t", "staging", None).is_err());
        assert!(ignored.done(), "every read was made");
        assert_eq!(ignored.pauses.get(), CLOSE_READS - 1);
    }

    /// The review's B2: only a full, readable listing shows the cluster is absent.
    #[test]
    fn only_a_full_listing_shows_no_cluster() -> Result<()> {
        let other = json!({ "id": "beef", "name": "mandate-production" });
        let fake = Fake::new(vec![(
            "GET",
            LIST,
            200,
            listing(std::slice::from_ref(&other)),
        )]);
        assert_eq!(
            close_window(&fake, "dop_v1_t", "staging", None)?,
            Window::NoCluster
        );
        for (status, body) in [
            (403, listing(&[])),
            (200, "not json".to_owned()),
            (200, json!({ "meta": { "total": 0 } }).to_string()),
            (200, json!({ "kubernetes_clusters": [] }).to_string()),
            (
                200,
                json!({ "kubernetes_clusters": [other], "meta": { "total": 201 } }).to_string(),
            ),
        ] {
            let fake = Fake::new(vec![("GET", LIST, status, body.clone())]);
            assert!(
                close_window(&fake, "dop_v1_t", "staging", None).is_err(),
                "{status} {body} is not proof of no cluster"
            );
        }
        Ok(())
    }

    fn credentials(expires_at: &str) -> String {
        json!({
            "server": "https://c0ffee.k8s.ondigitalocean.com",
            "certificate_authority_data": "Q0E=",
            "client_certificate_data": null,
            "client_key_data": null,
            "token": "dop_v1_kube",
            "expires_at": expires_at,
        })
        .to_string()
    }

    /// 2026-10-19T04:00:00Z in Unix seconds.
    const NOW: i64 = 1_792_382_400;

    /// The review's nits: the kubeconfig carries the credentials' token under the expected context,
    /// and credentials that outlive 900 seconds, or have expired, are refused.
    #[test]
    fn the_kubeconfig_is_short_lived() -> Result<()> {
        assert_eq!(unix_seconds("2026-10-19T04:00:00Z")?, NOW);
        assert_eq!(unix_seconds("2024-02-29T23:59:59.5Z")?, 1_709_251_199);
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z")?, 0);
        for refused in [
            "2026-10-19T04:00:00",
            "2026-10-19T04:00:00+00:00",
            "2026-10-19 04:00:00Z",
            "2026-13-19T04:00:00Z",
            "2026-10-19T04:00:00.Z",
            "2026-10-19T24:00:00Z",
            "",
        ] {
            assert!(unix_seconds(refused).is_err(), "{refused:?}");
        }
        let text =
            kubeconfig_from_credentials(&credentials("2026-10-19T04:15:00Z"), "do-nyc3-x", NOW)?;
        let config: Value = serde_json::from_str(&text)?;
        assert_eq!(config["current-context"], "do-nyc3-x");
        assert_eq!(
            config["users"][0]["user"],
            json!({ "token": "dop_v1_kube" })
        );
        assert_eq!(
            config["clusters"][0]["cluster"]["server"],
            "https://c0ffee.k8s.ondigitalocean.com"
        );
        assert!(
            kubeconfig_from_credentials(&credentials("2026-10-19T04:17:00Z"), "do-nyc3-x", NOW)
                .is_ok(),
            "the clock may be two minutes apart"
        );
        for expiry in [
            "2026-10-19T04:17:01Z",
            "2026-10-19T04:00:00Z",
            "2026-10-20T04:00:00Z",
        ] {
            assert!(
                kubeconfig_from_credentials(&credentials(expiry), "do-nyc3-x", NOW).is_err(),
                "credentials expiring {expiry} are refused"
            );
        }
        let no_token = credentials("2026-10-19T04:15:00Z").replace("dop_v1_kube", "");
        assert!(kubeconfig_from_credentials(&no_token, "do-nyc3-x", NOW).is_err());
        let plain = credentials("2026-10-19T04:15:00Z").replace("https://", "http://");
        assert!(kubeconfig_from_credentials(&plain, "do-nyc3-x", NOW).is_err());
        Ok(())
    }

    /// The review's B3: the credentials are fetched for 900 seconds and written privately; a
    /// refusal writes nothing.
    #[test]
    fn the_kubeconfig_is_fetched_for_900_seconds() -> Result<()> {
        let url = "https://api.digitalocean.com/v2/kubernetes/clusters/c0ffee/credentials?expiry_seconds=900";
        let dir = std::env::temp_dir().join(format!("mandate-fetch-test-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let fetched = Fake::new(vec![("GET", url, 200, credentials("2026-10-19T04:15:00Z"))]);
        let path = {
            let kubeconfig = fetch_kubeconfig(&fetched, "dop_v1_t", "c0ffee", "ctx", &dir, "0")?;
            let mode = fs::metadata(kubeconfig.path())?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "readable by this user alone");
            let config: Value = serde_json::from_str(&fs::read_to_string(kubeconfig.path())?)?;
            assert_eq!(config["current-context"], "ctx");
            kubeconfig.path().to_path_buf()
        };
        assert_eq!(path, dir.join("mandate-kubeconfig-0"));
        assert!(!path.exists(), "the kubeconfig is deleted with its guard");
        let refused = Fake::new(vec![("GET", url, 403, String::new())]);
        assert!(fetch_kubeconfig(&refused, "dop_v1_t", "c0ffee", "ctx", &dir, "1").is_err());
        assert!(
            !dir.join("mandate-kubeconfig-1").exists(),
            "a refusal writes nothing"
        );
        let expired = Fake::new(vec![("GET", url, 200, credentials("2020-01-01T00:00:00Z"))]);
        assert!(fetch_kubeconfig(&expired, "dop_v1_t", "c0ffee", "ctx", &dir, "2").is_err());
        assert!(!dir.join("mandate-kubeconfig-2").exists());
        fs::remove_dir_all(&dir)?;
        Ok(())
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

    /// Each environment's cluster and context name the region its root plans the cluster in.
    #[test]
    fn the_cluster_names_match_the_roots() {
        assert_eq!(
            cluster_names("staging"),
            (
                "mandate-staging".to_owned(),
                "do-sfo2-mandate-staging".to_owned()
            )
        );
        for (environment, root) in [
            (
                "staging",
                include_str!("../../deploy/tofu/environments/staging/main.tf"),
            ),
            (
                "production",
                include_str!("../../deploy/tofu/environments/production/main.tf"),
            ),
        ] {
            let region = root
                .lines()
                .map(str::trim)
                .find_map(|line| line.strip_prefix("region"))
                .and_then(|rest| rest.split('"').nth(1));
            assert_eq!(
                region,
                Some(cluster_region(environment)),
                "{environment}'s root plans its cluster where the context says"
            );
        }
    }
}
