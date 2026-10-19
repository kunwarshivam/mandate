//! Staging's vault, set up from CI (E21-58, DEC-2052): the founder chose to keep the vault's
//! recovery keys in the cluster, so staging's apply, after lane GO's bootstrap and inside the same
//! window, makes the seal key when there is none, waits for the vault's pod, and initializes the
//! vault once: the whole of `bao operator init`'s answer goes into the Secret
//! `vault-recovery/vault-init`, in a namespace no workload is bound to, and the root token it
//! returns is revoked before the step ends, however it ends. Every key and token is masked the
//! moment it exists, reaches `kubectl` on its standard input alone, and is never printed or
//! written to a file; the log carries status words. Each step is skipped when already done, so
//! every later apply changes nothing.

use anyhow::{Result, bail};

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

/// What the step needs besides `kubectl`: a new seal key, and a pause between reads of the pod.
pub(crate) struct Steps<'a> {
    pub(crate) kubectl: Kubectl<'a>,
    pub(crate) new_key: &'a dyn Fn() -> Result<String>,
    pub(crate) pause: &'a dyn Fn(),
}

/// Sets staging's vault up as far as it is not yet, answering the status words to print.
pub(crate) fn vault_init(_steps: &Steps<'_>) -> Result<Vec<&'static str>> {
    bail!("pending DEC-2052: staging's vault-init is not written yet")
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use anyhow::anyhow;
    use serde_json::json;

    use super::*;
    use crate::image;

    const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const ROOT: &str = "s.root-token-1";
    const RECOVERY: [&str; 3] = ["recovery-a", "recovery-b", "recovery-c"];

    /// A staging cluster as `kubectl` sees it, with the vault in one state or another.
    #[derive(Default)]
    struct Fake {
        seal_exists: bool,
        initialized: bool,
        reads_until_running: u32,
        status_garbled: bool,
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
                "exec" if has("status") => (
                    !self.initialized,
                    if self.status_garbled {
                        "Error checking seal status".to_owned()
                    } else {
                        json!({"initialized": self.initialized, "sealed": !self.initialized})
                            .to_string()
                    },
                ),
                "exec" if has("init") => (
                    true,
                    json!({"recovery_keys_b64": RECOVERY, "root_token": ROOT}).to_string(),
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
        let answer = stored["data"]["init.json"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert!(
            !answer.is_empty() && !answer.contains(ROOT),
            "the answer is stored base64-encoded"
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
            status_garbled: true,
            ..Fake::default()
        };
        assert!(fake.run().is_err());
        assert!(fake.calls_with("init").is_empty());
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
