//! AWS Signature Version 4 for the S3 API, header-signed, over the request exactly as it is sent.
//!
//! Every S3-compatible provider the plan names (the local test store, Cloudflare R2, Backblaze B2)
//! authenticates this way, so one signer serves all of them and a provider swap changes only the
//! endpoint. The caller passes every header it will send, `host`, `x-amz-date` and
//! `x-amz-content-sha256` among them, and all of them are signed.

use std::fmt::Write as _;
use std::path::Path;
use std::{fs, io};

use jiff::Timestamp;
use ring::{digest, hmac};
use secrecy::{ExposeSecret, SecretString};

/// An access key: its public identifier, its secret, and the session token of a temporary key.
#[derive(Clone)]
pub struct Credentials {
    access_key_id: String,
    secret_access_key: SecretString,
    session_token: Option<SecretString>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials { .. }")
    }
}

impl Credentials {
    /// A key from its parts.
    pub fn new(
        access_key_id: String,
        secret_access_key: SecretString,
        session_token: Option<SecretString>,
    ) -> Self {
        Self {
            access_key_id,
            secret_access_key,
            session_token,
        }
    }

    /// A long-lived key from two files, as the cell mounts secrets (DEC-822 item 4): the access
    /// key identifier and the secret, each trimmed of surrounding whitespace.
    pub fn from_files(access_key_id: &Path, secret_access_key: &Path) -> io::Result<Self> {
        let id = fs::read_to_string(access_key_id)?;
        let secret = fs::read_to_string(secret_access_key)?;
        Ok(Self::new(
            id.trim().to_owned(),
            SecretString::from(secret.trim()),
            None,
        ))
    }

    /// The session token a temporary key sends as `x-amz-security-token`.
    pub fn session_token(&self) -> Option<&str> {
        self.session_token
            .as_ref()
            .map(|token| token.expose_secret())
    }
}

/// The request to sign, as it will be sent.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// The HTTP method in upper case.
    pub method: &'a str,
    /// The path, already URI-encoded as it goes on the wire.
    pub path: &'a str,
    /// The query parameters, not encoded; a parameter with no value has an empty one.
    pub query: &'a [(&'a str, &'a str)],
    /// Every header sent, `host` included.
    pub headers: &'a [(&'a str, &'a str)],
    /// The hex SHA-256 of the body, the `x-amz-content-sha256` value.
    pub payload_sha256: &'a str,
}

/// The credential scope's region and service, and the signing time.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    /// The region, `us-east-1` for most S3-compatible stores and `auto` for R2.
    pub region: &'a str,
    /// `s3`, or `sts` for the security token service.
    pub service: &'a str,
    /// The time the request is signed; also the `x-amz-date` header.
    pub at: Timestamp,
}

/// The `Authorization` header value for `request`.
pub fn authorization(
    credentials: &Credentials,
    request: &Request<'_>,
    scope: &Scope<'_>,
) -> String {
    let mut headers: Vec<(String, String)> = request
        .headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), collapse(value)))
        .collect();
    headers.sort();
    let signed_headers = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let mut query: Vec<(String, String)> = request
        .query
        .iter()
        .map(|(name, value)| (uri_encode(name, false), uri_encode(value, false)))
        .collect();
    query.sort();
    let canonical_query = query
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let canonical_headers: String = headers
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect();
    let canonical_request = format!(
        "{}\n{}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{}",
        request.method, request.path, request.payload_sha256
    );
    let date_time = amz_date(scope.at);
    let date = date_time.get(..8).unwrap_or_default();
    let credential_scope = format!("{date}/{}/{}/aws4_request", scope.region, scope.service);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{date_time}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let secret = format!("AWS4{}", credentials.secret_access_key.expose_secret());
    let key = [date, scope.region, scope.service, "aws4_request"]
        .iter()
        .fold(secret.into_bytes(), |key, part| sign(&key, part.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={}",
        credentials.access_key_id,
        hex(&sign(&key, string_to_sign.as_bytes()))
    )
}

/// `20130524T000000Z`: the `x-amz-date` form of a time.
pub fn amz_date(at: Timestamp) -> String {
    at.strftime("%Y%m%dT%H%M%SZ").to_string()
}

/// The lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, bytes).as_ref())
}

/// RFC 3986 percent-encoding as SigV4 defines it: unreserved characters stay, everything else is
/// `%XX` in upper case, and `/` stays only when `keep_slash`.
pub fn uri_encode(text: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if unreserved || (keep_slash && byte == b'/') {
            out.push(char::from(byte));
        } else {
            let _infallible = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn sign(key: &[u8], message: &[u8]) -> Vec<u8> {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), message)
        .as_ref()
        .to_vec()
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _infallible = write!(out, "{byte:02x}");
    }
    out
}

/// A header value trimmed, with each run of spaces inside it made one space.
fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
