//! A correct address-store layout in a fresh temporary directory, owned by the test's own user,
//! which plays `owlhead_api` and `owlhead_notify` at once.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::startup::{
    ADDRESS_KEY_CREDENTIAL, ADDRESSES_DIR, Ids, Inputs, open_reader, open_writer,
};
use crate::{AddressReader, AddressRef, AddressWriter, MemberId, UnixNanos, WorkspaceId};

pub const ADDRESS_KEY: [u8; 32] = [5; 32];

pub const WORKSPACE: &str = "ws_01J8Z2";
pub const ALICE: &str = "01J8Z2A0000000000000000001";
pub const BOB: &str = "01J8Z2B0000000000000000002";

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
        let root = std::env::temp_dir().join(format!("addresses-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let vault = root.join("vault");
        let credentials = root.join("credentials");
        for dir in [&vault.join(ADDRESSES_DIR), &credentials] {
            fs::create_dir_all(dir).unwrap();
        }
        set_mode(&vault, 0o755);
        set_mode(&vault.join(ADDRESSES_DIR), 0o2710);
        let meta = fs::metadata(&vault).unwrap();
        let layout = Self {
            ids: Ids {
                root_uid: meta.uid(),
                api_uid: meta.uid(),
                notify_gid: meta.gid(),
            },
            root,
            vault,
            credentials,
            environment: vec![("PATH".to_owned(), "/usr/bin".to_owned())],
        };
        layout.install_key(ADDRESS_KEY_CREDENTIAL, ADDRESS_KEY);
        layout
    }

    pub fn inputs(&self) -> Inputs<'_> {
        Inputs {
            vault_dir: &self.vault,
            credentials_dir: &self.credentials,
            environment: &self.environment,
            ids: self.ids,
        }
    }

    pub fn writer(&self) -> AddressWriter {
        open_writer(&self.inputs()).expect("the writer opens on a correct layout")
    }

    pub fn reader(&self) -> AddressReader {
        open_reader(&self.inputs()).expect("the reader opens on a correct layout")
    }

    /// Installs a key credential as `LoadCredential=` presents it: a regular file, mode `0400`.
    pub fn install_key(&self, name: &str, key: [u8; 32]) {
        let path = self.credentials.join(name);
        if path.exists() {
            set_mode(&path, 0o600);
        }
        fs::write(&path, key).unwrap();
        set_mode(&path, 0o400);
    }

    pub fn workspace_dir(&self, workspace: &str) -> PathBuf {
        self.vault.join(ADDRESSES_DIR).join(workspace)
    }

    /// The file of [`address`]`(n)` in `workspace`.
    pub fn entry_path(&self, workspace: &str, n: u8) -> PathBuf {
        self.workspace_dir(workspace).join(address_text(n))
    }
}

impl Drop for Layout {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::parse(text).expect("a valid workspace id")
}

pub fn member(text: &str) -> MemberId {
    MemberId::parse(text).expect("a valid member id")
}

pub fn address_text(n: u8) -> String {
    format!("01J8Z2R00000000000000000{n:02}")
}

/// The `n`th test address: a canonical ULID that sorts by `n`.
pub fn address(n: u8) -> AddressRef {
    AddressRef::parse(&address_text(n)).expect("a valid address_ref")
}

/// The headers' times, in 2099, so every file a test writes was modified before them: a sweep's
/// cutoff at or after `T0` lists by `written_at` alone unless a test sets a file's time itself.
pub const T0: UnixNanos = UnixNanos(4_100_000_000_000_000_000);
pub const T1: UnixNanos = UnixNanos(4_100_000_001_000_000_000);
pub const T2: UnixNanos = UnixNanos(4_100_000_002_000_000_000);
