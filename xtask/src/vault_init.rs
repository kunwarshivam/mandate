//! Staging's vault, set up from CI (E21-58, DEC-2052): the founder chose to keep the vault's
//! recovery keys in the cluster, so staging's apply, after lane GO's bootstrap and inside the same
//! window, makes the seal key when there is none, waits for the vault's pod, and initializes the
//! vault once: `bao operator init`'s recovery keys go into the Secret `vault-recovery/vault-init`,
//! in a namespace no workload is bound to, and the root token it returns is never stored and is
//! revoked before the step ends, however it ends. Every key and token is masked the moment it
//! exists, reaches `kubectl` on its standard input alone, and is never printed or written to a
//! file; the log carries status words. Each step is skipped when already done, so every later
//! apply changes nothing.

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use crate::gitops::{SEAL_KEY_SECRET, SealKey, WORKSPACE_NAMESPACE, ensure_seal_key};
use crate::{image, mask};

/// `kubectl` against staging: its arguments and standard input, answering whether it succeeded
/// and what it printed on standard output.
pub(crate) type Kubectl<'a> = &'a dyn Fn(&[&str], Option<&str>) -> Result<(bool, String)>;

/// The vault's pod and container, as the chart names them.
const POD: &str = "vault-0";
const CONTAINER: &str = "vault";
/// The namespace that holds the vault's init answer, bound to no workload, and the Secret.
pub(crate) const RECOVERY_NAMESPACE: &str = "vault-recovery";
pub(crate) const RECOVERY_SECRET: &str = "vault-init";
/// How many times the step reads the vault's pod before it gives up on it running.
const POD_READS: u32 = 60;
/// How many times it reads the vault's status, since a pod that runs may not listen yet.
const STATUS_READS: u32 = 6;

/// What the step needs besides `kubectl`: a new seal key, and a pause between reads of the pod.
pub(crate) struct Steps<'a> {
    pub(crate) kubectl: Kubectl<'a>,
    pub(crate) new_key: &'a dyn Fn() -> Result<String>,
    pub(crate) pause: &'a dyn Fn(),
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

/// The seal key: kept when the Secret exists, else made, masked, and created from stdin.
fn seal(steps: &Steps<'_>) -> Result<&'static str> {
    let exists = || -> Result<bool> {
        let (read, listed) = (steps.kubectl)(
            &[
                "get",
                "secret",
                SEAL_KEY_SECRET,
                "--namespace",
                WORKSPACE_NAMESPACE,
                "--ignore-not-found",
                "-o",
                "name",
            ],
            None,
        )?;
        ensure!(read, "reading staging's vault seal key failed");
        Ok(!listed.trim().is_empty())
    };
    let made = || -> Result<String> {
        let key = (steps.new_key)()?;
        mask::add_mask(&key);
        Ok(key)
    };
    let create = |manifest: &str| -> Result<()> {
        let (created, _) = (steps.kubectl)(&["create", "-f", "-"], Some(manifest))?;
        ensure!(created, "creating staging's vault seal key failed");
        Ok(())
    };
    Ok(match ensure_seal_key(exists, made, create)? {
        SealKey::Kept => "seal key: exists",
        SealKey::Created => "seal key: created",
    })
}

/// Waits for the vault's pod to run; it waits on its seal key's Secret to mount.
fn running(steps: &Steps<'_>) -> Result<()> {
    for read in 1..=POD_READS {
        let (_, phase) = (steps.kubectl)(
            &[
                "get",
                "pod",
                POD,
                "--namespace",
                WORKSPACE_NAMESPACE,
                "--ignore-not-found",
                "-o",
                "jsonpath={.status.phase}",
            ],
            None,
        )?;
        if phase.trim() == "Running" {
            return Ok(());
        }
        if read < POD_READS {
            (steps.pause)();
        }
    }
    bail!("the vault's pod {WORKSPACE_NAMESPACE}/{POD} never ran, so it was not initialized")
}

/// Whether the vault is initialized, from `bao status`, which exits 2 while sealed but still
/// answers; an answer it cannot read stops the step rather than initializing blind.
fn initialized(steps: &Steps<'_>) -> Result<bool> {
    for read in 1..=STATUS_READS {
        let (_, answer) = in_vault(steps, &["bao", "status", "-format=json"], None)?;
        if let Some(initialized) = serde_json::from_str::<Value>(&answer)
            .ok()
            .and_then(|status| status["initialized"].as_bool())
        {
            return Ok(initialized);
        }
        if read < STATUS_READS {
            (steps.pause)();
        }
    }
    bail!("the vault's status could not be read in {STATUS_READS} tries, so it was not initialized")
}

/// Every string in `value`, each masked before anything can print it.
fn mask_all(value: &Value) {
    match value {
        Value::String(text) => mask::add_mask(text),
        Value::Array(items) => items.iter().for_each(mask_all),
        Value::Object(fields) => fields.values().for_each(mask_all),
        _ => {}
    }
}

/// The root token `bao operator init` returned, revoked when dropped: on success, on an error and
/// on a panic alike. [`RootToken::revoke`] revokes it early and says whether it worked.
struct RootToken<'s, 'a> {
    steps: &'s Steps<'a>,
    token: Option<String>,
}

impl RootToken<'_, '_> {
    fn revoke(mut self) -> Result<()> {
        let Some(token) = self.token.take() else {
            return Ok(());
        };
        let (revoked, _) = in_vault(
            self.steps,
            &[
                "sh",
                "-c",
                "BAO_TOKEN=\"$(cat)\" exec bao token revoke -self",
            ],
            Some(&token),
        )?;
        ensure!(
            revoked,
            "revoking the vault's root token failed, so it may be live: revoke it with a token \
             generated from the recovery keys in {RECOVERY_NAMESPACE}/{RECOVERY_SECRET}"
        );
        Ok(())
    }
}

impl Drop for RootToken<'_, '_> {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            in_vault(
                self.steps,
                &[
                    "sh",
                    "-c",
                    "BAO_TOKEN=\"$(cat)\" exec bao token revoke -self",
                ],
                Some(&token),
            )
            .ok();
        }
    }
}

/// Initializes the vault, stores its answer, root token aside, in the recovery Secret, and
/// revokes the root token, whether or not the answer was stored.
fn init(steps: &Steps<'_>) -> Result<()> {
    let (done, answer) = in_vault(
        steps,
        &[
            "bao",
            "operator",
            "init",
            "-format=json",
            "-recovery-shares=3",
            "-recovery-threshold=2",
        ],
        None,
    )?;
    let parsed = serde_json::from_str::<Value>(&answer).ok();
    if let Some(parsed) = &parsed {
        mask_all(parsed);
    }
    ensure!(done, "initializing the vault failed");
    let mut parsed = parsed.context("the vault's init answer could not be read")?;
    let root = RootToken {
        steps,
        token: parsed
            .as_object_mut()
            .and_then(|fields| fields.remove("root_token"))
            .and_then(|token| token.as_str().map(str::to_owned)),
    };
    let found = root.token.is_some();
    let stored = store(steps, &parsed.to_string());
    let revoked = root.revoke();
    stored?;
    ensure!(
        found,
        "the root token was not found in the vault's init answer, so it may be live: revoke it \
         with a token generated from the recovery keys in {RECOVERY_NAMESPACE}/{RECOVERY_SECRET}"
    );
    revoked
}

/// The vault's init answer, its root token taken out, as the Secret `vault-recovery/vault-init`,
/// created from stdin in a namespace made for it.
fn store(steps: &Steps<'_>, answer: &str) -> Result<()> {
    let namespace = json!({
        "apiVersion": "v1",
        "kind": "Namespace",
        "metadata": {"name": RECOVERY_NAMESPACE},
    });
    let (made, _) = (steps.kubectl)(
        &["apply", "--server-side", "-f", "-"],
        Some(&namespace.to_string()),
    )?;
    ensure!(made, "making the namespace {RECOVERY_NAMESPACE} failed");
    let secret = json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {
            "name": RECOVERY_SECRET,
            "namespace": RECOVERY_NAMESPACE,
            "labels": {"app.kubernetes.io/managed-by": "mandate-infra"},
        },
        "type": "Opaque",
        "data": {"init.json": image::base64(answer.as_bytes())},
    });
    let (created, _) = (steps.kubectl)(&["create", "-f", "-"], Some(&secret.to_string()))?;
    ensure!(
        created,
        "storing the vault's recovery keys in {RECOVERY_NAMESPACE}/{RECOVERY_SECRET} failed"
    );
    Ok(())
}

/// Sets staging's vault up as far as it is not yet, answering the status words to print.
pub(crate) fn vault_init(steps: &Steps<'_>) -> Result<Vec<&'static str>> {
    let mut words = vec![seal(steps)?];
    running(steps)?;
    if initialized(steps)? {
        words.push("vault: already initialized");
        return Ok(words);
    }
    init(steps)?;
    words.extend([
        "vault: initialized",
        "recovery keys: stored",
        "root token: revoked",
    ]);
    Ok(words)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use anyhow::anyhow;
    use serde_json::json;

    use super::*;

    const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const ROOT: &str = "s.root-token-1";
    const RECOVERY: [&str; 3] = ["recovery-a", "recovery-b", "recovery-c"];

    /// A staging cluster as `kubectl` sees it, with the vault in one state or another.
    #[derive(Default)]
    struct Fake {
        seal_exists: bool,
        initialized: bool,
        reads_until_running: u32,
        garbled_reads: u32,
        no_root: bool,
        status_reads: Cell<u32>,
        store_fails: bool,
        revoke_fails: bool,
        reads: Cell<u32>,
        keys_made: Cell<u32>,
        calls: RefCell<Vec<(Vec<String>, Option<String>)>>,
    }

    impl Fake {
        fn kubectl(&self, args: &[&str], stdin: Option<&str>) -> Result<(bool, String)> {
            self.calls.borrow_mut().push((
                args.iter().map(|arg| (*arg).to_owned()).collect(),
                stdin.map(str::to_owned),
            ));
            let has = |word: &str| args.contains(&word);
            let input = stdin.unwrap_or_default();
            Ok(match args.first().copied().unwrap_or_default() {
                "get" if has("secret") => (
                    true,
                    if self.seal_exists {
                        "secret/vault-seal-key\n"
                    } else {
                        ""
                    }
                    .to_owned(),
                ),
                "get" if has("pod") => {
                    self.reads.set(self.reads.get() + 1);
                    let running = self.reads.get() > self.reads_until_running;
                    (true, if running { "Running" } else { "Pending" }.to_owned())
                }
                "apply" => (true, String::new()),
                "create" if input.contains(RECOVERY_SECRET) => (!self.store_fails, String::new()),
                "create" => (true, String::new()),
                "exec" if has("status") => {
                    self.status_reads.set(self.status_reads.get() + 1);
                    let garbled = self.status_reads.get() <= self.garbled_reads;
                    (
                        !self.initialized,
                        if garbled {
                            "Error checking seal status".to_owned()
                        } else {
                            json!({"initialized": self.initialized, "sealed": !self.initialized})
                                .to_string()
                        },
                    )
                }
                "exec" if has("init") => (
                    true,
                    if self.no_root {
                        json!({"recovery_keys_b64": RECOVERY}).to_string()
                    } else {
                        json!({"recovery_keys_b64": RECOVERY, "root_token": ROOT}).to_string()
                    },
                ),
                "exec" if input == ROOT => (!self.revoke_fails, String::new()),
                other => return Err(anyhow!("unexpected kubectl {other}: {args:?}")),
            })
        }

        fn run(&self) -> Result<Vec<&'static str>> {
            let new_key = || {
                self.keys_made.set(self.keys_made.get() + 1);
                Ok(KEY.to_owned())
            };
            vault_init(&Steps {
                kubectl: &|args, stdin| self.kubectl(args, stdin),
                new_key: &new_key,
                pause: &|| {},
            })
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

        /// No key, token or recovery key ever reaches an argument; each reaches stdin alone.
        fn nothing_secret_in_argv(&self) {
            for (args, _) in self.calls.borrow().iter() {
                for secret in [KEY, ROOT].iter().chain(RECOVERY.iter()) {
                    assert!(
                        !args.iter().any(|arg| arg.contains(secret)),
                        "{secret} in {args:?}"
                    );
                }
            }
        }
    }

    /// A fresh staging: the seal key is made and created, the vault initialized, its answer stored
    /// in the recovery Secret, and the root token revoked, all through stdin.
    #[test]
    fn a_fresh_vault_is_set_up_once() -> Result<()> {
        let fake = Fake {
            reads_until_running: 2,
            ..Fake::default()
        };
        assert_eq!(
            fake.run()?,
            [
                "seal key: created",
                "vault: initialized",
                "recovery keys: stored",
                "root token: revoked"
            ]
        );
        assert_eq!(fake.keys_made.get(), 1);
        fake.nothing_secret_in_argv();
        let creates = fake.calls_with("create");
        assert_eq!(creates.len(), 2, "{creates:?}");
        let seal = creates[0].1.clone().unwrap_or_default();
        assert!(seal.contains("vault-seal-key") && seal.contains(&image::base64(KEY.as_bytes())));
        let stored: serde_json::Value =
            serde_json::from_str(&creates[1].1.clone().unwrap_or_default())?;
        assert_eq!(stored["metadata"]["namespace"], RECOVERY_NAMESPACE);
        assert_eq!(stored["metadata"]["name"], RECOVERY_SECRET);
        assert_eq!(
            stored["data"]["init.json"],
            image::base64(
                json!({"recovery_keys_b64": RECOVERY})
                    .to_string()
                    .as_bytes()
            ),
            "the answer is stored base64-encoded, its recovery keys kept and its root token dropped"
        );
        assert!(fake.revoked());
        assert!(
            fake.calls_with("init")[0].0.join(" ").contains(
                "bao operator init -format=json -recovery-shares=3 -recovery-threshold=2"
            )
        );
        Ok(())
    }

    /// A staging already set up is read and left alone: no key is made, nothing is created, the
    /// vault is not initialized again, and there is no token to revoke.
    #[test]
    fn a_set_up_vault_is_left_alone() -> Result<()> {
        let fake = Fake {
            seal_exists: true,
            initialized: true,
            ..Fake::default()
        };
        assert_eq!(
            fake.run()?,
            ["seal key: exists", "vault: already initialized"]
        );
        assert_eq!(fake.keys_made.get(), 0);
        assert!(fake.calls_with("create").is_empty() && fake.calls_with("init").is_empty());
        assert!(!fake.revoked());
        Ok(())
    }

    /// The root token is revoked even when storing the answer fails, and a revoke that fails is an
    /// error that says the token may be live.
    #[test]
    fn the_root_token_is_revoked_on_every_path() -> Result<()> {
        let fake = Fake {
            store_fails: true,
            ..Fake::default()
        };
        let error = fake.run().err().context_free()?;
        assert!(error.contains("recovery"), "{error}");
        assert!(fake.revoked(), "revoked although storing failed");
        fake.nothing_secret_in_argv();

        let fake = Fake {
            revoke_fails: true,
            ..Fake::default()
        };
        let error = fake.run().err().context_free()?;
        assert!(
            error.contains("root token") && error.contains("may be live"),
            "{error}"
        );

        let fake = Fake {
            no_root: true,
            ..Fake::default()
        };
        let error = fake.run().err().context_free()?;
        assert!(
            error.contains("root token was not found") && error.contains("may be live"),
            "{error}"
        );
        assert_eq!(
            fake.calls_with("create").len(),
            2,
            "the recovery keys are still stored"
        );
        Ok(())
    }

    /// A vault whose pod never runs, or whose status cannot be read, is never initialized blind.
    #[test]
    fn it_never_initializes_blind() -> Result<()> {
        let fake = Fake {
            reads_until_running: u32::MAX,
            ..Fake::default()
        };
        assert!(fake.run().is_err());
        assert_eq!(fake.reads.get(), POD_READS);
        assert!(fake.calls_with("init").is_empty());

        let fake = Fake {
            garbled_reads: u32::MAX,
            ..Fake::default()
        };
        assert!(fake.run().is_err());
        assert_eq!(fake.status_reads.get(), STATUS_READS);
        assert!(fake.calls_with("init").is_empty());

        let fake = Fake {
            garbled_reads: STATUS_READS - 1,
            ..Fake::default()
        };
        assert_eq!(
            fake.run()?[1],
            "vault: initialized",
            "a vault not yet listening is read again"
        );
        assert_eq!(fake.calls_with("init").len(), 1);
        Ok(())
    }

    /// An error's text, for a test that expects one.
    trait ContextFree {
        fn context_free(self) -> Result<String>;
    }

    impl ContextFree for Option<anyhow::Error> {
        fn context_free(self) -> Result<String> {
            self.map(|error| format!("{error:#}"))
                .ok_or_else(|| anyhow!("expected an error"))
        }
    }
}
