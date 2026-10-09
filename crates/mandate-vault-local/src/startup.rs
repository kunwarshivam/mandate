//! The checks the vault makes before it serves anything (DEC-692). Every one refuses rather than
//! repairs: a wrong owner, mode, or key arrangement means the host was set up wrongly, and the
//! vault does not start until it is fixed.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use secrecy::{ExposeSecretMut, SecretBox};

use crate::VaultError;

/// The systemd credential holding the key for `pending/` (authorization codes).
pub const PENDING_KEY_CREDENTIAL: &str = "vault-pending-key";
/// The systemd credential holding the key for `tokens/` (tokens and the client secret). The
/// executor's only; the API process must never be given it.
pub const TOKEN_KEY_CREDENTIAL: &str = "vault-token-key";
/// Environment variables that must never carry a key. Their presence alone refuses startup.
pub const FORBIDDEN_KEY_VARIABLES: [&str; 3] = [
    "MANDATE_VAULT_KEY",
    "MANDATE_VAULT_PENDING_KEY",
    "MANDATE_VAULT_TOKEN_KEY",
];
/// The length of each key, in bytes (ChaCha20-Poly1305).
pub const KEY_LEN: usize = 32;

/// Which process is starting the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The API process (`owlhead_api`): pending key only.
    Api,
    /// The connection's executor (`owlhead_exec`): both keys.
    Executor,
}

/// The numeric ids the layout must carry. On the demo host they are root, `owlhead_api`, and
/// `owlhead_exec`; tests pass their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ids {
    pub root_uid: u32,
    pub api_uid: u32,
    pub api_gid: u32,
    pub exec_uid: u32,
    pub exec_gid: u32,
}

/// Everything startup reads. The environment is passed in, so the check reads no global state.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub role: Role,
    pub vault_dir: &'a Path,
    pub credentials_dir: &'a Path,
    pub environment: &'a [(String, String)],
    pub ids: Ids,
}

/// The keys a started vault holds. The token key is present for the executor only.
pub struct Keys {
    pub pending: SecretBox<[u8; KEY_LEN]>,
    pub token: Option<SecretBox<[u8; KEY_LEN]>>,
}

impl fmt::Debug for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Keys(redacted)")
    }
}

/// Checks the arrangement and loads the keys:
///
/// 1. No [`FORBIDDEN_KEY_VARIABLES`] entry is set, whatever its value.
/// 2. [`PENDING_KEY_CREDENTIAL`] is a regular file of exactly [`KEY_LEN`] bytes in the
///    credentials directory. For [`Role::Executor`] so is [`TOKEN_KEY_CREDENTIAL`]; for
///    [`Role::Api`] that credential must be absent.
/// 3. The vault directory is a real directory (not a symlink) owned by root, mode `0755`;
///    `pending/` is owned by the API user and the executor's group, mode `2770`; `tokens/` is
///    owned by the executor user and the API's group, mode `0730`. Modes compare exactly, the
///    setgid bit included.
pub fn check(inputs: &Inputs<'_>) -> Result<Keys, VaultError> {
    refuse_key_variables(inputs.environment)?;
    check_directories(inputs.vault_dir, &inputs.ids)?;
    let pending = load_key(&inputs.credentials_dir.join(PENDING_KEY_CREDENTIAL))?;
    let token = match inputs.role {
        Role::Api => {
            refuse_token_key(&inputs.credentials_dir.join(TOKEN_KEY_CREDENTIAL))?;
            None
        }
        Role::Executor => Some(load_key(
            &inputs.credentials_dir.join(TOKEN_KEY_CREDENTIAL),
        )?),
    };
    Ok(Keys { pending, token })
}

/// The full `st_mode` each directory must have: the directory type bits (`S_IFDIR`, `0o040000`)
/// with its exact permission bits. Comparing the whole word refuses a symlink or any other file
/// type by the same comparison that refuses a wrong mode.
const VAULT_ST_MODE: u32 = 0o040_755;
/// `pending/`: `2770`, the setgid bit included.
const PENDING_ST_MODE: u32 = 0o042_770;
/// `tokens/`: `0730`.
const TOKENS_ST_MODE: u32 = 0o040_730;

/// The owner, group, and `st_mode` one directory must carry. `group` is `None` where DEC-692
/// item 1 names no group.
struct Expected {
    dir: &'static str,
    owner: u32,
    group: Option<u32>,
    st_mode: u32,
}

/// Refuses startup when any forbidden variable is named, whatever its value. Only names are
/// compared, so no value is read or kept.
fn refuse_key_variables(environment: &[(String, String)]) -> Result<(), VaultError> {
    if environment
        .iter()
        .any(|(name, _)| FORBIDDEN_KEY_VARIABLES.contains(&name.as_str()))
    {
        return Err(VaultError::KeyInEnvironment);
    }
    Ok(())
}

/// Checks the vault directory, then `pending/` and `tokens/`, without following a symlink.
fn check_directories(vault_dir: &Path, ids: &Ids) -> Result<(), VaultError> {
    let expected = [
        (
            vault_dir.to_path_buf(),
            Expected {
                dir: "vault",
                owner: ids.root_uid,
                group: None,
                st_mode: VAULT_ST_MODE,
            },
        ),
        (
            vault_dir.join("pending"),
            Expected {
                dir: "pending",
                owner: ids.api_uid,
                group: Some(ids.exec_gid),
                st_mode: PENDING_ST_MODE,
            },
        ),
        (
            vault_dir.join("tokens"),
            Expected {
                dir: "tokens",
                owner: ids.exec_uid,
                group: Some(ids.api_gid),
                st_mode: TOKENS_ST_MODE,
            },
        ),
    ];
    for (path, want) in &expected {
        check_directory(path, want)?;
    }
    Ok(())
}

/// A directory that cannot be stat'ed, including one that is missing, is a mismatch too.
fn check_directory(path: &Path, want: &Expected) -> Result<(), VaultError> {
    let mismatch = VaultError::DirectoryMismatch { dir: want.dir };
    let meta = fs::symlink_metadata(path).map_err(|_| mismatch.clone())?;
    let group_matches = want.group.is_none_or(|gid| meta.gid() == gid);
    if meta.uid() == want.owner && group_matches && meta.mode() == want.st_mode {
        Ok(())
    } else {
        Err(mismatch)
    }
}

/// The API process must not be given the token key in any form, a symlink included. It runs
/// after the pending key loaded from the same directory, so only a missing entry starts the API;
/// any other failure to stat it refuses (AGENTS.md rule 3).
pub(crate) fn refuse_token_key(path: &Path) -> Result<(), VaultError> {
    match fs::symlink_metadata(path).map_err(|e| e.kind()) {
        Err(ErrorKind::NotFound) => Ok(()),
        Ok(_) => Err(VaultError::TokenKeyInApiCredentials),
        Err(_) => Err(VaultError::Io),
    }
}

/// Reads one key straight into its secret box, so no unzeroized copy is left behind. The
/// credential must be a regular file (not a symlink) of exactly [`KEY_LEN`] bytes: a missing one
/// is [`VaultError::KeyMissing`], a wrong type or length [`VaultError::KeyMalformed`], and any
/// other failure to stat, open, or read it [`VaultError::Io`].
pub(crate) fn load_key(path: &Path) -> Result<SecretBox<[u8; KEY_LEN]>, VaultError> {
    let meta = fs::symlink_metadata(path)
        .map_err(|e| key_failure(&e, ErrorKind::NotFound, VaultError::KeyMissing))?;
    if !meta.file_type().is_file() {
        return Err(VaultError::KeyMalformed);
    }
    let mut file = File::open(path)
        .map_err(|e| key_failure(&e, ErrorKind::NotFound, VaultError::KeyMissing))?;
    let mut key = SecretBox::new(Box::new([0_u8; KEY_LEN]));
    file.read_exact(key.expose_secret_mut())
        .map_err(|e| key_failure(&e, ErrorKind::UnexpectedEof, VaultError::KeyMalformed))?;
    let mut beyond = [0_u8; 1];
    let extra = file
        .read(&mut beyond)
        .map_err(|e| key_failure(&e, ErrorKind::UnexpectedEof, VaultError::KeyMalformed))?;
    if extra == 0 {
        Ok(key)
    } else {
        Err(VaultError::KeyMalformed)
    }
}

/// Every key read maps its I/O failures here, so the stat, open, and both reads share one
/// mapping: only `own`, the one failure that is the key's own fault, becomes `fault`, and any
/// other failure refuses as [`VaultError::Io`] (AGENTS.md rule 3).
fn key_failure(error: &io::Error, own: ErrorKind, fault: VaultError) -> VaultError {
    if error.kind() == own {
        fault
    } else {
        VaultError::Io
    }
}
