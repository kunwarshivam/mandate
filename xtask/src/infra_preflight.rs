//! `cargo xtask infra`'s preflight and its per-run Spaces key (E21-56, DEC-2029 item 4,
//! DEC-2030 items 4 and 5). The preflight reads which credentials and permissions a run needs and
//! prints one checklist that names what is missing, never a value. The Spaces key is minted for
//! one run, handed to OpenTofu only through the child's environment, and deleted when the run
//! ends. Every credential reaches `curl` as a config on its standard input, never on its command
//! line.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::founding::without_step_secrets;
use crate::mask;

pub(crate) const DO_API: &str = "https://api.digitalocean.com/v2";
const CF_API: &str = "https://api.cloudflare.com/client/v4";
/// The region of every environment's state bucket: the US West (DEC-2047 item 6).
const SPACES_REGION: &str = "sfo2";
/// Where the state buckets were before the move, kept as the rollback until the founder deletes
/// them (DEC-2047 item 6).
const LEGACY_SPACES_REGION: &str = "nyc3";
/// The environment variables `infra.yml`'s step sets from the environment's secrets and variables.
const DO_TOKEN_ENV: &str = "DIGITALOCEAN_TOKEN";
const CF_TOKEN_ENV: &str = "TF_VAR_cloudflare_api_token";
const PASSPHRASE_ENV: &str = "TF_VAR_state_passphrase";
pub(crate) const KUBE_CIDRS_ENV: &str = "TF_VAR_kube_api_allowed_addresses";
pub(crate) const EMAILS_ENV: &str = "TF_VAR_access_allowed_emails";

/// The Cloudflare permissions a run needs, each with the group names Cloudflare lists for it
/// (its API says Write, its dashboard Edit, and account-owned tokens prefix API Tokens with
/// Account).
const CF_NEEDED: &[(&str, &[&str])] = &[
    (
        "API Tokens Edit",
        &[
            "API Tokens Write",
            "API Tokens Edit",
            "Account API Tokens Write",
            "Account API Tokens Edit",
        ],
    ),
    (
        "Cloudflare Tunnel Edit",
        &["Cloudflare Tunnel Write", "Cloudflare Tunnel Edit"],
    ),
    (
        "Access: Apps and Policies Edit",
        &[
            "Access: Apps and Policies Write",
            "Access: Apps and Policies Edit",
        ],
    ),
    (
        "Workers R2 Storage Edit",
        &["Workers R2 Storage Write", "Workers R2 Storage Edit"],
    ),
    ("DNS Edit", &["DNS Write", "DNS Edit"]),
    ("Zone Read", &["Zone Read", "Zone Write", "Zone Edit"]),
];

/// The DigitalOcean read scopes the preflight probes, each by one list call.
const DO_READ_PROBES: &[(&str, &str)] = &[
    ("Kubernetes read", "/kubernetes/clusters?per_page=1"),
    ("VPC read", "/vpcs?per_page=1"),
    ("Firewall read", "/firewalls?per_page=1"),
    ("Spaces Keys read", "/spaces/keys?per_page=1"),
];

/// One line of the checklist: what was checked, and whether it holds. A reminder is a step no API
/// can read; it never fails the preflight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    Ready(String),
    Missing(String),
    Reminder(String),
    /// A line that does not hold yet but waits on the change `until` names, so it does not fail.
    Pending {
        until: &'static str,
        what: String,
    },
}

/// The checklist the preflight prints, in the order it was checked.
#[derive(Debug, Default)]
pub(crate) struct Checklist {
    items: Vec<Item>,
}

impl Checklist {
    pub(crate) fn check(&mut self, holds: bool, what: impl Into<String>) {
        let what = what.into();
        self.items.push(if holds {
            Item::Ready(what)
        } else {
            Item::Missing(what)
        });
    }

    /// A line that holds, or else waits on `until` without failing the checklist.
    pub(crate) fn check_or_pending(
        &mut self,
        holds: bool,
        until: &'static str,
        what: impl Into<String>,
    ) {
        let what = what.into();
        self.items.push(if holds {
            Item::Ready(what)
        } else {
            Item::Pending { until, what }
        });
    }

    fn remind(&mut self, what: impl Into<String>) {
        self.items.push(Item::Reminder(what.into()));
    }

    /// Whether every checked line holds.
    pub(crate) fn complete(&self) -> bool {
        !self
            .items
            .iter()
            .any(|item| matches!(item, Item::Missing(_)))
    }

    /// The checklist as printed: one line each, missing lines marked so a reader finds them.
    pub(crate) fn render(&self, environment: &str) -> String {
        self.render_titled(&format!("infra preflight ({environment})"))
    }

    /// The checklist under `title`, as [`Checklist::render`] prints the preflight's.
    pub(crate) fn render_titled(&self, title: &str) -> String {
        let mut out = format!("{title}:\n");
        for item in &self.items {
            let (mark, what) = match item {
                Item::Ready(what) => ("[ok]      ".to_owned(), what),
                Item::Missing(what) => ("[MISSING] ".to_owned(), what),
                Item::Reminder(what) => ("[console] ".to_owned(), what),
                Item::Pending { until, what } => (format!("[pending {until}] "), what),
            };
            out.push_str("  ");
            out.push_str(&mark);
            out.push_str(what);
            out.push('\n');
        }
        out
    }
}

/// A Spaces key minted for one run. It has no `Debug`, so no format string can print its secret.
pub(crate) struct SpacesKey {
    pub(crate) access_key: String,
    pub(crate) secret_key: String,
}

/// This run's Spaces key, if one was minted, deleted when the guard is dropped: on success, on an
/// error, and on a panic alike (DEC-2030 item 4). [`KeyGuard::release`] deletes it early and says
/// whether DigitalOcean confirmed it.
pub(crate) struct KeyGuard {
    token: String,
    key: Option<SpacesKey>,
}

impl KeyGuard {
    fn none() -> Self {
        Self {
            token: String::new(),
            key: None,
        }
    }

    /// The key, while the guard holds one.
    pub(crate) fn key(&self) -> Option<&SpacesKey> {
        self.key.as_ref()
    }

    /// Deletes the key now; `true` when there was none or DigitalOcean confirmed the deletion. The
    /// key stays in the guard until the deletion is confirmed, so a failed request leaves it to
    /// [`Drop`] to try again.
    pub(crate) fn release(mut self) -> Result<bool> {
        let Some(key) = self.key.as_ref() else {
            return Ok(true);
        };
        let deleted = delete_key(&self.token, &key.access_key)?;
        if deleted {
            self.key = None;
        }
        Ok(deleted)
    }
}

impl Drop for KeyGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take()
            && !delete_key(&self.token, &key.access_key).unwrap_or(false)
        {
            let _ = writeln!(
                std::io::stderr(),
                "    this run's Spaces key was not deleted; `infra close` and the next run delete it \
                 (DEC-2030 item 4)"
            );
        }
    }
}

/// What the preflight leaves the run: its checklist, and the guard of the run's Spaces key.
pub(crate) struct Preflight {
    pub(crate) checklist: Checklist,
    pub(crate) key: KeyGuard,
}

/// The value of `name`, when set and not blank.
fn present(read: &dyn Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    read(name).filter(|value| !value.trim().is_empty())
}

/// Whether `value`, a JSON list as `infra.yml` hands it on (`KUBE_API_ALLOWED_CIDRS`,
/// `ACCESS_ALLOWED_EMAILS`), names nothing: unset, blank, or an empty JSON list.
pub(crate) fn no_cidrs(value: Option<&str>) -> bool {
    crate::names_nothing(value)
}

/// The checks that read only the step's environment: each secret by name, and production's
/// CIDRs (DEC-2030 items 3 and 5).
pub(crate) fn environment_items(
    environment: &str,
    read: &dyn Fn(&str) -> Option<String>,
) -> Checklist {
    let mut list = Checklist::default();
    let passphrase = present(read, PASSPHRASE_ENV);
    list.check(passphrase.is_some(), "secret TOFU_STATE_PASSPHRASE is set");
    if let Some(passphrase) = passphrase {
        list.check(
            passphrase.chars().count() >= 32,
            "secret TOFU_STATE_PASSPHRASE is at least 32 characters",
        );
    }
    list.check(
        present(read, DO_TOKEN_ENV).is_some(),
        "secret DIGITAL_OCEAN_API_KEY is set",
    );
    list.check(
        present(read, CF_TOKEN_ENV).is_some(),
        "secret CF_INFRA_API_TOKEN is set",
    );
    if environment == "production" {
        list.check(
            !no_cidrs(read(KUBE_CIDRS_ENV).as_deref()),
            "secret KUBE_API_ALLOWED_CIDRS lists at least one CIDR (production only)",
        );
        list.check(
            !no_cidrs(read(EMAILS_ENV).as_deref()),
            "secret ACCESS_ALLOWED_EMAILS lists at least one email (production only)",
        );
    }
    list
}

/// The needed Cloudflare permissions, by the name the runbook uses, that none of `groups` grants.
pub(crate) fn cloudflare_gaps(groups: &BTreeSet<String>) -> Vec<&'static str> {
    CF_NEEDED
        .iter()
        .filter(|(_, names)| !names.iter().any(|name| groups.contains(*name)))
        .map(|(label, _)| *label)
        .collect()
}

/// The prefix of every key this environment's runs mint, and so of every key they may delete.
pub(crate) fn key_prefix(environment: &str) -> String {
    format!("mandate-infra-{environment}-")
}

/// The keys in a DigitalOcean key listing whose names carry `prefix`, by access key.
pub(crate) fn keys_named(listing: &str, prefix: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Listing {
        keys: Vec<Listed>,
    }
    #[derive(Deserialize)]
    struct Listed {
        name: String,
        access_key: String,
    }
    let listing: Listing =
        serde_json::from_str(listing).context("DigitalOcean's key listing is not its JSON")?;
    Ok(listing
        .keys
        .into_iter()
        .filter(|key| key.name.starts_with(prefix))
        .map(|key| key.access_key)
        .collect())
}

/// How many keys one page of a DigitalOcean key listing holds.
pub(crate) fn listed_count(listing: &str) -> Result<usize> {
    let listing: serde_json::Value =
        serde_json::from_str(listing).context("DigitalOcean's key listing is not its JSON")?;
    Ok(listing
        .get("keys")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len))
}

/// The keys one page of the listing may hold, and the most pages read before the listing is
/// taken as unbounded rather than read forever.
const KEYS_PER_PAGE: usize = 200;
const MAX_KEY_PAGES: usize = 50;

/// Every Spaces key whose name carries `prefix`, by access key, read page by page until a page
/// is short; `None` when DigitalOcean refuses the listing (Spaces Keys read is missing).
fn keys_with_prefix(token: &str, prefix: &str) -> Result<Option<Vec<String>>> {
    let mut keys = Vec::new();
    for page in 1..=MAX_KEY_PAGES {
        let answer = api(
            "GET",
            &format!("{DO_API}/spaces/keys?page={page}&per_page={KEYS_PER_PAGE}"),
            token,
            None,
        )?;
        if !answer.ok() {
            return Ok(None);
        }
        keys.extend(keys_named(&answer.body, prefix)?);
        if listed_count(&answer.body)? < KEYS_PER_PAGE {
            return Ok(Some(keys));
        }
    }
    bail!("DigitalOcean listed more than {MAX_KEY_PAGES} pages of Spaces keys")
}

/// `value` as a double-quoted curl config string; a line break, which would end the option early,
/// is refused.
pub(crate) fn config_quoted(value: &str) -> Result<String> {
    ensure!(
        !value.contains(['\n', '\r']),
        "a curl config value holds a line break"
    );
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// One HTTP answer.
pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) body: String,
}

impl Answer {
    pub(crate) fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Runs `curl` with `config` on its standard input, so no credential is on a command line, and
/// returns the status and body. Only a request that is safe to repeat is retried.
fn curl(config: &str, retry: bool) -> Result<Answer> {
    let mut child = without_step_secrets(&mut Command::new("curl"))
        .args(["-sS", "--max-time", "60", "-K", "-"])
        .args(if retry {
            ["--retry", "3"].as_slice()
        } else {
            [].as_slice()
        })
        .args(["-w", "\n%{http_code}"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("starting `curl`")?;
    child
        .stdin
        .take()
        .context("curl's standard input")?
        .write_all(config.as_bytes())
        .context("writing curl's config")?;
    let out = child.wait_with_output().context("waiting for `curl`")?;
    ensure!(
        out.status.success(),
        "curl failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let text = String::from_utf8(out.stdout).context("curl printed non-UTF-8")?;
    let (body, code) = text.rsplit_once('\n').context("curl printed no status")?;
    Ok(Answer {
        status: code
            .trim()
            .parse()
            .context("curl's status is not a number")?,
        body: body.to_owned(),
    })
}

/// A `GET` to a public endpoint that takes no credential, such as the runner's address echo.
pub(crate) fn get_public(url: &str) -> Result<Answer> {
    curl(&format!("url = {}\n", config_quoted(url)?), true)
}

/// A request to a token-authorised JSON API. A `POST` is sent once, never retried, so a lost
/// answer never mints a second key.
pub(crate) fn api(method: &str, url: &str, token: &str, body: Option<&str>) -> Result<Answer> {
    let mut config = format!(
        "url = {}\nrequest = {}\nheader = {}\nheader = {}\n",
        config_quoted(url)?,
        config_quoted(method)?,
        config_quoted(&format!("Authorization: Bearer {token}"))?,
        config_quoted("Content-Type: application/json")?,
    );
    if let Some(body) = body {
        config.push_str(&format!("data-binary = {}\n", config_quoted(body)?));
    }
    curl(&config, method != "POST")
}

/// The curl config that signs a request to `path` in `region`'s Spaces with SigV4.
fn spaces_config(region: &str, path: &str, key: &SpacesKey) -> Result<String> {
    Ok(format!(
        "url = {}\naws-sigv4 = {}\nuser = {}\n",
        config_quoted(&format!("https://{region}.digitaloceanspaces.com/{path}"))?,
        config_quoted(&format!("aws:amz:{region}:s3"))?,
        config_quoted(&format!("{}:{}", key.access_key, key.secret_key))?,
    ))
}

/// A SigV4-signed request for the state bucket itself: `HEAD` to ask whether it exists, `PUT` to
/// make it.
fn bucket(method: &str, name: &str, key: &SpacesKey) -> Result<u16> {
    let mut config = spaces_config(SPACES_REGION, name, key)?;
    if method == "HEAD" {
        config.push_str("head\n");
    } else {
        config.push_str(&format!(
            "request = {}\ndata-binary = \"\"\n",
            config_quoted(method)?
        ));
    }
    Ok(curl(&config, true)?.status)
}

/// Deletes the Spaces key `access_key`; whether DigitalOcean confirmed it.
pub(crate) fn delete_key(token: &str, access_key: &str) -> Result<bool> {
    Ok(api(
        "DELETE",
        &format!("{DO_API}/spaces/keys/{access_key}"),
        token,
        None,
    )?
    .ok())
}

/// Mints this run's key, full access, since only that grant may create a bucket (DEC-2030 item 4),
/// straight into a [`KeyGuard`]. The answer is read as untyped JSON, whose errors cannot echo a
/// value. The access key id is masked and held by the guard the moment it is read, and the secret
/// is masked before anything else touches it, so a later error deletes the key and no log line can
/// show either (DEC-2018 item 7). An answer with no access key id at all leaves nothing to delete
/// here; `infra close` deletes every key of the environment's prefix.
fn mint_key(token: &str, name: &str) -> Result<KeyGuard> {
    let body = serde_json::json!({
        "name": name,
        "grants": [{ "bucket": "", "permission": "fullaccess" }],
    })
    .to_string();
    let answer = api("POST", &format!("{DO_API}/spaces/keys"), token, Some(&body))?;
    if !answer.ok() {
        return Ok(KeyGuard::none());
    }
    let minted = serde_json::from_str::<serde_json::Value>(&answer.body).unwrap_or_default();
    let field = |name: &str| {
        minted
            .get("key")
            .and_then(|key| key.get(name))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let Some(access_key) = field("access_key") else {
        bail!("DigitalOcean's minted key names no access key id; `infra close` deletes it");
    };
    mask::add_mask(&access_key);
    let mut guard = KeyGuard {
        token: token.to_owned(),
        key: Some(SpacesKey {
            access_key,
            secret_key: String::new(),
        }),
    };
    let Some(secret_key) = field("secret_key") else {
        bail!("DigitalOcean's minted key holds no secret; this run's guard deletes it");
    };
    mask::add_mask(&secret_key);
    if let Some(key) = guard.key.as_mut() {
        key.secret_key = secret_key;
    }
    Ok(guard)
}

/// The DigitalOcean half: the token, its read scopes, last run's keys deleted, this run's key
/// minted into a [`KeyGuard`] at once, and the state bucket found or, when `make_bucket`, made. An
/// error after the mint drops the guard, which deletes the key.
fn digitalocean(
    list: &mut Checklist,
    environment: &str,
    run: &str,
    token: &str,
    make_bucket: bool,
) -> Result<KeyGuard> {
    let accepted = api("GET", &format!("{DO_API}/account"), token, None)?.ok();
    list.check(
        accepted,
        "DIGITAL_OCEAN_API_KEY is accepted by DigitalOcean",
    );
    if !accepted {
        return Ok(KeyGuard::none());
    }
    let mut can_list = false;
    for (scope, path) in DO_READ_PROBES {
        let answer = api("GET", &format!("{DO_API}{path}"), token, None)?;
        list.check(
            answer.ok(),
            format!("DIGITAL_OCEAN_API_KEY has scope {scope}"),
        );
        if *scope == "Spaces Keys read" {
            can_list = answer.ok();
        }
    }
    let prefix = key_prefix(environment);
    if can_list
        && let Some(stale) = keys_with_prefix(token, &prefix)?
        && !stale.is_empty()
    {
        let mut deleted = true;
        for access_key in &stale {
            deleted &= delete_key(token, access_key)?;
        }
        list.check(
            deleted,
            format!(
                "DIGITAL_OCEAN_API_KEY has scope Spaces Keys delete (removing an earlier \
                 run's {prefix}* key)"
            ),
        );
    }
    let guard = mint_key(token, &format!("{prefix}{run}"))?;
    list.check(
        guard.key().is_some(),
        "DIGITAL_OCEAN_API_KEY has scope Spaces Keys create",
    );
    if let Some(key) = guard.key() {
        state_bucket(list, environment, key, make_bucket)?;
    }
    Ok(guard)
}

/// The environment's state bucket in the US West, and the one in nyc3 it replaces (DEC-2047 item 6).
pub(crate) fn state_buckets(environment: &str) -> (String, String) {
    (
        format!("mandate-tofu-state-west-{environment}"),
        format!("mandate-tofu-state-{environment}"),
    )
}

/// What the run does with the state, from the `HEAD` statuses of the West state object, its copy
/// marker, and the nyc3 state object.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StateMove {
    /// The West bucket holds the state, and it was either verified when copied (the marker) or
    /// born there (nyc3 holds none); nothing is copied.
    InWest,
    /// Neither holds it: a first run, with nothing to move.
    NothingToMove,
    /// nyc3 holds it and the West bucket holds no verified copy: it is copied, never moved,
    /// replacing any unverified West copy an interrupted run left.
    Copy,
    /// An answer that is none of these, a marker whose state is gone among them (the nyc3 copy may
    /// be older than what the West held): the run stops before `init`, so no plan reads empty,
    /// stale or unverified state.
    Unreadable,
}

pub(crate) fn state_move(west: u16, marker: u16, legacy: u16) -> StateMove {
    match (west, marker, legacy) {
        (200, 200, _) | (200, 404, 404) => StateMove::InWest,
        (200 | 404, 404, 200) => StateMove::Copy,
        (404, 404, 404) => StateMove::NothingToMove,
        _ => StateMove::Unreadable,
    }
}

/// A file in the run's temporary directory, named for this process and numbered per call, so no
/// two in one machine share a name, readable by this user alone, deleted when dropped.
struct TempFile {
    path: PathBuf,
}

/// The next number a [`TempFile`] takes in this process.
static TEMP_FILES: AtomicU64 = AtomicU64::new(0);

impl TempFile {
    fn new(name: &str) -> Result<Self> {
        let dir = std::env::var_os("RUNNER_TEMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!(
            "{name}-{}-{}",
            std::process::id(),
            TEMP_FILES.fetch_add(1, Ordering::Relaxed)
        ));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("creating {}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// The Spaces objects the state move reads and writes, so tests can drive the copy against a fake.
trait Store {
    /// `HEAD` to ask whether `path` in `region` exists, `GET` it into `file`, `PUT` it from `file`,
    /// or `DELETE` it, which only a West copy that failed its check is ever given; the status.
    fn object(&self, method: &str, region: &str, path: &str, file: &Path) -> Result<u16>;
}

/// The real store: SigV4-signed requests with this run's key, each a curl config on standard input.
struct Spaces<'key>(&'key SpacesKey);

impl Store for Spaces<'_> {
    fn object(&self, method: &str, region: &str, path: &str, file: &Path) -> Result<u16> {
        let mut config = spaces_config(region, path, self.0)?;
        let file = config_quoted(&file.to_string_lossy())?;
        match method {
            "HEAD" => config.push_str("head\n"),
            "GET" => config.push_str(&format!("output = {file}\n")),
            "PUT" => config.push_str(&format!("upload-file = {file}\n")),
            _ => config.push_str(&format!("request = {}\n", config_quoted(method)?)),
        }
        Ok(curl(&config, true)?.status)
    }
}

/// The SHA-256 of a file's bytes, refusing an empty one.
fn digest(file: &Path) -> Result<Vec<u8>> {
    let bytes = fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    ensure!(!bytes.is_empty(), "the state object is empty");
    Ok(Sha256::digest(&bytes).to_vec())
}

/// The state object's path in a bucket, and its copy marker's.
fn state_paths(bucket: &str, environment: &str) -> (String, String) {
    let object = format!("{bucket}/{environment}/tofu.tfstate");
    let marker = format!("{object}.verified-copy");
    (object, marker)
}

/// How a copy ended.
#[derive(Debug, PartialEq, Eq)]
enum Copied {
    /// Read back, equal by SHA-256, and marked.
    Verified,
    /// Not verified; `left_behind` when an unverified West copy could not be deleted. It has no
    /// marker, so the next run copies again rather than trusting it.
    Refused { left_behind: bool },
}

/// What the run says about a West copy that failed its check.
fn left_behind_note(left_behind: bool) -> &'static str {
    if left_behind {
        "an unverified copy is left in the West bucket without a marker, so the next run copies \
         the state again"
    } else {
        "no unverified copy is left in the West bucket"
    }
}

/// Copies the nyc3 state into the West bucket and verifies it: `PUT`, read back, compare by
/// SHA-256, then write the marker holding the digest. After a `PUT` that answered 2xx, or whose
/// answer was lost, any error or mismatch deletes the West copy, best effort, and keeps the
/// original error, saying whether the copy was left behind; a `PUT` that was refused wrote
/// nothing, so nothing is deleted.
fn copy_state(store: &dyn Store, environment: &str, read: &Path, back: &Path) -> Result<Copied> {
    let (west, legacy) = state_buckets(environment);
    let (west_path, marker_path) = state_paths(&west, environment);
    let (legacy_path, _) = state_paths(&legacy, environment);
    if store.object("GET", LEGACY_SPACES_REGION, &legacy_path, read)? != 200 {
        return Ok(Copied::Refused { left_behind: false });
    }
    let original = digest(read)?;
    let put = store.object("PUT", SPACES_REGION, &west_path, read);
    let may_have_written = !matches!(put, Ok(status) if !(200..300).contains(&status));
    let verified = match put {
        Ok(status) if (200..300).contains(&status) => (|| -> Result<bool> {
            if store.object("GET", SPACES_REGION, &west_path, back)? != 200 {
                return Ok(false);
            }
            Ok(digest(back)? == original)
        })(),
        Ok(_) => Ok(false),
        Err(error) => Err(error),
    };
    if !matches!(verified, Ok(true)) {
        let left_behind = may_have_written
            && !matches!(
                store.object("DELETE", SPACES_REGION, &west_path, Path::new("/dev/null")),
                Ok(200..=299)
            );
        return match verified {
            Err(error) => Err(error.context(format!(
                "copying the state to {west}: {}",
                left_behind_note(left_behind)
            ))),
            Ok(_) => Ok(Copied::Refused { left_behind }),
        };
    }
    let hex: String = original.iter().map(|byte| format!("{byte:02x}")).collect();
    fs::write(back, hex).context("writing the copy marker")?;
    if (200..300).contains(&store.object("PUT", SPACES_REGION, &marker_path, back)?) {
        Ok(Copied::Verified)
    } else {
        Ok(Copied::Refused { left_behind: true })
    }
}

/// Moves the encrypted state from the nyc3 bucket to the West one (DEC-2047 item 6): byte for byte,
/// never decrypted, verified by SHA-256 and marked before `init`. The nyc3 object is never changed
/// or deleted. Only `plan` and `apply` copy; `preflight` reports.
fn move_state(
    list: &mut Checklist,
    store: &dyn Store,
    environment: &str,
    copy: bool,
) -> Result<()> {
    let (west, legacy) = state_buckets(environment);
    let (west_path, marker_path) = state_paths(&west, environment);
    let (legacy_path, _) = state_paths(&legacy, environment);
    let unused = Path::new("/dev/null");
    let west_status = store.object("HEAD", SPACES_REGION, &west_path, unused)?;
    let marker_status = store.object("HEAD", SPACES_REGION, &marker_path, unused)?;
    let legacy_status = store.object("HEAD", LEGACY_SPACES_REGION, &legacy_path, unused)?;
    match state_move(west_status, marker_status, legacy_status) {
        StateMove::InWest => list.check(true, format!("OpenTofu's state is in {west}")),
        StateMove::NothingToMove => list.check(
            true,
            format!("no earlier state in {legacy} ({LEGACY_SPACES_REGION}) to move"),
        ),
        StateMove::Copy if !copy => list.check(
            true,
            format!(
                "the state in {legacy} ({LEGACY_SPACES_REGION}) is copied to {west} by plan or \
                 apply"
            ),
        ),
        StateMove::Copy => {
            let read = TempFile::new(&format!("mandate-state-{environment}-read"))?;
            let back = TempFile::new(&format!("mandate-state-{environment}-back"))?;
            let what = format!(
                "the state is copied from {legacy} ({LEGACY_SPACES_REGION}) to {west} and \
                 verified by SHA-256; {legacy} is kept as the rollback"
            );
            match copy_state(store, environment, &read.path, &back.path)? {
                Copied::Verified => list.check(true, what),
                Copied::Refused { left_behind } => list.check(
                    false,
                    format!("{what} (it was not: {})", left_behind_note(left_behind)),
                ),
            }
        }
        StateMove::Unreadable => list.check(
            false,
            format!(
                "the state in {west} and {legacy} is readable (statuses {west_status}, \
                 {marker_status} and {legacy_status}); nothing was planned, so no plan reads \
                 unverified state"
            ),
        ),
    }
    Ok(())
}

/// Finds the state bucket with this run's key, or makes it when `make_bucket`, then moves the
/// state into it from nyc3 if it is still there.
fn state_bucket(
    list: &mut Checklist,
    environment: &str,
    key: &SpacesKey,
    make_bucket: bool,
) -> Result<()> {
    let (name, _) = state_buckets(environment);
    match bucket("HEAD", &name, key)? {
        200 => list.check(true, format!("Spaces bucket {name} exists")),
        404 if make_bucket => {
            let made = bucket("PUT", &name, key)?;
            list.check(
                (200..300).contains(&made),
                format!("Spaces bucket {name} is created for this run's state"),
            );
        }
        404 => list.check(
            true,
            format!("Spaces bucket {name} will be created by plan or apply"),
        ),
        _ => list.check(
            false,
            format!("Spaces bucket {name} is reachable (its name may be held by another account)"),
        ),
    }
    move_state(list, &Spaces(key), environment, make_bucket)
}

/// The Cloudflare half: one account, the token's own permission groups, R2 enabled, and Access set
/// up.
fn cloudflare(list: &mut Checklist, token: &str) -> Result<()> {
    #[derive(Deserialize)]
    struct Envelope<T> {
        result: T,
    }
    #[derive(Deserialize)]
    struct Id {
        id: String,
    }
    #[derive(Deserialize)]
    struct Token {
        policies: Vec<Policy>,
    }
    #[derive(Deserialize)]
    struct Policy {
        permission_groups: Vec<Group>,
    }
    #[derive(Deserialize)]
    struct Group {
        name: String,
    }
    let accounts = api(
        "GET",
        &format!("{CF_API}/accounts?per_page=50"),
        token,
        None,
    )?;
    let accounts: Vec<Id> = if accounts.ok() {
        serde_json::from_str::<Envelope<Vec<Id>>>(&accounts.body)
            .context("Cloudflare's account list is not its JSON")?
            .result
    } else {
        Vec::new()
    };
    list.check(
        accounts.len() == 1,
        "CF_INFRA_API_TOKEN sees exactly one Cloudflare account",
    );
    let [account] = accounts.as_slice() else {
        return Ok(());
    };
    let account = &account.id;
    mask::add_mask(account);
    let mut record = None;
    for (verify, read) in [
        (
            format!("{CF_API}/user/tokens/verify"),
            format!("{CF_API}/user/tokens/"),
        ),
        (
            format!("{CF_API}/accounts/{account}/tokens/verify"),
            format!("{CF_API}/accounts/{account}/tokens/"),
        ),
    ] {
        let verified = api("GET", &verify, token, None)?;
        if !verified.ok() {
            continue;
        }
        let id = serde_json::from_str::<Envelope<Id>>(&verified.body)
            .context("Cloudflare's token verification is not its JSON")?
            .result
            .id;
        mask::add_mask(&id);
        record = Some(api("GET", &format!("{read}{id}"), token, None)?);
        break;
    }
    match record {
        Some(answer) if answer.ok() => {
            let own: Token = serde_json::from_str::<Envelope<Token>>(&answer.body)
                .context("Cloudflare's token record is not its JSON")?
                .result;
            let groups: BTreeSet<String> = own
                .policies
                .into_iter()
                .flat_map(|policy| policy.permission_groups)
                .map(|group| group.name)
                .collect();
            let gaps = cloudflare_gaps(&groups);
            for (label, _) in CF_NEEDED {
                list.check(
                    !gaps.contains(label),
                    format!("CF_INFRA_API_TOKEN has permission {label}"),
                );
            }
        }
        _ => list.check(
            false,
            "CF_INFRA_API_TOKEN has permission API Tokens Edit (it cannot read its own record, \
             so its other permissions are unchecked)",
        ),
    }
    let r2 = api(
        "GET",
        &format!("{CF_API}/accounts/{account}/r2/buckets?per_page=1"),
        token,
        None,
    )?;
    list.check(
        r2.ok(),
        "R2 is enabled on the Cloudflare account (a one-time console step)",
    );
    let access = api(
        "GET",
        &format!("{CF_API}/accounts/{account}/access/apps?per_page=1"),
        token,
        None,
    )?;
    let (enabled, line) = access_line(&access);
    list.check(enabled, line);
    Ok(())
}

/// The checklist line for Cloudflare Access, from the account's Access application list, which the
/// token's Access apps permission reads: Cloudflare refuses it with `access.api.error.not_enabled`
/// until the founder sets up a Zero Trust team, the one-time console step staging's first apply
/// failed on (DEC-2044 item 1).
pub(crate) fn access_line(answer: &Answer) -> (bool, String) {
    let what = "Cloudflare Access is enabled";
    if answer.ok() {
        (true, what.to_owned())
    } else if answer.body.contains("access.api.error.not_enabled") {
        (
            false,
            format!("{what} (one-time console step: Zero Trust → set up a team)"),
        )
    } else {
        (
            false,
            format!(
                "{what} (CF_INFRA_API_TOKEN could not read Access applications, status {})",
                answer.status
            ),
        )
    }
}

/// The whole preflight for `environment` (DEC-2030 item 5). The Cloudflare half runs first, so
/// nothing after the mint but the state bucket can fail. When `make_bucket`, as `plan` and `apply`
/// ask, a missing state bucket is created; the key it mints is returned in its guard.
pub(crate) fn preflight(
    environment: &str,
    run: &str,
    make_bucket: bool,
    read: &dyn Fn(&str) -> Option<String>,
) -> Result<Preflight> {
    if !matches!(environment, "staging" | "production") {
        bail!("the environment must be staging or production, not `{environment}`");
    }
    let mut checklist = environment_items(environment, read);
    if let Some(token) = present(read, CF_TOKEN_ENV) {
        cloudflare(&mut checklist, &token)?;
    }
    let mut key = KeyGuard::none();
    if let Some(token) = present(read, DO_TOKEN_ENV) {
        key = digitalocean(&mut checklist, environment, run, &token, make_bucket)?;
    }
    checklist.remind("DigitalOcean billing alert at $120, 80% of the $150 cap (no API reads it)");
    Ok(Preflight { checklist, key })
}

/// `infra close`'s work (DEC-2030 item 4): deletes every `mandate-infra-<environment>-*` Spaces
/// key, this run's included, so a run killed before its own guard ran leaves none behind. Returns
/// how many it deleted; a key DigitalOcean would not delete fails it.
pub(crate) fn close(environment: &str, token: &str) -> Result<usize> {
    if !matches!(environment, "staging" | "production") {
        bail!("the environment must be staging or production, not `{environment}`");
    }
    let Some(keys) = keys_with_prefix(token, &key_prefix(environment))? else {
        bail!(
            "DigitalOcean refused to list Spaces keys (DIGITAL_OCEAN_API_KEY needs Spaces Keys read)"
        );
    };
    for access_key in &keys {
        ensure!(
            delete_key(token, access_key)?,
            "DigitalOcean refused to delete a {} key (Spaces Keys delete)",
            key_prefix(environment)
        );
    }
    Ok(keys.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;

    fn reader(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        }
    }

    fn missing(list: &Checklist) -> Vec<String> {
        list.items
            .iter()
            .filter_map(|item| match item {
                Item::Missing(what) => Some(what.clone()),
                _ => None,
            })
            .collect()
    }

    /// DEC-2030 item 5: each secret is named when unset, blank or short, and nothing prints a value.
    #[test]
    fn the_checklist_names_each_missing_secret_and_no_value() {
        let none = environment_items("staging", &reader(&[]));
        assert_eq!(
            missing(&none),
            [
                "secret TOFU_STATE_PASSPHRASE is set",
                "secret DIGITAL_OCEAN_API_KEY is set",
                "secret CF_INFRA_API_TOKEN is set",
            ]
        );
        let set = reader(&[
            (PASSPHRASE_ENV, "short-passphrase-value"),
            (DO_TOKEN_ENV, "do-token-value"),
            (CF_TOKEN_ENV, "  "),
        ]);
        let partial = environment_items("staging", &set);
        assert_eq!(
            missing(&partial),
            [
                "secret TOFU_STATE_PASSPHRASE is at least 32 characters",
                "secret CF_INFRA_API_TOKEN is set",
            ]
        );
        let printed = partial.render("staging");
        for value in ["short-passphrase-value", "do-token-value"] {
            assert!(!printed.contains(value), "the checklist printed {value}");
        }
        assert!(!partial.complete());
    }

    /// DEC-2030 item 3: staging may leave KUBE_API_ALLOWED_CIDRS empty; production may not.
    #[test]
    fn only_production_needs_the_founders_cidrs() {
        let full = |cidrs: Option<&str>| {
            let mut pairs = vec![
                (PASSPHRASE_ENV, "a-passphrase-of-at-least-thirty-two-chars"),
                (DO_TOKEN_ENV, "t"),
                (CF_TOKEN_ENV, "t"),
                (EMAILS_ENV, "[\"founder@example.com\"]"),
            ];
            if let Some(cidrs) = cidrs {
                pairs.push((KUBE_CIDRS_ENV, cidrs));
            }
            reader(&pairs)
        };
        for cidrs in [None, Some(""), Some("[]"), Some(" [ ] ")] {
            assert!(environment_items("staging", &full(cidrs)).complete());
            assert_eq!(
                missing(&environment_items("production", &full(cidrs))),
                ["secret KUBE_API_ALLOWED_CIDRS lists at least one CIDR (production only)"],
                "{cidrs:?}"
            );
        }
        assert!(environment_items("production", &full(Some("[\"203.0.113.7/32\"]"))).complete());
        let without_emails = reader(&[
            (PASSPHRASE_ENV, "a-passphrase-of-at-least-thirty-two-chars"),
            (DO_TOKEN_ENV, "t"),
            (CF_TOKEN_ENV, "t"),
            (KUBE_CIDRS_ENV, "[\"203.0.113.7/32\"]"),
            (EMAILS_ENV, "[]"),
        ]);
        assert_eq!(
            missing(&environment_items("production", &without_emails)),
            ["secret ACCESS_ALLOWED_EMAILS lists at least one email (production only)"]
        );
        assert!(environment_items("staging", &without_emails).complete());
    }

    /// The review's blocker (DEC-2018 item 7): `mint_key` masks the access key id before the
    /// guard holds it and the secret before it is stored, and goes through `crate::mask`, the one
    /// place a mask command is written. Read from the source, since the mint itself needs
    /// DigitalOcean.
    #[test]
    fn the_minted_key_is_masked_before_anything_holds_it() {
        let source = include_str!("infra_preflight.rs");
        let body = source
            .split("fn mint_key(")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap_or_default();
        let at = |needle: &str| body.find(needle);
        let mask_id = at("mask::add_mask(&access_key);");
        let guard = at("let mut guard = KeyGuard {");
        let mask_secret = at("mask::add_mask(&secret_key);");
        let store = at("key.secret_key = secret_key;");
        assert!(
            mask_id.is_some() && mask_id < guard,
            "the id is masked before the guard holds it"
        );
        assert!(
            guard < mask_secret,
            "the guard holds the key before the secret can fail"
        );
        assert!(
            mask_secret.is_some() && mask_secret < store,
            "the secret is masked before it is stored"
        );
        assert!(
            !body.contains("add-mask"),
            "masks go through crate::mask alone"
        );
    }

    /// DEC-2030 item 4: a guard with no key deletes nothing and releases cleanly, so an early
    /// return before the mint never calls DigitalOcean.
    #[test]
    fn an_empty_key_guard_releases_cleanly() -> Result<()> {
        assert!(KeyGuard::none().key().is_none());
        assert!(KeyGuard::none().release()?);
        drop(KeyGuard::none());
        Ok(())
    }

    /// DEC-2047 item 6: the state is copied only when nyc3 holds it and the West bucket holds no
    /// verified copy; a marker whose state is gone, or any other answer, stops the run before
    /// `init`, so no plan reads empty, stale or unverified state.
    #[test]
    fn the_state_moves_only_from_a_readable_nyc3_copy() {
        assert_eq!(state_move(200, 200, 200), StateMove::InWest);
        assert_eq!(state_move(200, 200, 404), StateMove::InWest);
        assert_eq!(state_move(200, 200, 403), StateMove::InWest);
        assert_eq!(state_move(200, 404, 404), StateMove::InWest);
        assert_eq!(state_move(404, 404, 200), StateMove::Copy);
        assert_eq!(state_move(200, 404, 200), StateMove::Copy);
        assert_eq!(state_move(404, 404, 404), StateMove::NothingToMove);
        for (west, marker, legacy) in [
            (404, 200, 200),
            (404, 200, 404),
            (200, 403, 200),
            (200, 404, 403),
            (403, 404, 200),
            (500, 404, 404),
            (404, 404, 403),
            (301, 404, 200),
        ] {
            assert_eq!(
                state_move(west, marker, legacy),
                StateMove::Unreadable,
                "{west}, {marker} and {legacy} stop the run"
            );
        }
    }

    /// How the fake store misbehaves.
    #[derive(Default)]
    struct Faults {
        read_back_fails: bool,
        read_back_empty: bool,
        put_response_lost: bool,
        put_corrupts: bool,
        put_refused: bool,
        delete_fails: bool,
    }

    /// Spaces in memory, keyed by region and path.
    #[derive(Default)]
    struct FakeStore {
        objects: RefCell<BTreeMap<String, Vec<u8>>>,
        faults: Faults,
        writes: Cell<u32>,
        deletes: Cell<u32>,
    }

    impl FakeStore {
        fn with_legacy(state: &[u8], faults: Faults) -> Self {
            let store = Self {
                faults,
                ..Self::default()
            };
            store.objects.borrow_mut().insert(
                format!("{LEGACY_SPACES_REGION}/mandate-tofu-state-staging/staging/tofu.tfstate"),
                state.to_vec(),
            );
            store
        }

        fn get(&self, path: &str) -> Option<Vec<u8>> {
            self.objects
                .borrow()
                .get(&format!("{SPACES_REGION}/{path}"))
                .cloned()
        }
    }

    impl Store for FakeStore {
        fn object(&self, method: &str, region: &str, path: &str, file: &Path) -> Result<u16> {
            let at = format!("{region}/{path}");
            let west = region == SPACES_REGION && path.ends_with("tofu.tfstate");
            let present = self.objects.borrow().contains_key(&at);
            match method {
                "HEAD" => Ok(if present { 200 } else { 404 }),
                "GET" if west && self.faults.read_back_fails => bail!("curl: operation timed out"),
                "GET" => {
                    let bytes = self.objects.borrow().get(&at).cloned();
                    match bytes {
                        Some(_) if west && self.faults.read_back_empty => {
                            fs::write(file, b"")?;
                            Ok(200)
                        }
                        Some(bytes) => {
                            fs::write(file, bytes)?;
                            Ok(200)
                        }
                        None => Ok(404),
                    }
                }
                "PUT" if west && self.faults.put_refused => Ok(403),
                "PUT" => {
                    self.writes.set(self.writes.get() + 1);
                    let mut bytes = fs::read(file)?;
                    if west && self.faults.put_corrupts {
                        bytes.reverse();
                    }
                    self.objects.borrow_mut().insert(at, bytes);
                    if west && self.faults.put_response_lost {
                        bail!("curl: connection reset after the upload");
                    }
                    Ok(200)
                }
                "DELETE" if self.faults.delete_fails => {
                    self.deletes.set(self.deletes.get() + 1);
                    bail!("curl: could not resolve host")
                }
                "DELETE" => {
                    self.deletes.set(self.deletes.get() + 1);
                    self.objects.borrow_mut().remove(&at);
                    Ok(204)
                }
                _ => bail!("an unexpected request"),
            }
        }
    }

    const STATE: &[u8] = b"encrypted-state-bytes";
    const WEST_STATE: &str = "mandate-tofu-state-west-staging/staging/tofu.tfstate";
    const WEST_MARKER: &str = "mandate-tofu-state-west-staging/staging/tofu.tfstate.verified-copy";

    fn run_move(store: &FakeStore, copy: bool) -> Result<Vec<String>> {
        let mut list = Checklist::default();
        move_state(&mut list, store, "staging", copy)?;
        Ok(missing(&list))
    }

    /// DEC-2047 item 6: a verified copy is the nyc3 bytes, marked with their digest, and the nyc3
    /// object is untouched; the next run trusts it and copies nothing.
    #[test]
    fn a_verified_copy_is_marked_and_trusted() -> Result<()> {
        let store = FakeStore::with_legacy(STATE, Faults::default());
        assert!(run_move(&store, true)?.is_empty());
        assert_eq!(store.get(WEST_STATE).as_deref(), Some(STATE));
        let marker = String::from_utf8(store.get(WEST_MARKER).unwrap_or_default())?;
        let expected: String = Sha256::digest(STATE)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(marker, expected, "the marker holds the verified digest");
        assert_eq!(
            store
                .objects
                .borrow()
                .get(&format!(
                    "{LEGACY_SPACES_REGION}/mandate-tofu-state-staging/staging/tofu.tfstate"
                ))
                .map(Vec::as_slice),
            Some(STATE),
            "the nyc3 object is untouched"
        );
        let writes = store.writes.get();
        assert!(run_move(&store, true)?.is_empty());
        assert_eq!(
            store.writes.get(),
            writes,
            "a marked copy is not copied again"
        );
        Ok(())
    }

    /// A preflight reports the copy and writes nothing.
    #[test]
    fn a_preflight_copies_nothing() -> Result<()> {
        let store = FakeStore::with_legacy(STATE, Faults::default());
        assert!(run_move(&store, false)?.is_empty());
        assert_eq!(store.writes.get(), 0);
        assert!(store.get(WEST_STATE).is_none());
        Ok(())
    }

    /// The review of #1854: once a `PUT` is attempted, a timed-out or empty read-back, a lost `PUT`
    /// answer, or a mismatch deletes the West copy, keeps the original error, and writes no marker.
    #[test]
    fn an_unverified_copy_is_deleted_and_never_marked() -> Result<()> {
        for (name, faults, errs) in [
            (
                "a timed-out read-back",
                Faults {
                    read_back_fails: true,
                    ..Faults::default()
                },
                true,
            ),
            (
                "an empty read-back",
                Faults {
                    read_back_empty: true,
                    ..Faults::default()
                },
                true,
            ),
            (
                "a lost PUT answer",
                Faults {
                    put_response_lost: true,
                    ..Faults::default()
                },
                true,
            ),
            (
                "a mismatch",
                Faults {
                    put_corrupts: true,
                    ..Faults::default()
                },
                false,
            ),
        ] {
            let store = FakeStore::with_legacy(STATE, faults);
            match run_move(&store, true) {
                Err(error) => {
                    assert!(errs, "{name} fails the run with its own error");
                    assert!(
                        format!("{error:#}").contains("no unverified copy is left"),
                        "{name}: {error:#}"
                    );
                }
                Ok(missing) => {
                    assert!(!errs, "{name} fails the run with its own error");
                    assert_eq!(missing.len(), 1, "{name} is a MISSING line");
                    assert!(missing[0].contains("no unverified copy is left"), "{name}");
                }
            }
            assert!(
                store.get(WEST_STATE).is_none(),
                "{name} deletes the West copy"
            );
            assert!(store.get(WEST_MARKER).is_none(), "{name} writes no marker");
        }
        Ok(())
    }

    /// The review of #1854: a `PUT` that was refused wrote nothing, so nothing is deleted, and the
    /// line says no copy is left.
    #[test]
    fn a_refused_put_deletes_nothing() -> Result<()> {
        let store = FakeStore::with_legacy(
            STATE,
            Faults {
                put_refused: true,
                delete_fails: true,
                ..Faults::default()
            },
        );
        let missing = run_move(&store, true)?;
        assert_eq!(missing.len(), 1);
        assert!(
            missing[0].contains("no unverified copy is left"),
            "{}",
            missing[0]
        );
        assert_eq!(
            store.deletes.get(),
            0,
            "nothing was written, so nothing is deleted"
        );
        assert!(store.get(WEST_STATE).is_none() && store.get(WEST_MARKER).is_none());
        Ok(())
    }

    /// A refused `PUT` never leads to a marker, even over an unmarked West copy an earlier run
    /// left, whose bytes this run cannot vouch for.
    #[test]
    fn a_refused_put_never_marks_an_old_copy() -> Result<()> {
        let store = FakeStore::with_legacy(
            STATE,
            Faults {
                put_refused: true,
                ..Faults::default()
            },
        );
        store
            .objects
            .borrow_mut()
            .insert(format!("{SPACES_REGION}/{WEST_STATE}"), STATE.to_vec());
        assert_eq!(run_move(&store, true)?.len(), 1);
        assert!(
            store.get(WEST_MARKER).is_none(),
            "no marker over a copy this run did not write"
        );
        Ok(())
    }

    /// Two temporary files in one process never share a name.
    #[test]
    fn temporary_files_never_share_a_name() -> Result<()> {
        let one = TempFile::new("mandate-name-test")?;
        let two = TempFile::new("mandate-name-test")?;
        assert_ne!(one.path, two.path);
        Ok(())
    }

    /// A copy that could not even be deleted carries no marker, so the next run copies again and
    /// verifies, rather than trusting it.
    #[test]
    fn an_unmarked_copy_is_copied_again() -> Result<()> {
        let store = FakeStore::with_legacy(
            STATE,
            Faults {
                put_corrupts: true,
                delete_fails: true,
                ..Faults::default()
            },
        );
        let missing = run_move(&store, true)?;
        assert_eq!(missing.len(), 1);
        assert!(
            missing[0].contains("an unverified copy is left"),
            "the line says the copy was left behind: {}",
            missing[0]
        );
        assert!(store.get(WEST_STATE).is_some(), "the bad copy survived");
        assert!(store.get(WEST_MARKER).is_none());
        let healed = FakeStore {
            objects: RefCell::new(store.objects.borrow().clone()),
            ..FakeStore::default()
        };
        assert!(run_move(&healed, true)?.is_empty());
        assert_eq!(healed.get(WEST_STATE).as_deref(), Some(STATE));
        assert!(healed.get(WEST_MARKER).is_some());
        Ok(())
    }

    /// The West buckets carry the new names, and the nyc3 ones the old (DEC-2047 item 6).
    #[test]
    fn the_state_buckets_are_named_for_the_west() {
        assert_eq!(
            state_buckets("staging"),
            (
                "mandate-tofu-state-west-staging".to_owned(),
                "mandate-tofu-state-staging".to_owned()
            )
        );
        for root in [
            include_str!("../../deploy/tofu/environments/staging/versions.tf"),
            include_str!("../../deploy/tofu/environments/production/versions.tf"),
        ] {
            assert!(root.contains(&format!("https://{SPACES_REGION}.digitaloceanspaces.com")));
            assert!(root.contains("bucket                      = \"mandate-tofu-state-west-"));
        }
    }

    /// A copy is compared byte for byte; an empty object is never taken as the state.
    #[test]
    fn the_copy_is_compared_by_digest() -> Result<()> {
        let one = TempFile::new(&format!("mandate-digest-test-a-{}", std::process::id()))?;
        let two = TempFile::new(&format!("mandate-digest-test-b-{}", std::process::id()))?;
        fs::write(&one.path, b"ciphertext")?;
        fs::write(&two.path, b"ciphertext")?;
        assert_eq!(digest(&one.path)?, digest(&two.path)?);
        fs::write(&two.path, b"ciphertexu")?;
        assert_ne!(digest(&one.path)?, digest(&two.path)?);
        fs::write(&two.path, b"")?;
        assert!(digest(&two.path).is_err(), "an empty object is refused");
        let path = one.path.clone();
        drop(one);
        assert!(!path.exists(), "the temporary copy is deleted");
        Ok(())
    }

    /// DEC-2044 item 1: Access not yet set up is named as the console step, before any plan.
    #[test]
    fn access_not_enabled_names_the_console_step() {
        let answer = |status, body: &str| Answer {
            status,
            body: body.to_owned(),
        };
        assert_eq!(
            access_line(&answer(200, "{\"result\":[]}")),
            (true, "Cloudflare Access is enabled".to_owned())
        );
        let (enabled, line) = access_line(&answer(
            403,
            "{\"errors\":[{\"code\":9999,\"message\":\"access.api.error.not_enabled: Access is not enabled\"}]}",
        ));
        assert!(!enabled);
        assert!(
            line.contains("one-time console step: Zero Trust → set up a team"),
            "{line}"
        );
        let (enabled, line) = access_line(&answer(403, "{\"errors\":[{\"code\":10000}]}"));
        assert!(!enabled);
        assert!(
            line.contains("could not read Access applications, status 403"),
            "{line}"
        );
    }

    /// DEC-2029 item 4: each needed Cloudflare permission is named when no group grants it, by
    /// either of Cloudflare's spellings.
    #[test]
    fn cloudflare_gaps_name_each_missing_permission() {
        let all: BTreeSet<String> = [
            "Account API Tokens Write",
            "Cloudflare Tunnel Write",
            "Access: Apps and Policies Edit",
            "Workers R2 Storage Write",
            "DNS Write",
            "Zone Read",
        ]
        .map(str::to_owned)
        .into();
        assert!(cloudflare_gaps(&all).is_empty());
        let mut without_tokens = all.clone();
        without_tokens.remove("Account API Tokens Write");
        assert_eq!(cloudflare_gaps(&without_tokens), ["API Tokens Edit"]);
        let reads: BTreeSet<String> = ["API Tokens Read", "DNS Read", "Workers R2 Storage Read"]
            .map(str::to_owned)
            .into();
        assert_eq!(
            cloudflare_gaps(&reads).len(),
            CF_NEEDED.len(),
            "a read never stands for an edit"
        );
    }

    /// DEC-2030 item 4: a run deletes only its own environment's minted keys.
    #[test]
    fn only_this_environments_run_keys_are_deleted() -> Result<()> {
        let listing = r#"{"keys":[
            {"name":"mandate-infra-staging-123","access_key":"A1","grants":[]},
            {"name":"mandate-infra-production-9","access_key":"A2","grants":[]},
            {"name":"founder-laptop","access_key":"A3","grants":[]},
            {"name":"x-mandate-infra-staging-1","access_key":"A4","grants":[]}
        ],"links":{},"meta":{"total":4}}"#;
        assert_eq!(keys_named(listing, &key_prefix("staging"))?, ["A1"]);
        assert_eq!(
            listed_count(listing)?,
            4,
            "a page counts every key, not only the prefix's"
        );
        assert_eq!(listed_count(r#"{"keys":[]}"#)?, 0);
        assert!(listed_count("not json").is_err());
        assert!(keys_named("not json", "p").is_err());
        Ok(())
    }

    /// A credential in curl's config can neither end its option early nor escape its quotes.
    #[test]
    fn config_values_are_quoted_and_cannot_break_a_line() -> Result<()> {
        assert_eq!(config_quoted(r#"a"b\c"#)?, r#""a\"b\\c""#);
        assert!(config_quoted("a\nurl = \"https://elsewhere\"").is_err());
        assert!(config_quoted("a\rb").is_err());
        Ok(())
    }

    /// The checklist marks missing lines and reminders apart from what holds.
    #[test]
    fn the_rendered_checklist_marks_each_line() {
        let mut list = Checklist::default();
        list.check(true, "one");
        list.check(false, "two");
        list.remind("three");
        assert_eq!(
            list.render("staging"),
            "infra preflight (staging):\n  [ok]      one\n  [MISSING] two\n  [console] three\n"
        );
        assert!(!list.complete());
        let mut fine = Checklist::default();
        fine.check(true, "one");
        fine.remind("a console step never fails it");
        assert!(fine.complete());
    }
}
