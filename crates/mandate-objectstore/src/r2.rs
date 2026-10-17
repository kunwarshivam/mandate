//! Cloudflare R2's bucket lock rules, read through Cloudflare's API and judged against §6.2.
//!
//! R2 configures locks only through `GET`/`PUT /accounts/{account_id}/r2/buckets/{bucket}/lock`,
//! not the S3 API (DEC-973 item 5), so the check reads them there with a read token and hands them
//! to [`crate::lock::judge_r2`], which refuses them: they are removable by any token that can edit
//! the bucket's configuration.

use std::time::Duration;

use reqwest::Url;
use secrecy::{ExposeSecret, SecretString};

use crate::check::CheckError;
use crate::lock::{judge_r2, parse_r2_rules};
use crate::s3::{ConfigError, S3Error, is_loopback};

const TIMEOUT: Duration = Duration::from_secs(30);

/// Cloudflare's API base.
pub const CLOUDFLARE_API: &str = "https://api.cloudflare.com/client/v4";

/// Where to read one bucket's lock rules, and the token to read them with.
#[derive(Clone)]
pub struct R2LockApi {
    http: reqwest::Client,
    url: Url,
    token: SecretString,
}

impl std::fmt::Debug for R2LockApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("R2LockApi")
            .field("url", &self.url.as_str())
            .finish_non_exhaustive()
    }
}

impl R2LockApi {
    /// The lock-rules URL of `bucket` in `account_id` under `api_base` ([`CLOUDFLARE_API`], or a
    /// loopback fake in tests), read with `token`.
    pub fn new(
        api_base: &str,
        account_id: &str,
        bucket: &str,
        token: SecretString,
    ) -> Result<Self, ConfigError> {
        let base = Url::parse(api_base).map_err(|_| ConfigError::Url)?;
        let host = base.host_str().ok_or(ConfigError::Url)?;
        match base.scheme() {
            "https" => {}
            "http" if is_loopback(host) => {}
            "http" => return Err(ConfigError::PlainHttp),
            _ => return Err(ConfigError::Url),
        }
        let account_ok =
            !account_id.is_empty() && account_id.bytes().all(|b| b.is_ascii_alphanumeric());
        let bucket_ok = (3..=63).contains(&bucket.len())
            && bucket
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !account_ok || !bucket_ok {
            return Err(ConfigError::Name);
        }
        let url = Url::parse(&format!(
            "{}/accounts/{account_id}/r2/buckets/{bucket}/lock",
            api_base.trim_end_matches('/')
        ))
        .map_err(|_| ConfigError::Url)?;
        let _already_installed = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(TIMEOUT)
            .build()
            .map_err(|_| ConfigError::Client)?;
        Ok(Self { http, url, token })
    }

    /// The URL the rules are read from.
    pub fn url(&self) -> &str {
        self.url.as_str()
    }
}

/// Reads the bucket's lock rules and judges them for objects under `prefix`. It issues no
/// [`crate::check::LockCertificate`]: only [`crate::check::check_s3`] does, and that check refuses
/// R2 as well, whose S3 API has no Object Lock. This one reports why R2's own rules fall short, for
/// the run against R2 that confirms DEC-973's finding.
pub async fn check_r2(api: &R2LockApi, prefix: &str) -> Result<(), CheckError> {
    let response = api
        .http
        .get(api.url.clone())
        .bearer_auth(api.token.expose_secret())
        .send()
        .await
        .map_err(|_| S3Error::Transport)?;
    let status = response.status();
    let body = response.bytes().await.map_err(|_| S3Error::Transport)?;
    if status.is_client_error() {
        return Err(S3Error::Rejected {
            call: "GetBucketLock",
            status: status.as_u16(),
            code: String::new(),
        }
        .into());
    }
    if !status.is_success() {
        return Err(S3Error::Server {
            status: status.as_u16(),
        }
        .into());
    }
    let rules = parse_r2_rules(&body).map_err(|_| S3Error::Malformed {
        call: "GetBucketLock",
    })?;
    judge_r2(&rules, prefix)?;
    Ok(())
}
