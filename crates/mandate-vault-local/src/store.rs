//! The encrypted reads and writes of `pending/` and `tokens/` (DEC-692 items 4 and 5).
//!
//! Each entry is one file: a four-byte format tag, a random 12-byte nonce, and the
//! ChaCha20-Poly1305 ciphertext with its tag, under the pending key for a code and the token key
//! for a token or the client secret, with the entry's [`Entry::associated_data`].
//!
//! A write goes to a fresh temporary file in the same directory, opened with `O_EXCL`, set to
//! its exact mode on the open file, synced, and renamed over the entry, so a reader sees the old
//! entry or the new one and never a part.
//!
//! A read checks the file before it decrypts anything. The name must be a regular file, not a
//! symlink; the open file must be that same inode (same device and inode number as the name's
//! `lstat`), so a name swapped between the check and the open is refused; and the open file must
//! have exactly one link, the owner and mode its kind requires, and a size the format allows:
//!
//! | Kind | Owner | Group | `st_mode` |
//! |---|---|---|---|
//! | Code | the API user | the executor's group | `0100640` |
//! | Token, client secret | the executor user | any | `0100600` |

use std::fmt;
use std::fs::{self, File, Metadata, OpenOptions, Permissions};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use rustix::fs::{CWD, RenameFlags, renameat_with};
use secrecy::zeroize::Zeroize;
use secrecy::{ExposeSecret, SecretBox, SecretSlice};

use crate::VaultError;
use crate::entry::{Entry, Kind};
use crate::startup::{self, Ids, Inputs, KEY_LEN, Keys, Role};

/// The longest secret an entry holds, in bytes.
pub const MAX_SECRET_LEN: usize = 8192;
/// The format tag every entry file starts with.
pub const FORMAT_TAG: [u8; 4] = *b"MVL\x01";
/// The nonce length of ChaCha20-Poly1305, in bytes.
pub const NONCE_LEN: usize = 12;
/// The authentication tag length of ChaCha20-Poly1305, in bytes.
pub const TAG_LEN: usize = 16;
/// The bytes before the ciphertext: the format tag and the nonce.
const HEADER_LEN: usize = 16;
/// The longest entry file: the header, the longest secret, and the tag.
const MAX_FILE_LEN: usize = 8224;
/// The shortest entry file: the header, a one-byte secret, and the tag.
const MIN_FILE_LEN: usize = 33;
const _: () = assert!(
    HEADER_LEN == FORMAT_TAG.len() + NONCE_LEN
        && MAX_FILE_LEN == HEADER_LEN + MAX_SECRET_LEN + TAG_LEN
        && MIN_FILE_LEN == HEADER_LEN + 1 + TAG_LEN,
    "the entry file lengths follow from the format"
);

/// The vault as the API process holds it: the pending key only. It writes codes and unlinks a
/// code or a token by name; it reads nothing.
pub struct ApiVault {
    vault_dir: PathBuf,
    pending_key: SecretBox<[u8; KEY_LEN]>,
}

/// The vault as the executor holds it: both keys. It reads codes, and writes and reads tokens
/// and the client secret.
pub struct ExecutorVault {
    vault_dir: PathBuf,
    ids: Ids,
    pending_key: SecretBox<[u8; KEY_LEN]>,
    token_key: SecretBox<[u8; KEY_LEN]>,
}

/// A started vault, in the role its inputs named.
#[derive(Debug)]
pub enum Opened {
    Api(ApiVault),
    Executor(ExecutorVault),
}

impl fmt::Debug for ApiVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiVault(redacted)")
    }
}

impl fmt::Debug for ExecutorVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ExecutorVault(redacted)")
    }
}

/// Runs [`crate::startup::check`] and returns the vault in the role `inputs` names.
pub fn open(inputs: &Inputs<'_>) -> Result<Opened, VaultError> {
    let Keys { pending, token } = startup::check(inputs)?;
    let vault_dir = inputs.vault_dir.to_path_buf();
    let ids = inputs.ids;
    match (inputs.role, token) {
        (Role::Api, _) => Ok(Opened::Api(ApiVault {
            vault_dir,
            pending_key: pending,
        })),
        (Role::Executor, Some(token_key)) => Ok(Opened::Executor(ExecutorVault {
            vault_dir,
            ids,
            pending_key: pending,
            token_key,
        })),
        (Role::Executor, None) => Err(VaultError::KeyMissing),
    }
}

/// The refusal for a vault held in the other role.
const WRONG_VAULT: VaultError = VaultError::WrongRole { kind: "vault" };

impl Opened {
    /// The executor's vault, or [`VaultError::WrongRole`] for the API's.
    pub fn into_executor(self) -> Result<ExecutorVault, VaultError> {
        match self {
            Self::Executor(vault) => Ok(vault),
            Self::Api(_) => Err(WRONG_VAULT),
        }
    }

    /// The API's vault, or [`VaultError::WrongRole`] for the executor's.
    pub fn into_api(self) -> Result<ApiVault, VaultError> {
        match self {
            Self::Api(vault) => Ok(vault),
            Self::Executor(_) => Err(WRONG_VAULT),
        }
    }
}

impl ApiVault {
    /// Writes a code entry, replacing any earlier one. Any other kind is
    /// [`VaultError::WrongRole`].
    pub fn put_code(&self, entry: &Entry, secret: &SecretSlice<u8>) -> Result<(), VaultError> {
        if entry.kind() != Kind::Code {
            return Err(wrong_role(entry));
        }
        write_entry(&self.vault_dir, &self.pending_key, entry, secret)
    }

    /// Unlinks a code or a token by name; an absent entry is already deleted. The client secret
    /// is [`VaultError::WrongRole`].
    pub fn delete(&self, entry: &Entry) -> Result<(), VaultError> {
        if entry.kind() == Kind::ClientSecret {
            return Err(wrong_role(entry));
        }
        let path = entry_path(&self.vault_dir, entry);
        unlink(&path)?;
        sweep_taken(&path, entry)
    }
}

impl ExecutorVault {
    /// Writes a token or the client secret, replacing any earlier one. A code is
    /// [`VaultError::WrongRole`]: only the API writes codes, so only its files pass a code read.
    pub fn put(&self, entry: &Entry, secret: &SecretSlice<u8>) -> Result<(), VaultError> {
        if entry.kind() == Kind::Code {
            return Err(wrong_role(entry));
        }
        write_entry(&self.vault_dir, &self.token_key, entry, secret)
    }

    /// Reads and decrypts an entry of any kind; `None` when it is absent.
    pub fn read(&self, entry: &Entry) -> Result<Option<SecretSlice<u8>>, VaultError> {
        let path = entry_path(&self.vault_dir, entry);
        let Some(bytes) = read_checked(&path, entry.kind(), &self.ids)? else {
            return Ok(None);
        };
        let key = match entry.kind() {
            Kind::Code => &self.pending_key,
            Kind::Token | Kind::ClientSecret => &self.token_key,
        };
        unseal(key, entry, bytes).map(Some)
    }

    /// Unlinks an entry of any kind, and any name a take left beside it; an absent entry is
    /// already deleted.
    pub fn delete(&self, entry: &Entry) -> Result<(), VaultError> {
        let path = entry_path(&self.vault_dir, entry);
        unlink(&path)?;
        sweep_taken(&path, entry)
    }

    /// Takes a code: renames it to a fresh name of 16 random bytes, reads and decrypts it there,
    /// and unlinks it. The rename is atomic and never replaces an existing name, so of any number
    /// of concurrent takers exactly one gets the code and the rest `None` (DEC-2628 item 2). An
    /// entry this call cannot decrypt, such as one bound to another connection, is renamed back
    /// and refused, so a wrong taker destroys nothing; it hides the code from the rightful taker
    /// only for that moment. The rename back never replaces a code written meanwhile
    /// ([`put_back`]). A delete whose sweep unlinks the take name while a take is reading it fails
    /// the take closed: the take finds nothing and answers `None`, which the exchange reads as
    /// `CodeMissing`.
    pub fn take_code(&self, entry: &Entry) -> Result<Option<SecretSlice<u8>>, VaultError> {
        let path = entry_path(&self.vault_dir, entry);
        let taken = taken_path(&path, entry)?;
        self.take_code_into(entry, &taken)
    }

    /// [`ExecutorVault::take_code`] into `taken`, which must not exist: an existing name there is
    /// refused and left as it was.
    pub(crate) fn take_code_into(
        &self,
        entry: &Entry,
        taken: &Path,
    ) -> Result<Option<SecretSlice<u8>>, VaultError> {
        if entry.kind() != Kind::Code {
            return Err(wrong_role(entry));
        }
        let path = entry_path(&self.vault_dir, entry);
        let taken = taken.to_path_buf();
        match renameat_with(CWD, &path, CWD, &taken, RenameFlags::NOREPLACE) {
            Ok(()) => {}
            Err(e) if e == rustix::io::Errno::NOENT => return Ok(None),
            Err(_) => return Err(VaultError::Io),
        }
        let read = read_checked(&taken, Kind::Code, &self.ids).and_then(|bytes| {
            bytes
                .map(|bytes| unseal(&self.pending_key, entry, bytes))
                .transpose()
        });
        match read {
            Ok(secret) => unlink(&taken).map(|()| secret),
            Err(error) => {
                put_back(&taken, &path);
                Err(error)
            }
        }
    }
}

/// Renames a refused take at `taken` back to `path`, never replacing a code the API wrote there
/// meanwhile: when one was, that newer code stays and the refused entry is unlinked. A rename that
/// fails otherwise leaves the refused entry under its take name, for the next delete's sweep.
pub(crate) fn put_back(taken: &Path, path: &Path) {
    if let Err(rustix::io::Errno::EXIST) =
        renameat_with(CWD, taken, CWD, path, RenameFlags::NOREPLACE)
    {
        let _swept_by_the_next_delete_if_this_fails = unlink(taken);
    }
}

/// The prefix of every name a take of `entry` uses.
fn taken_prefix(entry: &Entry) -> String {
    format!("{}.taken-", entry.file_name())
}

/// A fresh name beside `path` for one take: [`taken_prefix`] and 16 random bytes in hex.
fn taken_path(path: &Path, entry: &Entry) -> Result<PathBuf, VaultError> {
    let mut random = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| VaultError::Random)?;
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(path.with_file_name(format!("{}{suffix}", taken_prefix(entry))))
}

/// Unlinks every name a take of `entry` left beside `path`, as a take that stopped between its
/// rename and its unlink leaves one.
pub(crate) fn sweep_taken(path: &Path, entry: &Entry) -> Result<(), VaultError> {
    let Some(dir) = path.parent() else {
        return Err(VaultError::Io);
    };
    let prefix = taken_prefix(entry);
    let names = match fs::read_dir(dir) {
        Ok(names) => names,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(VaultError::Io),
    };
    for name in names {
        let name = name.map_err(|_| VaultError::Io)?;
        if name.file_name().to_string_lossy().starts_with(&prefix) {
            unlink(&name.path())?;
        }
    }
    Ok(())
}

fn wrong_role(entry: &Entry) -> VaultError {
    VaultError::WrongRole {
        kind: entry.kind().as_str(),
    }
}

fn entry_path(vault_dir: &Path, entry: &Entry) -> PathBuf {
    vault_dir.join(entry.kind().dir()).join(entry.file_name())
}

/// The owner, group, and `st_mode` the open file of one kind must carry. `group` is `None`
/// where the mode grants the group nothing.
struct FileRule {
    owner: u32,
    group: Option<u32>,
    st_mode: u32,
}

/// The permission bits a write gives an entry of `kind`: the bits of its rule's `st_mode`.
fn create_mode(kind: Kind) -> u32 {
    match kind {
        Kind::Code => 0o640,
        Kind::Token | Kind::ClientSecret => 0o600,
    }
}

fn file_rule(kind: Kind, ids: &Ids) -> FileRule {
    match kind {
        Kind::Code => FileRule {
            owner: ids.api_uid,
            group: Some(ids.exec_gid),
            st_mode: 0o100_640,
        },
        Kind::Token | Kind::ClientSecret => FileRule {
            owner: ids.exec_uid,
            group: None,
            st_mode: 0o100_600,
        },
    }
}

/// Checks an entry's file before any byte of it is decrypted: `named` is the name's `lstat`,
/// `opened` the open file's `fstat`. They must be one inode, a regular file with one link, at
/// the owner and `st_mode` the table above gives `kind`.
pub(crate) fn check_open_file(
    kind: Kind,
    ids: &Ids,
    named: &Metadata,
    opened: &Metadata,
) -> Result<(), VaultError> {
    let rule = file_rule(kind, ids);
    let same_inode = named.dev() == opened.dev() && named.ino() == opened.ino();
    let group_matches = rule.group.is_none_or(|gid| opened.gid() == gid);
    if same_inode
        && named.file_type().is_file()
        && opened.nlink() == 1
        && opened.uid() == rule.owner
        && group_matches
        && opened.mode() == rule.st_mode
    {
        Ok(())
    } else {
        Err(VaultError::FileMismatch { dir: kind.dir() })
    }
}

/// Reads an entry's file through [`check_open_file`]; `None` when the name is absent. Only a
/// regular file is opened, so a FIFO or device under the name is refused without blocking. A
/// name deleted between the check and the open is an [`VaultError::Io`] refusal, which a caller
/// retries.
fn read_checked(path: &Path, kind: Kind, ids: &Ids) -> Result<Option<Vec<u8>>, VaultError> {
    let mismatch = VaultError::FileMismatch { dir: kind.dir() };
    let named = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(VaultError::Io),
    };
    if !named.file_type().is_file() {
        return Err(mismatch);
    }
    let file = File::open(path).map_err(|_| VaultError::Io)?;
    let opened = file.metadata().map_err(|_| VaultError::Io)?;
    check_open_file(kind, ids, &named, &opened)?;
    let too_long = u64::try_from(MAX_FILE_LEN).map_or(true, |max| opened.len() > max);
    if too_long {
        return Err(VaultError::EntryMalformed);
    }
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_FILE_LEN.saturating_add(1)).unwrap_or(u64::MAX);
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| VaultError::Io)?;
    Ok(Some(bytes))
}

/// An AEAD key built for one operation from the held key bytes.
fn aead_key(key: &SecretBox<[u8; KEY_LEN]>) -> Result<LessSafeKey, VaultError> {
    UnboundKey::new(&CHACHA20_POLY1305, key.expose_secret())
        .map(LessSafeKey::new)
        .map_err(|_| VaultError::KeyMalformed)
}

/// Encrypts `secret` for `entry` and writes it through a temporary file renamed over the entry.
fn write_entry(
    vault_dir: &Path,
    key: &SecretBox<[u8; KEY_LEN]>,
    entry: &Entry,
    secret: &SecretSlice<u8>,
) -> Result<(), VaultError> {
    let bytes = seal(key, entry, secret.expose_secret())?;
    let dir = vault_dir.join(entry.kind().dir());
    let mode = create_mode(entry.kind());
    let mut suffix = [0_u8; 8];
    SystemRandom::new()
        .fill(&mut suffix)
        .map_err(|_| VaultError::Random)?;
    let mut temp_name = format!(".{}.", entry.file_name());
    for byte in suffix {
        temp_name.push_str(&format!("{byte:02x}"));
    }
    temp_name.push_str(".tmp");
    let temp = dir.join(temp_name);
    let written = write_then_rename(&temp, &dir.join(entry.file_name()), mode, &bytes);
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written.and_then(|()| {
        File::open(&dir)
            .and_then(|d| d.sync_all())
            .map_err(|_| VaultError::Io)
    })
}

fn write_then_rename(
    temp: &Path,
    target: &Path,
    mode: u32,
    bytes: &[u8],
) -> Result<(), VaultError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(temp)
        .map_err(|_| VaultError::Io)?;
    file.set_permissions(Permissions::from_mode(mode))
        .and_then(|()| file.write_all(bytes))
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(temp, target))
        .map_err(|_| VaultError::Io)
}

/// The entry file for `secret`: the format tag, a fresh nonce, the ciphertext and the tag. The
/// plaintext is copied only into the buffer the cipher overwrites in place.
fn seal(
    key: &SecretBox<[u8; KEY_LEN]>,
    entry: &Entry,
    secret: &[u8],
) -> Result<Vec<u8>, VaultError> {
    if secret.is_empty() || secret.len() > MAX_SECRET_LEN {
        return Err(VaultError::SecretSize);
    }
    let mut nonce = [0_u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| VaultError::Random)?;
    let key = aead_key(key)?;
    let mut out = Vec::with_capacity(MAX_FILE_LEN);
    out.extend_from_slice(&FORMAT_TAG);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(secret);
    let sealed = match out.get_mut(HEADER_LEN..) {
        Some(body) => key
            .seal_in_place_separate_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(entry.associated_data()),
                body,
            )
            .map_err(|_| VaultError::SecretSize),
        None => Err(VaultError::SecretSize),
    };
    match sealed {
        Ok(tag) => {
            out.extend_from_slice(tag.as_ref());
            Ok(out)
        }
        Err(e) => {
            out.zeroize();
            Err(e)
        }
    }
}

/// Decrypts an entry file for `entry`. The plaintext is copied once into an exact-size buffer the
/// secret box owns, and the file buffer is zeroed whatever the outcome.
fn unseal(
    key: &SecretBox<[u8; KEY_LEN]>,
    entry: &Entry,
    mut bytes: Vec<u8>,
) -> Result<SecretSlice<u8>, VaultError> {
    let result = unseal_in_place(key, entry, &mut bytes);
    bytes.zeroize();
    result
}

fn unseal_in_place(
    key: &SecretBox<[u8; KEY_LEN]>,
    entry: &Entry,
    bytes: &mut [u8],
) -> Result<SecretSlice<u8>, VaultError> {
    let well_formed = (MIN_FILE_LEN..=MAX_FILE_LEN).contains(&bytes.len())
        && bytes.get(..FORMAT_TAG.len()) == Some(&FORMAT_TAG[..]);
    let nonce: Option<[u8; NONCE_LEN]> = bytes
        .get(FORMAT_TAG.len()..HEADER_LEN)
        .and_then(|n| n.try_into().ok());
    let (Some(nonce), true) = (nonce, well_formed) else {
        return Err(VaultError::EntryMalformed);
    };
    let key = aead_key(key)?;
    let body = bytes
        .get_mut(HEADER_LEN..)
        .ok_or(VaultError::EntryMalformed)?;
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(entry.associated_data()),
            body,
        )
        .map_err(|_| VaultError::EntryRejected)?;
    let mut owned = vec![0_u8; plain.len()];
    owned.copy_from_slice(plain);
    Ok(SecretSlice::from(owned))
}

/// Unlinks `path`; an absent one is already deleted.
fn unlink(path: &Path) -> Result<(), VaultError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err(VaultError::Io),
    }
}
