//! Staging's vault provisioned from CI (E21-32, DEC-2630): after `vault_init`, inside the same
//! window, staging's apply gives every process its AppRole credentials as a Kubernetes Secret, so
//! nobody runs `mandate vault provision` by hand. When every Secret is already there it changes
//! nothing. Otherwise it makes a root token from two of the recovery keys `vault_init` stored,
//! through the vault's unauthenticated root-generation endpoints on a listener only the pod's
//! loopback reaches, runs `mandate vault provision --secrets-namespace` with that token on its
//! standard input, applies the Secrets it prints through `kubectl`'s standard input, and revokes
//! the token, however the step ends. Every key, one-time pad, and token is masked the moment it
//! exists and is never an argument, an environment variable, a file, or a log line.
//!
//! Each provisioning sets Kubernetes auth up too, at the `kubernetes` Service's address
//! (DEC-2062), and records that address in the ConfigMap `vault-kubernetes-auth`, which holds
//! nothing secret. A vault whose Secrets are there but whose ConfigMap is missing or names another
//! address gains Kubernetes auth alone, `--kubernetes-only`, which mints no secret id, so no
//! credential in use is rotated.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddrV4;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

use crate::gitops::WORKSPACE_NAMESPACE;
use crate::mask;
use crate::vault_init::{Kubectl, RECOVERY_NAMESPACE, RECOVERY_SECRET};

/// The vault's pod and container, as the chart names them.
const POD: &str = "vault-0";
const CONTAINER: &str = "vault";
/// The listener only the pod's loopback reaches, the one that answers root generation.
pub(crate) const GENERATE_ROOT_ADDRESS: &str = "-address=https://127.0.0.1:8210";
/// How many recovery keys `vault_init` asks for to make a root token (`-recovery-threshold=2`).
const THRESHOLD: usize = 2;
/// The label every provisioned Secret carries (`mandate_vault::kube::MANAGED_BY_LABEL`).
const MANAGED_BY: &str = "app.kubernetes.io/managed-by=mandate-vault-provision";
/// The Secrets of the identities that are not per connection, as `mandate_vault::kube::secret_name`
/// names them, the role provisioner's among them, which `--secrets-namespace` issues (DEC-2350). A
/// vault provisioned before it was listed lacks it, so the next apply provisions once more.
const PLATFORM_SECRETS: [&str; 8] = [
    "vault-approle-api",
    "vault-approle-connection-manager",
    "vault-approle-runtime",
    "vault-approle-secret-import",
    "vault-approle-model-gateway",
    "vault-approle-news-reader",
    "vault-approle-email-sender",
    "vault-approle-provisioner",
];
/// The ConfigMap that records the API server address the vault's Kubernetes auth was last set up
/// at, and its key.
const MARKER: &str = "vault-kubernetes-auth";
const MARKER_KEY: &str = "host";
/// Revokes the token on standard input, as `vault_init` revokes its own.
const REVOKE: [&str; 3] = [
    "sh",
    "-c",
    "BAO_TOKEN=\"$(cat)\" exec bao token revoke -self",
];

/// `mandate vault provision`, given its arguments after `vault` and its standard input.
pub(crate) type Provision<'a> = &'a dyn Fn(&[String], &str) -> Result<(bool, String)>;

/// `mandate vault put <secret>`, given the secret's name and its standard input.
pub(crate) type Put<'a> = &'a dyn Fn(&str, &str) -> Result<(bool, String)>;

/// What the step needs: `kubectl` against staging, and `mandate vault provision` and `mandate vault
/// put` run against the vault with the given standard input, each answering whether it succeeded
/// and what it printed: the Secrets, or one status line.
pub(crate) struct Steps<'a> {
    pub(crate) kubectl: Kubectl<'a>,
    pub(crate) provision: Provision<'a>,
    pub(crate) put: Put<'a>,
}

/// The name of an executor's Secret, as `mandate_vault::kube::secret_name` writes it: the
/// connection id's bytes in lowercase hexadecimal.
fn executor_secret(connection: &str) -> String {
    let hex: String = connection
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("vault-approle-executor-{hex}")
}

/// `kubectl exec` into the vault's container, with `stdin` when the command reads one.
fn in_vault(steps: &Steps<'_>, command: &[&str], stdin: Option<&str>) -> Result<(bool, String)> {
    let mut args = vec!["exec"];
    if stdin.is_some() {
        args.push("-i");
    }
    args.extend([
        POD,
        "--namespace",
        WORKSPACE_NAMESPACE,
        "-c",
        CONTAINER,
        "--",
    ]);
    args.extend(command);
    (steps.kubectl)(&args, stdin)
}

/// Whether every Secret the step writes for `connections` is already there.
fn provisioned(steps: &Steps<'_>, connections: &[&str]) -> Result<bool> {
    let (read, listed) = (steps.kubectl)(
        &[
            "get",
            "secrets",
            "--namespace",
            WORKSPACE_NAMESPACE,
            "--selector",
            MANAGED_BY,
            "-o",
            "jsonpath={.items[*].metadata.name}",
        ],
        None,
    )?;
    ensure!(read, "listing staging's vault credentials failed");
    let present: Vec<&str> = listed.split_whitespace().collect();
    let executors: Vec<String> = connections.iter().map(|c| executor_secret(c)).collect();
    Ok(PLATFORM_SECRETS
        .iter()
        .copied()
        .chain(executors.iter().map(String::as_str))
        .all(|name| present.contains(&name)))
}

/// `host` when it is exactly `https://`, an IPv4 address and a port from 1 to 65535, written the
/// one way `SocketAddrV4` writes it, as `mandate_vault::kubernetes::api_server_address` takes it:
/// the vault sends its own token there on every TokenReview, so nothing else is trusted.
fn api_server_host(host: &str) -> Option<String> {
    let address: SocketAddrV4 = host.strip_prefix("https://")?.parse().ok()?;
    let written = format!("https://{address}");
    (address.port() != 0 && written == host).then_some(written)
}

/// The `kubernetes` Service's address, as the vault's pod reaches the API server.
fn kubernetes_host(steps: &Steps<'_>) -> Result<String> {
    let (read, address) = (steps.kubectl)(
        &[
            "get",
            "service",
            "kubernetes",
            "--namespace",
            "default",
            "-o",
            "jsonpath={.spec.clusterIP}:{.spec.ports[?(@.name==\"https\")].port}",
        ],
        None,
    )?;
    ensure!(read, "reading the kubernetes Service's address failed");
    api_server_host(&format!("https://{address}"))
        .context("the kubernetes Service's address is not an IPv4 address and a port")
}

/// The address [`MARKER`] records, when it is one [`api_server_host`] takes; a missing, empty or
/// malformed one is none, so the step sets Kubernetes auth up again.
fn marked_host(steps: &Steps<'_>) -> Result<Option<String>> {
    let (read, marked) = (steps.kubectl)(
        &[
            "get",
            "configmap",
            MARKER,
            "--namespace",
            WORKSPACE_NAMESPACE,
            "--ignore-not-found",
            "-o",
            "jsonpath={.data.host}",
        ],
        None,
    )?;
    ensure!(read, "reading {MARKER} failed");
    Ok(api_server_host(&marked))
}

/// [`MARKER`]'s manifest, naming `host`.
fn marker_manifest(host: &str) -> String {
    serde_json::json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {
            "name": MARKER,
            "namespace": WORKSPACE_NAMESPACE,
            "labels": {"app.kubernetes.io/managed-by": "mandate-vault-provision"},
        },
        "data": {MARKER_KEY: host},
    })
    .to_string()
}

/// The recovery keys `vault_init` stored, each masked as it is read.
fn recovery_keys(steps: &Steps<'_>) -> Result<Vec<String>> {
    let (read, answer) = (steps.kubectl)(
        &[
            "get",
            "secret",
            RECOVERY_SECRET,
            "--namespace",
            RECOVERY_NAMESPACE,
            "-o",
            "go-template={{index .data \"init.json\" | base64decode}}",
        ],
        None,
    )?;
    let keys: Vec<String> = serde_json::from_str::<Value>(&answer)
        .ok()
        .and_then(|init| {
            init["recovery_keys_b64"].as_array().map(|keys| {
                keys.iter()
                    .filter_map(|key| key.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default();
    keys.iter().for_each(|key| mask::add_mask(key));
    ensure!(
        read && keys.len() >= THRESHOLD,
        "the recovery keys in {RECOVERY_NAMESPACE}/{RECOVERY_SECRET} could not be read"
    );
    Ok(keys)
}

/// `bao write` against the root-generation listener, with its JSON answer's `data`.
fn generate_root(steps: &Steps<'_>, args: &[&str], stdin: Option<&str>) -> Result<Value> {
    let mut command = vec!["bao", "write", "-format=json", GENERATE_ROOT_ADDRESS];
    command.extend(args);
    let (done, answer) = in_vault(steps, &command, stdin)?;
    let data = serde_json::from_str::<Value>(&answer)
        .ok()
        .map(|answer| answer["data"].clone());
    if let Some(otp) = data.as_ref().and_then(|data| data["otp"].as_str()) {
        mask::add_mask(otp);
    }
    if let Some(encoded) = data
        .as_ref()
        .and_then(|data| data["encoded_token"].as_str())
    {
        mask::add_mask(encoded);
    }
    ensure!(
        done,
        "generating a root token from the recovery keys failed"
    );
    data.context("the vault's root-generation answer could not be read")
}

/// Standard base64 with or without padding, as the vault encodes a generated token.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0_u32;
    let mut held = 0_u32;
    let mut bytes = Vec::new();
    for symbol in text.trim_end_matches('=').bytes() {
        let value = ALPHABET.iter().position(|a| *a == symbol)?;
        bits = (bits << 6) | u32::try_from(value).ok()?;
        held += 6;
        if held >= 8 {
            held -= 8;
            bytes.push(u8::try_from((bits >> held) & 0xff).ok()?);
        }
    }
    Some(bytes)
}

/// The root token from the vault's `encoded_token` and the attempt's one-time pad: the decoded
/// bytes, each XORed with the pad's byte at its place.
fn decode_root(encoded: &str, otp: &str) -> Result<String> {
    let bytes = base64_decode(encoded).context("the generated token is not base64")?;
    ensure!(
        bytes.len() == otp.len(),
        "the generated token and its one-time pad differ in length"
    );
    let token: Vec<u8> = bytes.iter().zip(otp.bytes()).map(|(b, p)| b ^ p).collect();
    String::from_utf8(token).context("the generated token is not text")
}

/// A root token made from the recovery keys, revoked when dropped: on success, on an error and on
/// a panic alike. [`Root::revoke`] revokes it early and says whether it worked.
struct Root<'s, 'a> {
    steps: &'s Steps<'a>,
    token: Option<String>,
}

/// What the step says when a root token may be left live, with no token in the text.
pub(crate) const MAY_BE_LIVE: &str = "the generated root token may be live: revoke it with a token \
     generated from the recovery keys in vault-recovery/vault-init (DEC-2630)";

/// Revokes `token`, trying once more when `kubectl` itself could not be started, answering whether
/// the vault revoked it.
fn revoke_token(steps: &Steps<'_>, token: &str) -> bool {
    let attempt = || in_vault(steps, &REVOKE, Some(token));
    match attempt().or_else(|_| attempt()) {
        Ok((revoked, _)) => revoked,
        Err(_) => false,
    }
}

impl Root<'_, '_> {
    fn revoke(mut self) -> Result<()> {
        let Some(token) = self.token.take() else {
            return Ok(());
        };
        ensure!(revoke_token(self.steps, &token), "{MAY_BE_LIVE}");
        Ok(())
    }
}

impl Drop for Root<'_, '_> {
    fn drop(&mut self) {
        if let Some(token) = self.token.take()
            && !revoke_token(self.steps, &token)
        {
            eprintln!("    vault-provision: {MAY_BE_LIVE}");
        }
    }
}

/// Makes a root token from the first [`THRESHOLD`] recovery keys, cancelling any attempt an
/// earlier run left open first, and cancelling this one again when it does not complete. A
/// completed generation whose token cannot be decoded leaves a root token no one holds; the step
/// fails saying it may be live.
fn root<'s, 'a>(steps: &'s Steps<'a>, keys: &[String]) -> Result<Root<'s, 'a>> {
    let cancel = || {
        in_vault(
            steps,
            &[
                "bao",
                "delete",
                GENERATE_ROOT_ADDRESS,
                "sys/generate-root/attempt",
            ],
            None,
        )
    };
    cancel()?;
    let attempt = generate_root(steps, &["-f", "sys/generate-root/attempt"], None)?;
    let (Some(nonce), Some(otp)) = (attempt["nonce"].as_str(), attempt["otp"].as_str()) else {
        cancel().ok();
        bail!("the vault's root-generation attempt named no nonce or one-time pad");
    };
    let nonce_arg = format!("nonce={nonce}");
    let mut answer = Value::Null;
    for key in keys.iter().take(THRESHOLD) {
        match generate_root(
            steps,
            &["sys/generate-root/update", &nonce_arg, "key=-"],
            Some(key),
        ) {
            Ok(data) => answer = data,
            Err(error) => {
                cancel().ok();
                return Err(error);
            }
        }
    }
    let Some(encoded) = answer["encoded_token"]
        .as_str()
        .filter(|_| answer["complete"] == true)
    else {
        cancel().ok();
        bail!("the vault's root generation did not complete with {THRESHOLD} recovery keys");
    };
    let Ok(token) = decode_root(encoded, otp) else {
        cancel().ok();
        bail!(
            "the generated root token could not be decoded, so it was not revoked; {MAY_BE_LIVE}"
        );
    };
    mask::add_mask(&token);
    Ok(Root {
        steps,
        token: Some(token),
    })
}

/// The connections staging's vault gives an executor's credentials to, from
/// `deploy/hosted/vault-connections.json`: paper connection ids, which are not secret (DEC-2630).
pub(crate) fn staging_connections(root: &Path) -> Result<Vec<String>> {
    let path = root.join("deploy/hosted/vault-connections.json");
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let listed: Value =
        serde_json::from_str(&text).context("vault-connections.json is not JSON")?;
    listed["connections"]
        .as_array()
        .context("vault-connections.json names no `connections` list")?
        .iter()
        .map(|id| {
            id.as_str()
                .map(str::to_owned)
                .context("a connection in vault-connections.json is not a string")
        })
        .collect()
}

/// The `mandate` CLI the step runs, relative to the checkout: `infra.yml`'s build step builds it
/// beside xtask, with no secret in its environment, so no build script ever sees one.
pub(crate) const CLI: &str = "target/debug/mandate";

/// A `kubectl port-forward` to the vault's pod, stopped when dropped.
struct Forward(Child);

impl Drop for Forward {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// Runs `mandate vault <command>` at `cli` against the vault through a `kubectl port-forward` that
/// `kubectl` (a command already naming the cluster) starts, with `--address` and `--ca` (the
/// ConfigMap `vault-ca`'s, which is not secret) added and `stdin` on its standard input, answering
/// whether it succeeded and what it printed. Its environment is cleared, and its standard error,
/// which carries reason codes alone, is shown.
pub(crate) fn run_cli(
    kubectl: &dyn Fn() -> Command,
    read: Kubectl<'_>,
    cli: &Path,
    temp: &Path,
    command: &[String],
    stdin: &str,
) -> Result<(bool, String)> {
    let (got, ca) = read(
        &[
            "get",
            "configmap",
            "vault-ca",
            "--namespace",
            WORKSPACE_NAMESPACE,
            "-o",
            "jsonpath={.data.ca\\.crt}",
        ],
        None,
    )?;
    ensure!(got && !ca.is_empty(), "reading the vault's CA failed");
    let ca_file = temp.join("vault-ca.pem");
    fs::write(&ca_file, ca).context("writing the vault's CA")?;
    let mut child = kubectl()
        .args([
            "port-forward",
            "--namespace",
            WORKSPACE_NAMESPACE,
            "pod/vault-0",
            ":8200",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("starting kubectl port-forward")?;
    let stdout = child.stdout.take().context("port-forward's stdout")?;
    let forward = Forward(child);
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .context("reading port-forward's address")?;
    let port = line
        .strip_prefix("Forwarding from 127.0.0.1:")
        .and_then(|rest| rest.split_whitespace().next())
        .context("port-forward named no local port")?;
    let mut run = Command::new(cli)
        .env_clear()
        .arg("vault")
        .args(command)
        .arg("--address")
        .arg(format!("https://127.0.0.1:{port}"))
        .arg("--ca")
        .arg(&ca_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("starting the mandate CLI")?;
    run.stdin
        .take()
        .context("the CLI's stdin")?
        .write_all(stdin.as_bytes())?;
    let out = run
        .wait_with_output()
        .context("waiting for the mandate CLI")?;
    drop(forward);
    Ok((
        out.status.success(),
        String::from_utf8(out.stdout).context("the CLI printed non-UTF-8")?,
    ))
}

/// The arguments of `mandate vault provision` for `connections`, Kubernetes auth at `host`, its
/// token on stdin and its Secrets on stdout.
pub(crate) fn provision_command(connections: &[&str], host: &str) -> Vec<String> {
    let mut command: Vec<String> = [
        "provision",
        "--operator-token-stdin",
        "--secrets-namespace",
        WORKSPACE_NAMESPACE,
        "--kubernetes-host",
        host,
    ]
    .map(str::to_owned)
    .to_vec();
    for connection in connections {
        command.extend(["--connection".to_owned(), (*connection).to_owned()]);
    }
    command
}

/// The arguments of `mandate vault provision --kubernetes-only` at `host`, its token on stdin.
pub(crate) fn kubernetes_command(host: &str) -> Vec<String> {
    [
        "provision",
        "--operator-token-stdin",
        "--kubernetes-only",
        "--kubernetes-host",
        host,
    ]
    .map(str::to_owned)
    .to_vec()
}

/// The `mandate vault put` name of the model provider's key (DEC-2630).
pub(crate) const MODEL_KEY_SECRET: &str = "model-provider-key";

/// The `mandate vault put` name of the news reader's Alpaca paper data key (DEC-2277).
pub(crate) const NEWS_KEY_SECRET: &str = "news-data-key";

/// The one line `mandate vault put news-data-key` writes, `<key id>:<secret>`, from the two
/// environment secrets: empty when both are, so the vault keeps what it holds. One without the
/// other, or a part that holds `:`, whitespace or a control character, as a paste can leave, fails
/// the step, so a key the reader would refuse is never written (DEC-2277).
pub(crate) fn news_data_key(key_id: &str, secret: &str) -> Result<String> {
    let clean = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c != ':' && !c.is_whitespace() && !c.is_control())
    };
    match (key_id.is_empty(), secret.is_empty()) {
        (true, true) => Ok(String::new()),
        _ if clean(key_id) && clean(secret) => Ok(format!("{key_id}:{secret}")),
        _ => bail!(
            "the news data key needs both ALPACA_NEWS_KEY_ID and ALPACA_NEWS_SECRET, each one line with no `:`"
        ),
    }
}

/// The `mandate vault put` name of the Alpaca OAuth client secret (DEC-2636).
pub(crate) const ALPACA_CLIENT_SECRET: &str = "alpaca-oauth-client-secret";

/// The arguments of `mandate vault put <secret>`, its credentials and the secret on stdin.
pub(crate) fn put_command(secret: &str) -> Vec<String> {
    ["put", secret].map(str::to_owned).to_vec()
}

/// The secret-import identity's credentials from its Secret, its secret id masked as it is read,
/// as the first two lines `mandate vault put` reads.
fn secret_import(steps: &Steps<'_>) -> Result<String> {
    let (read, lines) = (steps.kubectl)(
        &[
            "get",
            "secret",
            "vault-approle-secret-import",
            "--namespace",
            WORKSPACE_NAMESPACE,
            "-o",
            "go-template={{index .data \"role-id\" | base64decode}}{{\"\\n\"}}{{index .data \"secret-id\" | base64decode}}",
        ],
        None,
    )?;
    if let Some((_, secret_id)) = lines.split_once('\n') {
        mask::add_mask(secret_id);
    }
    ensure!(
        read && lines.lines().count() == 2,
        "the secret-import identity's credentials could not be read"
    );
    Ok(lines)
}

/// Provisions staging's vault for `connections`, with Kubernetes auth at the `kubernetes`
/// Service's address, unless every Secret is already there; sets Kubernetes auth alone up when
/// they are but [`MARKER`] does not name that address; records the address once either is done;
/// and revokes the root token on every path, a failure's included. Then it writes
/// each of `model_key` (DEC-2630), `alpaca_client_secret` (DEC-2636) and `news_key` (DEC-2277) that
/// is not empty through
/// the secret-import identity on every run, so a rotation is the secret replaced and the apply run
/// again; an empty one is left as the vault holds it, never deleted. Answers the status words to
/// print.
pub(crate) fn vault_provision(
    steps: &Steps<'_>,
    connections: &[&str],
    model_key: &str,
    alpaca_client_secret: &str,
    news_key: &str,
) -> Result<Vec<&'static str>> {
    let mut words = Vec::new();
    let host = kubernetes_host(steps)?;
    let present = provisioned(steps, connections)?;
    let marked = marked_host(steps)?.as_deref() == Some(host.as_str());
    if present && marked {
        words.extend(["credentials: present", "kubernetes auth: present"]);
    } else {
        let keys = recovery_keys(steps)?;
        let root = root(steps, &keys)?;
        let token = root.token.clone().unwrap_or_default();
        let stdin = format!("{token}\n");
        words.push("root token: generated");
        if present {
            words.push("credentials: present");
            let (set, _) = (steps.provision)(&kubernetes_command(&host), &stdin)?;
            ensure!(set, "mandate vault provision --kubernetes-only failed");
        } else {
            let (provisioned, list) =
                (steps.provision)(&provision_command(connections, &host), &stdin)?;
            ensure!(provisioned, "mandate vault provision failed");
            let (applied, _) = (steps.kubectl)(&["apply", "-f", "-"], Some(&list))?;
            ensure!(applied, "applying staging's vault credentials failed");
            words.push("credentials: provisioned");
        }
        let (recorded, _) = (steps.kubectl)(&["apply", "-f", "-"], Some(&marker_manifest(&host)))?;
        ensure!(recorded, "recording {MARKER} failed");
        root.revoke()?;
        words.extend(["kubernetes auth: set up", "root token: revoked"]);
    }
    words.push(import(
        steps,
        MODEL_KEY_SECRET,
        model_key,
        ["model provider key: not set", "model provider key: written"],
    )?);
    words.push(import(
        steps,
        ALPACA_CLIENT_SECRET,
        alpaca_client_secret,
        [
            "alpaca oauth client secret: not set",
            "alpaca oauth client secret: written",
        ],
    )?);
    words.push(import(
        steps,
        NEWS_KEY_SECRET,
        news_key,
        ["news data key: not set", "news data key: written"],
    )?);
    Ok(words)
}

/// Writes `value` as `secret` through the secret-import identity, its credentials and the value on
/// the CLI's stdin alone, unless it is empty; answers `words`' first when it is, its second once
/// written, and fails the step when the write fails.
fn import(
    steps: &Steps<'_>,
    secret: &str,
    value: &str,
    words: [&'static str; 2],
) -> Result<&'static str> {
    let [not_set, written] = words;
    if value.is_empty() {
        return Ok(not_set);
    }
    let credentials = secret_import(steps)?;
    let (done, _) = (steps.put)(secret, &format!("{credentials}\n{value}\n"))?;
    ensure!(done, "writing {secret} failed");
    Ok(written)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use anyhow::anyhow;
    use serde_json::json;

    use super::*;
    use crate::image;

    const ROOT: &str = "s.generated-root-token-123";
    const OTP: &str = "0123456789abcdefghijklmnop";
    const KEYS: [&str; 3] = ["recovery-a", "recovery-b", "recovery-c"];
    const LIST: &str = r#"{"apiVersion":"v1","kind":"List","items":[{"secret":"id-1"}]}"#;
    const IMPORT: &str = "import-role\nimport-secret-id";
    const MODEL_KEY: &str = "model-key-canary";
    const CLIENT_SECRET: &str = "alpaca-client-secret-canary";

    fn encoded_root() -> String {
        let xored: Vec<u8> = ROOT.bytes().zip(OTP.bytes()).map(|(t, p)| t ^ p).collect();
        image::base64(&xored).trim_end_matches('=').to_owned()
    }

    /// Staging as `kubectl` sees it, its vault answering root generation one way or another.
    #[derive(Default)]
    struct Fake {
        present: Vec<String>,
        unreadable_keys: bool,
        incomplete: bool,
        provision_fails: bool,
        apply_fails: bool,
        put_fails: bool,
        garbled_token: bool,
        revoke_fails: bool,
        revoke_spawn_failures: Cell<u32>,
        model_key: &'static str,
        client_secret: &'static str,
        news_key: &'static str,
        /// What `kubectl` reads as the `kubernetes` Service's address; empty is `10.43.0.1:443`.
        api_server: &'static str,
        /// What [`MARKER`] names; `None` is the default address, as a provisioned vault records.
        marker: Option<&'static str>,
        kubernetes_only_fails: bool,
        marker_apply_fails: bool,
        calls: RefCell<Vec<(Vec<String>, Option<String>)>>,
        provisioned_with: RefCell<Vec<String>>,
        commands: RefCell<Vec<Vec<String>>>,
        put_with: RefCell<Vec<String>>,
    }

    impl Fake {
        fn kubectl(&self, args: &[&str], stdin: Option<&str>) -> Result<(bool, String)> {
            self.calls.borrow_mut().push((
                args.iter().map(|arg| (*arg).to_owned()).collect(),
                stdin.map(str::to_owned),
            ));
            let has = |word: &str| args.contains(&word);
            Ok(match args.first().copied().unwrap_or_default() {
                "get" if has("secrets") => (true, self.present.join(" ")),
                "get" if has("service") => (
                    true,
                    if self.api_server.is_empty() {
                        "10.43.0.1:443"
                    } else {
                        self.api_server
                    }
                    .to_owned(),
                ),
                "get" if has(MARKER) => (
                    true,
                    self.marker.unwrap_or("https://10.43.0.1:443").to_owned(),
                ),
                "get" if has("vault-approle-secret-import") => (true, IMPORT.to_owned()),
                "get" if has(RECOVERY_SECRET) => (
                    !self.unreadable_keys,
                    json!({"recovery_keys_b64": KEYS}).to_string(),
                ),
                "exec" if has("delete") => (true, String::new()),
                "exec" if has("sys/generate-root/attempt") => (
                    true,
                    json!({"data": {"nonce": "nonce-1", "otp": OTP, "started": true}}).to_string(),
                ),
                "exec" if has("sys/generate-root/update") => {
                    let updates = self.calls_with("sys/generate-root/update").len();
                    let complete = updates == THRESHOLD && !self.incomplete;
                    let encoded = if self.garbled_token {
                        "AAAA".to_owned()
                    } else {
                        encoded_root()
                    };
                    let data = if complete {
                        json!({"complete": true, "encoded_token": encoded})
                    } else {
                        json!({"complete": false, "progress": updates})
                    };
                    (true, json!({ "data": data }).to_string())
                }
                "exec" if stdin == Some(ROOT) => {
                    let failures = self.revoke_spawn_failures.get();
                    if failures > 0 {
                        self.revoke_spawn_failures.set(failures - 1);
                        return Err(anyhow!("kubectl could not start"));
                    }
                    (!self.revoke_fails, String::new())
                }
                "apply" if stdin.is_some_and(|manifest| manifest.contains(MARKER)) => {
                    (!self.marker_apply_fails, String::new())
                }
                "apply" => (!self.apply_fails, String::new()),
                other => return Err(anyhow!("unexpected kubectl {other}: {args:?}")),
            })
        }

        fn run(&self, connections: &[&str]) -> Result<Vec<&'static str>> {
            let provision = |command: &[String], stdin: &str| {
                self.commands.borrow_mut().push(command.to_vec());
                if command.iter().any(|arg| arg == "--kubernetes-only") {
                    return Ok((!self.kubernetes_only_fails, String::new()));
                }
                self.provisioned_with.borrow_mut().push(stdin.to_owned());
                Ok((!self.provision_fails, LIST.to_owned()))
            };
            let put = |secret: &str, stdin: &str| {
                self.put_with.borrow_mut().push(format!("{secret}:{stdin}"));
                Ok((!self.put_fails, String::new()))
            };
            vault_provision(
                &Steps {
                    kubectl: &|args, stdin| self.kubectl(args, stdin),
                    provision: &provision,
                    put: &put,
                },
                connections,
                self.model_key,
                self.client_secret,
                self.news_key,
            )
        }

        fn calls_with(&self, word: &str) -> Vec<(Vec<String>, Option<String>)> {
            self.calls
                .borrow()
                .iter()
                .filter(|(args, _)| args.iter().any(|arg| arg == word))
                .cloned()
                .collect()
        }

        fn revoked(&self) -> bool {
            self.calls.borrow().iter().any(|(args, stdin)| {
                args.iter().any(|a| a.contains("revoke")) && stdin.as_deref() == Some(ROOT)
            })
        }

        /// No key, pad, or token ever reaches an argument; each reaches stdin alone.
        fn nothing_secret_in_argv(&self) {
            let encoded = encoded_root();
            for (args, _) in self.calls.borrow().iter() {
                for secret in [ROOT, OTP, encoded.as_str()].iter().chain(KEYS.iter()) {
                    assert!(
                        !args.iter().any(|arg| arg.contains(secret)),
                        "{secret} in {args:?}"
                    );
                }
            }
        }
    }

    fn all_present(connections: &[&str]) -> Vec<String> {
        PLATFORM_SECRETS
            .iter()
            .map(|name| (*name).to_owned())
            .chain(connections.iter().map(|c| executor_secret(c)))
            .collect()
    }

    #[test]
    fn an_executors_secret_is_named_as_mandate_vault_names_it() {
        assert_eq!(executor_secret("c1"), "vault-approle-executor-6331");
        assert_eq!(
            executor_secret("Ab_-9"),
            "vault-approle-executor-41625f2d39"
        );
    }

    #[test]
    fn base64_decodes_with_or_without_padding_and_refuses_other_symbols() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("ab", "YWI"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE="),
        ] {
            assert_eq!(base64_decode(encoded), Some(plain.as_bytes().to_vec()));
        }
        assert_eq!(base64_decode("+/8"), Some(vec![0xfb, 0xff]));
        assert_eq!(base64_decode("Zm9v_"), None);
    }

    #[test]
    fn the_root_token_is_the_encoded_bytes_xored_with_the_pad() -> Result<()> {
        assert_eq!(ROOT.len(), OTP.len());
        assert_eq!(decode_root(&encoded_root(), OTP)?, ROOT);
        assert!(
            decode_root(&encoded_root(), &OTP[1..]).is_err(),
            "a pad of another length"
        );
        Ok(())
    }

    /// Every Secret there: nothing is generated, provisioned, or applied.
    #[test]
    fn a_provisioned_vault_is_left_as_it_is() -> Result<()> {
        let fake = Fake {
            present: all_present(&["c1"]),
            ..Fake::default()
        };
        assert_eq!(
            fake.run(&["c1"])?,
            [
                "credentials: present",
                "kubernetes auth: present",
                "model provider key: not set",
                "alpaca oauth client secret: not set",
                "news data key: not set"
            ]
        );
        assert_eq!(fake.calls.borrow().len(), 3);
        assert!(fake.commands.borrow().is_empty());
        Ok(())
    }

    /// A staging vault provisioned before the role provisioner (DEC-2350) holds every platform
    /// Secret but `vault-approle-provisioner`, so the next apply provisions it once, and
    /// `--secrets-namespace` issues that Secret with the others.
    #[test]
    fn a_vault_without_the_provisioners_secret_is_provisioned() -> Result<()> {
        assert!(PLATFORM_SECRETS.contains(&"vault-approle-provisioner"));
        let mut present = all_present(&["c1"]);
        present.retain(|name| name != "vault-approle-provisioner");
        let fake = Fake {
            present,
            ..Fake::default()
        };
        assert!(fake.run(&["c1"])?.contains(&"credentials: provisioned"));
        assert!(
            fake.commands
                .borrow()
                .iter()
                .any(|command| command.iter().any(|a| a == "--secrets-namespace"))
        );
        Ok(())
    }

    /// One Secret missing: a root token is made from two recovery keys on stdin, the earlier
    /// attempt cancelled first, the CLI given the token, its Secrets applied from stdin, and the
    /// token revoked.
    #[test]
    fn a_missing_secret_provisions_the_vault_once() -> Result<()> {
        let mut present = all_present(&["c1"]);
        present.retain(|name| name != "vault-approle-runtime");
        let fake = Fake {
            present,
            ..Fake::default()
        };
        assert_eq!(
            fake.run(&["c1"])?,
            [
                "root token: generated",
                "credentials: provisioned",
                "kubernetes auth: set up",
                "root token: revoked",
                "model provider key: not set",
                "alpaca oauth client secret: not set",
                "news data key: not set"
            ]
        );
        let updates = fake.calls_with("sys/generate-root/update");
        let fed: Vec<Option<String>> = updates.into_iter().map(|(_, stdin)| stdin).collect();
        assert_eq!(fed, [Some(KEYS[0].to_owned()), Some(KEYS[1].to_owned())]);
        let order: Vec<String> = fake
            .calls
            .borrow()
            .iter()
            .filter_map(|(args, _)| {
                args.iter()
                    .find(|a| a.starts_with("sys/") || *a == "apply" || a.contains("revoke"))
                    .cloned()
            })
            .collect();
        assert_eq!(
            order,
            [
                "sys/generate-root/attempt",
                "sys/generate-root/attempt",
                "sys/generate-root/update",
                "sys/generate-root/update",
                "apply",
                "apply",
                REVOKE[2],
            ]
        );
        assert_eq!(*fake.provisioned_with.borrow(), [format!("{ROOT}\n")]);
        assert_eq!(
            *fake.commands.borrow(),
            [provision_command(&["c1"], "https://10.43.0.1:443")]
        );
        let applied = fake.calls_with("apply");
        assert_eq!(applied[0].1.as_deref(), Some(LIST));
        assert_eq!(
            applied[1].1.as_deref(),
            Some(marker_manifest("https://10.43.0.1:443").as_str())
        );
        assert!(fake.revoked());
        fake.nothing_secret_in_argv();
        Ok(())
    }

    /// Every Secret there but no record of Kubernetes auth, or a record of another address or
    /// one that is no address: Kubernetes auth alone is set up at the Service's address, with the
    /// root token on stdin, no Secret applied and no secret id minted; the address is recorded,
    /// and the token revoked.
    #[test]
    fn a_provisioned_vault_without_kubernetes_auth_at_its_address_gains_it_alone() -> Result<()> {
        for marker in [
            "",
            "https://10.43.0.2:443",
            "https://10.43.0.1:6443",
            "https://10.43.0.1:443/",
            "https://u@10.43.0.1:443",
            " https://10.43.0.1:443",
        ] {
            let fake = Fake {
                present: all_present(&["c1"]),
                marker: Some(marker),
                ..Fake::default()
            };
            assert_eq!(
                fake.run(&["c1"])?,
                [
                    "root token: generated",
                    "credentials: present",
                    "kubernetes auth: set up",
                    "root token: revoked",
                    "model provider key: not set",
                    "alpaca oauth client secret: not set",
                    "news data key: not set"
                ],
                "{marker:?}"
            );
            assert_eq!(
                *fake.commands.borrow(),
                [kubernetes_command("https://10.43.0.1:443")],
                "{marker:?}"
            );
            assert!(fake.provisioned_with.borrow().is_empty(), "{marker:?}");
            let applied: Vec<Option<String>> = fake
                .calls_with("apply")
                .into_iter()
                .map(|(_, stdin)| stdin)
                .collect();
            assert_eq!(
                applied,
                [Some(marker_manifest("https://10.43.0.1:443"))],
                "{marker:?}"
            );
            assert!(fake.revoked(), "{marker:?}");
            fake.nothing_secret_in_argv();
        }
        Ok(())
    }

    /// The address is the `kubernetes` Service's, an IPv4 address and a port; anything else stops
    /// the step before a root token is made.
    #[test]
    fn an_api_server_that_is_not_an_ipv4_address_and_port_stops_the_step() {
        for api_server in [
            "kubernetes.default.svc:443",
            "10.43.0.1:",
            ":443",
            "10.43.0.1:0",
            "10.43.0.1:443/x",
            "u@10.43.0.1:443",
            "[::1]:443",
        ] {
            let fake = Fake {
                api_server,
                ..Fake::default()
            };
            assert!(fake.run(&["c1"]).is_err(), "{api_server:?}");
            assert!(fake.calls_with("exec").is_empty(), "{api_server:?}");
            assert!(fake.commands.borrow().is_empty(), "{api_server:?}");
        }
        let elsewhere = Fake {
            api_server: "10.96.0.1:6443",
            ..Fake::default()
        };
        elsewhere.run(&["c1"]).unwrap();
        assert_eq!(
            elsewhere.commands.borrow()[0],
            provision_command(&["c1"], "https://10.96.0.1:6443")
        );
    }

    /// The marker's manifest is a ConfigMap in the workspace's namespace, labelled as the
    /// provisioned Secrets are, naming the address alone.
    #[test]
    fn the_marker_is_a_configmap_naming_the_address() {
        let manifest: Value =
            serde_json::from_str(&marker_manifest("https://10.43.0.1:443")).unwrap();
        assert_eq!(
            manifest,
            json!({
                "apiVersion": "v1",
                "kind": "ConfigMap",
                "metadata": {
                    "name": "vault-kubernetes-auth",
                    "namespace": "ws-ws1",
                    "labels": {"app.kubernetes.io/managed-by": "mandate-vault-provision"},
                },
                "data": {"host": "https://10.43.0.1:443"},
            })
        );
        assert_eq!(
            MANAGED_BY,
            "app.kubernetes.io/managed-by=mandate-vault-provision"
        );
    }

    /// The root token is revoked however setting Kubernetes auth up ends: the CLI failing, or
    /// recording the address failing, on either path.
    #[test]
    fn a_failed_kubernetes_setup_or_record_still_revokes_the_token() {
        for fake in [
            Fake {
                present: all_present(&["c1"]),
                marker: Some(""),
                kubernetes_only_fails: true,
                ..Fake::default()
            },
            Fake {
                present: all_present(&["c1"]),
                marker: Some(""),
                marker_apply_fails: true,
                ..Fake::default()
            },
            Fake {
                marker_apply_fails: true,
                ..Fake::default()
            },
        ] {
            assert!(fake.run(&["c1"]).is_err());
            assert!(fake.revoked());
            fake.nothing_secret_in_argv();
        }
        let failed = Fake {
            present: all_present(&["c1"]),
            marker: Some(""),
            kubernetes_only_fails: true,
            ..Fake::default()
        };
        assert!(failed.run(&["c1"]).is_err());
        assert!(failed.calls_with("apply").is_empty(), "no address recorded");
    }

    #[test]
    fn a_new_connection_provisions_the_vault() -> Result<()> {
        let fake = Fake {
            present: all_present(&["c1"]),
            ..Fake::default()
        };
        fake.run(&["c1", "c2"])?;
        assert_eq!(fake.provisioned_with.borrow().len(), 1);
        Ok(())
    }

    #[test]
    fn a_failed_provision_or_apply_still_revokes_the_token() {
        for fake in [
            Fake {
                provision_fails: true,
                ..Fake::default()
            },
            Fake {
                apply_fails: true,
                ..Fake::default()
            },
        ] {
            assert!(fake.run(&["c1"]).is_err());
            assert!(fake.revoked());
            fake.nothing_secret_in_argv();
        }
    }

    #[test]
    fn a_failed_provision_applies_nothing() {
        let fake = Fake {
            provision_fails: true,
            ..Fake::default()
        };
        assert!(fake.run(&["c1"]).is_err());
        assert!(fake.calls_with("apply").is_empty());
    }

    #[test]
    fn an_incomplete_generation_is_cancelled_and_provisions_nothing() {
        let fake = Fake {
            incomplete: true,
            ..Fake::default()
        };
        assert!(fake.run(&["c1"]).is_err());
        assert_eq!(fake.calls_with("delete").len(), 2);
        assert!(fake.provisioned_with.borrow().is_empty());
        assert!(!fake.revoked());
    }

    #[test]
    fn unreadable_recovery_keys_stop_before_the_vault_is_touched() {
        let fake = Fake {
            unreadable_keys: true,
            ..Fake::default()
        };
        assert!(fake.run(&["c1"]).is_err());
        assert!(fake.calls_with("exec").is_empty());
    }

    /// A key set is written through the secret-import identity on every run, its credentials and
    /// the key on the CLI's stdin alone, whether or not the vault was provisioned this run.
    #[test]
    fn a_set_model_key_is_written_on_every_run() -> Result<()> {
        let fake = Fake {
            present: all_present(&["c1"]),
            model_key: MODEL_KEY,
            ..Fake::default()
        };
        assert_eq!(
            fake.run(&["c1"])?,
            [
                "credentials: present",
                "kubernetes auth: present",
                "model provider key: written",
                "alpaca oauth client secret: not set",
                "news data key: not set"
            ]
        );
        assert_eq!(
            *fake.put_with.borrow(),
            [format!("model-provider-key:{IMPORT}\n{MODEL_KEY}\n")]
        );
        for (args, _) in fake.calls.borrow().iter() {
            assert!(!args.iter().any(|arg| arg.contains(MODEL_KEY)), "{args:?}");
        }
        let fresh = Fake {
            model_key: MODEL_KEY,
            ..Fake::default()
        };
        fresh.run(&["c1"])?;
        assert_eq!(fresh.put_with.borrow().len(), 1);
        assert!(fresh.revoked());
        Ok(())
    }

    #[test]
    fn an_unset_model_key_is_not_written() -> Result<()> {
        let fake = Fake::default();
        fake.run(&["c1"])?;
        assert!(fake.put_with.borrow().is_empty());
        assert!(fake.calls_with("vault-approle-secret-import").is_empty());
        Ok(())
    }

    #[test]
    fn a_failed_put_fails_the_step() {
        let fake = Fake {
            present: all_present(&[]),
            model_key: MODEL_KEY,
            put_fails: true,
            ..Fake::default()
        };
        assert!(fake.run(&[]).is_err());
    }

    /// A client secret set is written through the secret-import identity on every run, apart from
    /// the model key, its credentials and the secret on the CLI's stdin alone (DEC-2636).
    #[test]
    fn a_set_client_secret_is_written_on_every_run() -> Result<()> {
        let fake = Fake {
            present: all_present(&["c1"]),
            client_secret: CLIENT_SECRET,
            ..Fake::default()
        };
        assert_eq!(
            fake.run(&["c1"])?,
            [
                "credentials: present",
                "kubernetes auth: present",
                "model provider key: not set",
                "alpaca oauth client secret: written",
                "news data key: not set"
            ]
        );
        assert_eq!(
            *fake.put_with.borrow(),
            [format!(
                "alpaca-oauth-client-secret:{IMPORT}\n{CLIENT_SECRET}\n"
            )]
        );
        for (args, _) in fake.calls.borrow().iter() {
            assert!(
                !args.iter().any(|arg| arg.contains(CLIENT_SECRET)),
                "{args:?}"
            );
        }
        let both = Fake {
            model_key: MODEL_KEY,
            client_secret: CLIENT_SECRET,
            ..Fake::default()
        };
        both.run(&["c1"])?;
        assert_eq!(
            *both.put_with.borrow(),
            [
                format!("model-provider-key:{IMPORT}\n{MODEL_KEY}\n"),
                format!("alpaca-oauth-client-secret:{IMPORT}\n{CLIENT_SECRET}\n"),
            ]
        );
        assert!(both.revoked());
        Ok(())
    }

    #[test]
    fn an_unset_client_secret_writes_nothing() -> Result<()> {
        let fake = Fake {
            present: all_present(&[]),
            model_key: MODEL_KEY,
            ..Fake::default()
        };
        fake.run(&[])?;
        assert!(
            fake.put_with
                .borrow()
                .iter()
                .all(|put| !put.starts_with("alpaca-oauth-client-secret:")),
            "nothing is written, and nothing deleted, for an unset secret"
        );
        Ok(())
    }

    #[test]
    fn a_failed_client_secret_put_fails_the_step() {
        let fake = Fake {
            present: all_present(&[]),
            client_secret: CLIENT_SECRET,
            put_fails: true,
            ..Fake::default()
        };
        assert!(fake.run(&[]).is_err());
    }

    /// DEC-2277: the news data key set is written through the secret-import identity on every run,
    /// as one line `<key id>:<secret>` on the CLI's stdin alone, after the other two, and no part of
    /// it reaches an argument.
    #[test]
    fn a_set_news_data_key_is_written_on_every_run() -> Result<()> {
        const NEWS_KEY: &str = "PKCANARYKEYID:news-secret-canary";
        let fake = Fake {
            present: all_present(&["c1"]),
            news_key: NEWS_KEY,
            ..Fake::default()
        };
        assert_eq!(
            fake.run(&["c1"])?,
            [
                "credentials: present",
                "kubernetes auth: present",
                "model provider key: not set",
                "alpaca oauth client secret: not set",
                "news data key: written"
            ]
        );
        assert_eq!(
            *fake.put_with.borrow(),
            [format!("news-data-key:{IMPORT}\n{NEWS_KEY}\n")]
        );
        for (args, _) in fake.calls.borrow().iter() {
            assert!(
                !args
                    .iter()
                    .any(|arg| arg.contains("canary") || arg.contains("PKCANARY")),
                "{args:?}"
            );
        }
        let failing = Fake {
            present: all_present(&[]),
            news_key: NEWS_KEY,
            put_fails: true,
            ..Fake::default()
        };
        assert!(failing.run(&[]).is_err());
        Ok(())
    }

    /// The two environment secrets make one line, both or neither: a half key, or a part holding
    /// the separator or a line break, fails the step rather than writing a key the reader cannot
    /// use (DEC-2277).
    #[test]
    fn the_news_data_key_is_both_parts_or_nothing() -> Result<()> {
        assert_eq!(news_data_key("", "")?, "");
        assert_eq!(news_data_key("PKID", "secret")?, "PKID:secret");
        for (id, secret) in [
            ("PKID", ""),
            ("", "secret"),
            ("PK:ID", "secret"),
            ("PKID", "sec:ret"),
            ("PKID\n", "secret"),
            ("PKID", "secret\r"),
            ("PKID ", "secret"),
            ("PKID", " secret"),
            ("PKID", "sec\tret"),
        ] {
            assert!(news_data_key(id, secret).is_err(), "{id:?} {secret:?}");
        }
        Ok(())
    }

    #[test]
    fn the_commands_are_the_clis() {
        assert_eq!(
            provision_command(&["c1"], "https://10.43.0.1:443"),
            [
                "provision",
                "--operator-token-stdin",
                "--secrets-namespace",
                "ws-ws1",
                "--kubernetes-host",
                "https://10.43.0.1:443",
                "--connection",
                "c1"
            ]
        );
        assert_eq!(
            kubernetes_command("https://10.43.0.1:443"),
            [
                "provision",
                "--operator-token-stdin",
                "--kubernetes-only",
                "--kubernetes-host",
                "https://10.43.0.1:443"
            ]
        );
        assert_eq!(put_command(MODEL_KEY_SECRET), ["put", "model-provider-key"]);
        assert_eq!(
            put_command(ALPACA_CLIENT_SECRET),
            ["put", "alpaca-oauth-client-secret"]
        );
        assert_eq!(put_command(NEWS_KEY_SECRET), ["put", "news-data-key"]);
    }

    /// A generation that completes with a token the pad cannot decode fails saying the token may
    /// be live, and provisions nothing.
    #[test]
    fn an_undecodable_root_token_fails_saying_it_may_be_live() {
        let fake = Fake {
            garbled_token: true,
            ..Fake::default()
        };
        let error = fake.run(&["c1"]).map(|_| ()).map_err(|e| e.to_string());
        assert!(
            error.as_ref().is_err_and(|e| e.contains(MAY_BE_LIVE)),
            "{error:?}"
        );
        assert!(fake.provisioned_with.borrow().is_empty());
        assert_eq!(fake.calls_with("delete").len(), 2);
    }

    /// A revoke the vault refuses fails the step saying the token may be live.
    #[test]
    fn a_refused_revoke_fails_saying_the_token_may_be_live() {
        let fake = Fake {
            revoke_fails: true,
            ..Fake::default()
        };
        let error = fake.run(&["c1"]).map(|_| ()).map_err(|e| e.to_string());
        assert!(
            error.as_ref().is_err_and(|e| e.contains(MAY_BE_LIVE)),
            "{error:?}"
        );
    }

    /// A revoke whose `kubectl` could not start is tried once more.
    #[test]
    fn a_revoke_whose_kubectl_failed_is_tried_again() -> Result<()> {
        let fake = Fake {
            revoke_spawn_failures: Cell::new(1),
            ..Fake::default()
        };
        fake.run(&["c1"])?;
        assert!(fake.revoked());
        Ok(())
    }
}
