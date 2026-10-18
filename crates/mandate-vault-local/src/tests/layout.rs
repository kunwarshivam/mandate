//! A correct vault layout in a fresh temporary directory, owned by the test's own user, which
//! plays root, `owlhead_api`, and `owlhead_exec` at once.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::startup::{Ids, Inputs, PENDING_KEY_CREDENTIAL, Role, TOKEN_KEY_CREDENTIAL};
use crate::store::{ApiVault, ExecutorVault, open};

pub const PENDING_KEY: [u8; 32] = [7; 32];
pub const TOKEN_KEY: [u8; 32] = [9; 32];

static NEXT: AtomicU32 = AtomicU32::new(0);

pub struct Layout {
    pub root: PathBuf,
    pub vault: PathBuf,
    pub credentials: PathBuf,
    pub ids: Ids,
    pub environment: Vec<(String, String)>,
}

pub fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

impl Layout {
    pub fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("vault-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let vault = root.join("vault");
        let credentials = root.join("credentials");
        for dir in [
            &vault,
            &vault.join("pending"),
            &vault.join("tokens"),
            &credentials,
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        set_mode(&vault, 0o755);
        set_mode(&vault.join("pending"), 0o2770);
        set_mode(&vault.join("tokens"), 0o730);
        fs::write(credentials.join(PENDING_KEY_CREDENTIAL), PENDING_KEY).unwrap();
        fs::write(credentials.join(TOKEN_KEY_CREDENTIAL), TOKEN_KEY).unwrap();
        let meta = fs::metadata(&vault).unwrap();
        let ids = Ids {
            root_uid: meta.uid(),
            api_uid: meta.uid(),
            api_gid: meta.gid(),
            exec_uid: meta.uid(),
            exec_gid: meta.gid(),
        };
        Self {
            root,
            vault,
            credentials,
            ids,
            environment: vec![("PATH".to_owned(), "/usr/bin".to_owned())],
        }
    }

    pub fn inputs(&self, role: Role) -> Inputs<'_> {
        Inputs {
            role,
            vault_dir: &self.vault,
            credentials_dir: &self.credentials,
            environment: &self.environment,
            ids: self.ids,
        }
    }

    /// Both roles over one vault: the executor opens with both keys, then the API with the token
    /// key moved aside, as its own unit would load it.
    pub fn both(&self) -> (ApiVault, ExecutorVault) {
        let executor = open(&self.inputs(Role::Executor))
            .unwrap()
            .into_executor()
            .unwrap();
        let token_key = self.credentials.join(TOKEN_KEY_CREDENTIAL);
        let aside = self.root.join("token-key-aside");
        fs::rename(&token_key, &aside).unwrap();
        let api = open(&self.inputs(Role::Api)).unwrap().into_api().unwrap();
        fs::rename(&aside, &token_key).unwrap();
        (api, executor)
    }

    /// The path of `entry`'s file.
    pub fn file(&self, entry: &crate::entry::Entry) -> PathBuf {
        self.vault.join(entry.kind().dir()).join(entry.file_name())
    }

    /// The names in one vault subdirectory, sorted.
    pub fn names(&self, dir: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.vault.join(dir))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    pub fn without_token_key(self) -> Self {
        fs::remove_file(self.credentials.join(TOKEN_KEY_CREDENTIAL)).unwrap();
        self
    }
}

impl Drop for Layout {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
