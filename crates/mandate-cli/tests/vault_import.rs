//! `mandate vault import` takes its secret from stdin only and prints none of it, whatever
//! refuses it (DEC-692 item 6, CN-1). The host running the tests has no `owlhead_exec`, so every
//! import here is refused; `mandate-vault-local`'s tests cover the stored secret.

use std::io::Write;
use std::process::{Command, Output, Stdio};

const CANARY: &str = "canary-5d1e8b2c-cli-secret-do-not-print";

fn import(args: &[&str], env: &[(&str, &str)], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mandate"))
        .args(args)
        .env_clear()
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the CLI starts");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(stdin.as_bytes())
        .ok();
    child.wait_with_output().expect("the CLI finishes")
}

fn assert_silent_about_the_canary(output: &Output, case: &str) {
    for stream in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(stream);
        assert!(!text.contains(CANARY), "{case} printed the canary: {text}");
        assert!(
            !text.contains(&CANARY[..16]),
            "{case} printed part of the canary: {text}"
        );
    }
}

const IMPORT: [&str; 3] = ["vault", "import", "alpaca-oauth-client-secret"];

#[test]
fn an_import_without_its_credentials_is_refused_silently() {
    let output = import(&IMPORT, &[], CANARY);
    assert!(!output.status.success());
    assert_silent_about_the_canary(&output, "no credentials");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CREDENTIALS_DIRECTORY"),
        "names what is missing: {stderr}"
    );
}

#[test]
fn an_import_on_a_host_without_the_service_users_is_refused_silently() {
    let credentials = std::env::temp_dir();
    let credentials = credentials.to_str().expect("a UTF-8 temporary directory");
    for env in [
        vec![("CREDENTIALS_DIRECTORY", credentials)],
        vec![
            ("CREDENTIALS_DIRECTORY", credentials),
            ("MANDATE_VAULT_TOKEN_KEY", CANARY),
        ],
        vec![
            ("CREDENTIALS_DIRECTORY", credentials),
            ("MANDATE_VAULT_DIR", "/nonexistent/vault"),
        ],
    ] {
        let output = import(&IMPORT, &env, &format!("{CANARY}\n"));
        assert!(!output.status.success(), "{env:?}");
        assert_silent_about_the_canary(&output, &format!("{env:?}"));
    }
}

#[test]
fn the_secret_is_never_an_argument() {
    for args in [
        vec!["vault", "import", "alpaca-oauth-client-secret", CANARY],
        vec!["vault", "import", CANARY],
        vec![
            "vault",
            "import",
            "alpaca-oauth-client-secret",
            "--secret",
            CANARY,
        ],
    ] {
        let output = import(&args, &[], "");
        assert!(!output.status.success(), "{args:?}");
        assert_eq!(output.status.code(), Some(2), "clap refuses {args:?}");
    }
}

#[test]
fn the_import_has_help() {
    let output = import(&["vault", "import", "--help"], &[], "");
    assert!(output.status.success(), "set-secrets.sh probes it");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("alpaca-oauth-client-secret"), "{stdout}");
}

mod in_process {
    //! The import's glue, run in this process over a vault laid out in a temporary directory,
    //! with a user database that names the test's own user as both service users.

    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    use mandate_cli::vault::{ImportArgs, ImportedSecret, UserDatabase, run_import};
    use mandate_vault_local::entry::{ALPACA_CLIENT_SECRET_KEY_REF, Entry, KeyRef};
    use mandate_vault_local::import::{API_USER, EXEC_USER};
    use mandate_vault_local::startup::{Ids, Inputs, Role};
    use mandate_vault_local::store::open;
    use secrecy::ExposeSecret;

    use super::CANARY;

    struct Host {
        root: PathBuf,
        users: UserDatabase,
    }

    fn mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    impl Host {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("mandate-cli-vault-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let vault = root.join("vault");
            for dir in [
                vault.clone(),
                vault.join("pending"),
                vault.join("tokens"),
                root.join("credentials"),
            ] {
                fs::create_dir_all(dir).unwrap();
            }
            mode(&vault, 0o755);
            mode(&vault.join("pending"), 0o2770);
            mode(&vault.join("tokens"), 0o730);
            fs::write(root.join("credentials/vault-pending-key"), [3; 32]).unwrap();
            fs::write(root.join("credentials/vault-token-key"), [4; 32]).unwrap();
            let meta = fs::metadata(&vault).unwrap();
            let (uid, gid) = (meta.uid(), meta.gid());
            let users = UserDatabase {
                passwd: format!(
                    "root:x:{uid}:{gid}::/:/bin/false\n{API_USER}:x:{uid}:{gid}::/:/bin/false\n{EXEC_USER}:x:{uid}:{gid}::/:/bin/false\n"
                ),
                group: format!("{API_USER}:x:{gid}:\n{EXEC_USER}:x:{gid}:\n"),
            };
            Self { root, users }
        }

        fn environment(&self) -> Vec<(String, String)> {
            vec![
                (
                    "CREDENTIALS_DIRECTORY".to_owned(),
                    self.root.join("credentials").display().to_string(),
                ),
                (
                    "MANDATE_VAULT_DIR".to_owned(),
                    self.root.join("vault").display().to_string(),
                ),
            ]
        }

        fn stored(&self) -> Option<Vec<u8>> {
            let environment = Vec::new();
            let meta = fs::metadata(self.root.join("vault")).unwrap();
            let inputs = Inputs {
                role: Role::Executor,
                vault_dir: &self.root.join("vault"),
                credentials_dir: &self.root.join("credentials"),
                environment: &environment,
                ids: Ids {
                    root_uid: meta.uid(),
                    api_uid: meta.uid(),
                    api_gid: meta.gid(),
                    exec_uid: meta.uid(),
                    exec_gid: meta.gid(),
                },
            };
            let vault = open(&inputs).unwrap().into_executor().unwrap();
            let entry = Entry::client_secret(KeyRef::new(ALPACA_CLIENT_SECRET_KEY_REF).unwrap());
            vault
                .read(&entry)
                .unwrap()
                .map(|s| s.expose_secret().to_vec())
        }
    }

    impl Drop for Host {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn args() -> ImportArgs {
        ImportArgs {
            secret: ImportedSecret::AlpacaOauthClientSecret,
        }
    }

    #[test]
    fn the_import_stores_stdin_and_reports_without_it() {
        let host = Host::new("ok");
        let mut out = Vec::new();
        let mut stdin = format!("{CANARY}\n");
        run_import(
            &args(),
            &host.environment(),
            &host.users,
            &mut stdin.as_bytes(),
            &mut out,
        )
        .unwrap();
        stdin.clear();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out, "the Alpaca OAuth client secret is in the vault\n");
        assert_eq!(host.stored().as_deref(), Some(CANARY.as_bytes()));
    }

    #[test]
    fn each_input_the_import_reads_is_its_own() {
        let host = Host::new("inputs");
        let refuse = |environment: &[(String, String)], users: &UserDatabase| {
            let mut out = Vec::new();
            let error = run_import(
                &args(),
                environment,
                users,
                &mut CANARY.as_bytes(),
                &mut out,
            )
            .unwrap_err()
            .to_string();
            assert!(out.is_empty());
            assert!(!error.contains(CANARY), "{error}");
            error
        };
        let mut environment = host.environment();
        environment[1].1 = host.root.join("elsewhere").display().to_string();
        assert!(refuse(&environment, &host.users).contains("directory_mismatch"));
        let mut environment = host.environment();
        environment[0].1 = host.root.join("no-credentials").display().to_string();
        assert!(refuse(&environment, &host.users).contains("key_missing"));
        let mut environment = host.environment();
        environment.push(("MANDATE_VAULT_PENDING_KEY".to_owned(), CANARY.to_owned()));
        assert!(refuse(&environment, &host.users).contains("key_in_environment"));
        let environment = vec![host.environment()[1].clone()];
        assert!(refuse(&environment, &host.users).contains("CREDENTIALS_DIRECTORY"));
        let no_exec = UserDatabase {
            passwd: host
                .users
                .passwd
                .lines()
                .take(2)
                .collect::<Vec<_>>()
                .join("\n"),
            group: host.users.group.clone(),
        };
        assert!(refuse(&host.environment(), &no_exec).contains("unknown_user"));
        let mut stdin_refused = host.environment();
        stdin_refused.reverse();
        let mut out = Vec::new();
        let error = run_import(
            &args(),
            &stdin_refused,
            &host.users,
            &mut format!("{CANARY} {CANARY}").as_bytes(),
            &mut out,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("secret_not_printable"), "{error}");
        assert!(!error.contains(CANARY));
        assert_eq!(host.stored(), None);
    }
}
