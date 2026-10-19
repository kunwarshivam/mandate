//! Staging's vault provisioned from CI (E21-32, DEC-2630): after `vault_init`, inside the same
//! window, staging's apply gives every process its AppRole credentials as a Kubernetes Secret, so
//! nobody runs `mandate vault provision` by hand. When every Secret is already there it changes
//! nothing. Otherwise it makes a root token from two of the recovery keys `vault_init` stored,
//! through the vault's unauthenticated root-generation endpoints on a listener only the pod's
//! loopback reaches, runs `mandate vault provision --secrets-namespace` with that token on its
//! standard input, applies the Secrets it prints through `kubectl`'s standard input, and revokes
//! the token, however the step ends. Every key, one-time pad, and token is masked the moment it
//! exists and is never an argument, an environment variable, a file, or a log line.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
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
/// names them.
const PLATFORM_SECRETS: [&str; 6] = [
    "vault-approle-api",
    "vault-approle-connection-manager",
    "vault-approle-runtime",
    "vault-approle-secret-import",
    "vault-approle-model-gateway",
    "vault-approle-email-sender",
];
/// Revokes the token on standard input, as `vault_init` revokes its own.
const REVOKE: [&str; 3] = [
    "sh",
    "-c",
    "BAO_TOKEN=\"$(cat)\" exec bao token revoke -self",
];

/// What the step needs: `kubectl` against staging, and `mandate vault provision` run against the
/// vault with the token given on its standard input, answering whether it succeeded and the
/// Secrets it printed.
pub(crate) struct Steps<'a> {
    pub(crate) kubectl: Kubectl<'a>,
    pub(crate) provision: &'a dyn Fn(&str) -> Result<(bool, String)>,
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

impl Root<'_, '_> {
    fn revoke(mut self) -> Result<()> {
        let Some(token) = self.token.take() else {
            return Ok(());
        };
        let (revoked, _) = in_vault(self.steps, &REVOKE, Some(&token))?;
        ensure!(
            revoked,
            "revoking the generated root token failed, so it may be live: revoke it with a token \
             generated from the recovery keys in {RECOVERY_NAMESPACE}/{RECOVERY_SECRET}"
        );
        Ok(())
    }
}

impl Drop for Root<'_, '_> {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            in_vault(self.steps, &REVOKE, Some(&token)).ok();
        }
    }
}

/// Makes a root token from the first [`THRESHOLD`] recovery keys, cancelling any attempt an
/// earlier run left open first, and cancelling this one again when it does not complete.
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
    let token = decode_root(encoded, otp)?;
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

/// Builds the `mandate` CLI the step runs, before the control-plane window opens, answering its
/// path.
pub(crate) fn build_cli(root: &Path) -> Result<PathBuf> {
    let status = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "mandate-cli",
            "--bin",
            "mandate",
        ])
        .status()
        .context("starting cargo")?;
    ensure!(status.success(), "building the mandate CLI failed");
    Ok(root.join("target/release/mandate"))
}

/// A `kubectl port-forward` to the vault's pod, stopped when dropped.
struct Forward(Child);

impl Drop for Forward {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// Runs `mandate vault provision --secrets-namespace` at `cli` against the vault through a
/// `kubectl port-forward` that `kubectl` (a command already naming the cluster) starts, trusting
/// the CA from the ConfigMap `vault-ca`, with `token` on its standard input, answering whether it
/// succeeded and the Secrets it printed. Its environment is cleared, and its standard error, which
/// carries reason codes alone, is shown.
pub(crate) fn run_cli(
    kubectl: &dyn Fn() -> Command,
    read: Kubectl<'_>,
    cli: &Path,
    temp: &Path,
    connections: &[String],
    token: &str,
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
    let mut command = Command::new(cli);
    command
        .env_clear()
        .args(["vault", "provision", "--address"])
        .arg(format!("https://127.0.0.1:{port}"))
        .arg("--ca")
        .arg(&ca_file)
        .args([
            "--operator-token-stdin",
            "--secrets-namespace",
            WORKSPACE_NAMESPACE,
        ]);
    for connection in connections {
        command.args(["--connection", connection]);
    }
    let mut run = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("starting mandate vault provision")?;
    run.stdin
        .take()
        .context("the CLI's stdin")?
        .write_all(format!("{token}\n").as_bytes())?;
    let out = run
        .wait_with_output()
        .context("waiting for mandate vault provision")?;
    drop(forward);
    Ok((
        out.status.success(),
        String::from_utf8(out.stdout).context("the CLI printed non-UTF-8")?,
    ))
}

/// Provisions staging's vault for `connections` unless every Secret is already there, answering
/// the status words to print.
pub(crate) fn vault_provision(
    steps: &Steps<'_>,
    connections: &[&str],
) -> Result<Vec<&'static str>> {
    if provisioned(steps, connections)? {
        return Ok(vec!["credentials: present"]);
    }
    let keys = recovery_keys(steps)?;
    let root = root(steps, &keys)?;
    let token = root.token.clone().unwrap_or_default();
    let (provisioned, list) = (steps.provision)(&token)?;
    ensure!(provisioned, "mandate vault provision failed");
    let (applied, _) = (steps.kubectl)(&["apply", "-f", "-"], Some(&list))?;
    ensure!(applied, "applying staging's vault credentials failed");
    root.revoke()?;
    Ok(vec![
        "root token: generated",
        "credentials: provisioned",
        "root token: revoked",
    ])
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use anyhow::anyhow;
    use serde_json::json;

    use super::*;
    use crate::image;

    const ROOT: &str = "s.generated-root-token-123";
    const OTP: &str = "0123456789abcdefghijklmnop";
    const KEYS: [&str; 3] = ["recovery-a", "recovery-b", "recovery-c"];
    const LIST: &str = r#"{"apiVersion":"v1","kind":"List","items":[{"secret":"id-1"}]}"#;

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
        calls: RefCell<Vec<(Vec<String>, Option<String>)>>,
        provisioned_with: RefCell<Vec<String>>,
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
                    let data = if complete {
                        json!({"complete": true, "encoded_token": encoded_root()})
                    } else {
                        json!({"complete": false, "progress": updates})
                    };
                    (true, json!({ "data": data }).to_string())
                }
                "exec" if stdin == Some(ROOT) => (true, String::new()),
                "apply" => (!self.apply_fails, String::new()),
                other => return Err(anyhow!("unexpected kubectl {other}: {args:?}")),
            })
        }

        fn run(&self, connections: &[&str]) -> Result<Vec<&'static str>> {
            let provision = |token: &str| {
                self.provisioned_with.borrow_mut().push(token.to_owned());
                Ok((!self.provision_fails, LIST.to_owned()))
            };
            vault_provision(
                &Steps {
                    kubectl: &|args, stdin| self.kubectl(args, stdin),
                    provision: &provision,
                },
                connections,
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
        assert_eq!(fake.run(&["c1"])?, ["credentials: present"]);
        assert_eq!(fake.calls.borrow().len(), 1);
        assert!(fake.provisioned_with.borrow().is_empty());
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
                "root token: revoked"
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
                REVOKE[2],
            ]
        );
        assert_eq!(*fake.provisioned_with.borrow(), [ROOT]);
        assert_eq!(fake.calls_with("apply")[0].1.as_deref(), Some(LIST));
        assert!(fake.revoked());
        fake.nothing_secret_in_argv();
        Ok(())
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
}
