//! `cargo xtask infra`'s preflight and its per-run Spaces key (E21-56, DEC-2029 item 4,
//! DEC-2030 items 4 and 5). The preflight reads which credentials and permissions a run needs and
//! prints one checklist that names what is missing, never a value. The Spaces key is minted for
//! one run, handed to OpenTofu only through the child's environment, and deleted when the run
//! ends. Every credential reaches `curl` as a config on its standard input, never on its command
//! line.

use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

const DO_API: &str = "https://api.digitalocean.com/v2";
const CF_API: &str = "https://api.cloudflare.com/client/v4";
/// The region of every environment's state bucket (DEC-995 item 2).
const SPACES_REGION: &str = "nyc3";
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
    ("Spaces Keys read", "/spaces/keys?per_page=200"),
];

/// One line of the checklist: what was checked, and whether it holds. A reminder is a step no API
/// can read; it never fails the preflight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    Ready(String),
    Missing(String),
    Reminder(String),
}

/// The checklist the preflight prints, in the order it was checked.
#[derive(Debug, Default)]
pub(crate) struct Checklist {
    items: Vec<Item>,
}

impl Checklist {
    fn check(&mut self, holds: bool, what: impl Into<String>) {
        let what = what.into();
        self.items.push(if holds {
            Item::Ready(what)
        } else {
            Item::Missing(what)
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
        let mut out = format!("infra preflight ({environment}):\n");
        for item in &self.items {
            let (mark, what) = match item {
                Item::Ready(what) => ("[ok]      ", what),
                Item::Missing(what) => ("[MISSING] ", what),
                Item::Reminder(what) => ("[console] ", what),
            };
            out.push_str("  ");
            out.push_str(mark);
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

    /// Deletes the key now; `true` when there was none or DigitalOcean confirmed the deletion.
    pub(crate) fn release(mut self) -> Result<bool> {
        match self.key.take() {
            Some(key) => delete_key(&self.token, &key.access_key),
            None => Ok(true),
        }
    }
}

impl Drop for KeyGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take()
            && !delete_key(&self.token, &key.access_key).unwrap_or(false)
        {
            eprintln!(
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
    value.is_none_or(|value| {
        value
            .chars()
            .all(|c| c.is_whitespace() || c == '[' || c == ']')
    })
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
            "variable KUBE_API_ALLOWED_CIDRS lists at least one CIDR (production only)",
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
struct Answer {
    status: u16,
    body: String,
}

impl Answer {
    fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Runs `curl` with `config` on its standard input, so no credential is on a command line, and
/// returns the status and body. Only a request that is safe to repeat is retried.
fn curl(config: &str, retry: bool) -> Result<Answer> {
    let mut child = Command::new("curl")
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

/// A request to a token-authorised JSON API. A `POST` is sent once, never retried, so a lost
/// answer never mints a second key.
fn api(method: &str, url: &str, token: &str, body: Option<&str>) -> Result<Answer> {
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

/// A SigV4-signed request for the state bucket itself: `HEAD` to ask whether it exists, `PUT` to
/// make it.
fn bucket(method: &str, name: &str, key: &SpacesKey) -> Result<u16> {
    let mut config = format!(
        "url = {}\naws-sigv4 = {}\nuser = {}\n",
        config_quoted(&format!(
            "https://{SPACES_REGION}.digitaloceanspaces.com/{name}"
        ))?,
        config_quoted(&format!("aws:amz:{SPACES_REGION}:s3"))?,
        config_quoted(&format!("{}:{}", key.access_key, key.secret_key))?,
    );
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

/// Mints this run's key, full access, since only that grant may create a bucket (DEC-2030 item 4).
fn mint_key(token: &str, name: &str) -> Result<Option<SpacesKey>> {
    #[derive(Deserialize)]
    struct Minted {
        key: MintedKey,
    }
    #[derive(Deserialize)]
    struct MintedKey {
        access_key: String,
        secret_key: String,
    }
    let body = serde_json::json!({
        "name": name,
        "grants": [{ "bucket": "", "permission": "fullaccess" }],
    })
    .to_string();
    let answer = api("POST", &format!("{DO_API}/spaces/keys"), token, Some(&body))?;
    if !answer.ok() {
        return Ok(None);
    }
    let minted: Minted =
        serde_json::from_str(&answer.body).context("DigitalOcean's minted key is not its JSON")?;
    Ok(Some(SpacesKey {
        access_key: minted.key.access_key,
        secret_key: minted.key.secret_key,
    }))
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
    let mut listing = None;
    for (scope, path) in DO_READ_PROBES {
        let answer = api("GET", &format!("{DO_API}{path}"), token, None)?;
        list.check(
            answer.ok(),
            format!("DIGITAL_OCEAN_API_KEY has scope {scope}"),
        );
        if *scope == "Spaces Keys read" && answer.ok() {
            listing = Some(answer.body);
        }
    }
    let prefix = key_prefix(environment);
    if let Some(listing) = listing {
        let stale = keys_named(&listing, &prefix)?;
        if !stale.is_empty() {
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
    }
    let guard = KeyGuard {
        token: token.to_owned(),
        key: mint_key(token, &format!("{prefix}{run}"))?,
    };
    list.check(
        guard.key().is_some(),
        "DIGITAL_OCEAN_API_KEY has scope Spaces Keys create",
    );
    if let Some(key) = guard.key() {
        state_bucket(list, environment, key, make_bucket)?;
    }
    Ok(guard)
}

/// Finds the state bucket with this run's key, or makes it when `make_bucket`.
fn state_bucket(
    list: &mut Checklist,
    environment: &str,
    key: &SpacesKey,
    make_bucket: bool,
) -> Result<()> {
    let name = format!("mandate-tofu-state-{environment}");
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
    Ok(())
}

/// The Cloudflare half: one account, the token's own permission groups, and R2 enabled.
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
    Ok(())
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
    let listing = api(
        "GET",
        &format!("{DO_API}/spaces/keys?per_page=200"),
        token,
        None,
    )?;
    ensure!(
        listing.ok(),
        "DigitalOcean refused to list Spaces keys (DIGITAL_OCEAN_API_KEY needs Spaces Keys read)"
    );
    let keys = keys_named(&listing.body, &key_prefix(environment))?;
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
                ["variable KUBE_API_ALLOWED_CIDRS lists at least one CIDR (production only)"],
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

    /// DEC-2030 item 4: a guard with no key deletes nothing and releases cleanly, so an early
    /// return before the mint never calls DigitalOcean.
    #[test]
    fn an_empty_key_guard_releases_cleanly() -> Result<()> {
        assert!(KeyGuard::none().key().is_none());
        assert!(KeyGuard::none().release()?);
        drop(KeyGuard::none());
        Ok(())
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
