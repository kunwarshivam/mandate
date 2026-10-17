//! Cloudflare R2's bucket lock rules, read through Cloudflare's API and judged against §6.2.
//!
//! R2 configures locks only through `GET`/`PUT /accounts/{account_id}/r2/buckets/{bucket}/lock`,
//! not the S3 API (DEC-973 item 5), so the check reads them there with a read token and hands them
//! to [`crate::lock::judge_r2`], which refuses them: they are removable by any token that can edit
//! the bucket's configuration.

use reqwest::Url;
use secrecy::SecretString;

use crate::check::{CheckError, LockCertificate};
use crate::s3::{ConfigError, S3Error};

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
        let _ = (api_base, account_id, bucket, token);
        Err(ConfigError::Client)
    }

    /// The URL the rules are read from.
    pub fn url(&self) -> &str {
        self.url.as_str()
    }
}

/// Reads the bucket's lock rules and judges them for objects under `prefix`.
pub async fn check_r2(api: &R2LockApi, prefix: &str) -> Result<LockCertificate, CheckError> {
    let _ = (api, prefix, &api.http, &api.token, S3Error::Transport);
    Err(CheckError::Unimplemented { story: "E21-40" })
}
