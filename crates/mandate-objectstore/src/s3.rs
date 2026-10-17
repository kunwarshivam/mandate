//! A plain S3 endpoint: path-style requests to one bucket, signed with [`crate::sigv4`], for the
//! few calls the lock check and the locked writer need.
//!
//! The endpoint is configuration ([`Endpoint::new`]): HTTPS, or plain HTTP only to a loopback
//! address, where the local test store runs. No redirect is followed, so a request reaches only the
//! configured host. A response is one of three things: done (2xx), [`Outcome::Rejected`] (4xx, or
//! 501 for an operation the provider does not implement, which is how R2 answers the Object Lock
//! calls), or an [`S3Error`] (any other status, a transport failure, a body that cannot be read).
//! The lock check counts only a rejection as the store refusing; an error is not evidence, so the
//! check fails closed on it.

use std::net::IpAddr;
use std::time::Duration;

use base64::Engine as _;
use jiff::Timestamp;
use reqwest::{Method, StatusCode, Url};
use ring::digest;

use crate::lock::{BucketLock, LockMode, RetentionPeriod};
use crate::sigv4::{self, Credentials, Request, Scope};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const USER_AGENT: &str = "mandate-objectstore";

/// Why an endpoint's configuration was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The URL does not parse, has a path, query or credentials, or is not HTTP(S).
    #[error("the object store URL must be a bare http(s)://host[:port]")]
    Url,
    /// Plain HTTP to anything but a loopback address.
    #[error("the object store must be reached over HTTPS unless it is on a loopback address")]
    PlainHttp,
    /// An empty or malformed bucket or region.
    #[error("the bucket or region is not valid")]
    Name,
    /// The HTTP client could not be built.
    #[error("the HTTP client could not be built")]
    Client,
}

/// Where the store is: its base URL, the region to sign for, and the bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    base: Url,
    host: String,
    region: String,
    bucket: String,
}

impl Endpoint {
    /// Checks and keeps an endpoint. `url` is `https://host[:port]`, or `http://` to a loopback
    /// address; `bucket` is a DNS-style bucket name, used path-style.
    pub fn new(url: &str, region: &str, bucket: &str) -> Result<Self, ConfigError> {
        let base = Url::parse(url).map_err(|_| ConfigError::Url)?;
        let bare = base.path() == "/"
            && base.query().is_none()
            && base.fragment().is_none()
            && base.username().is_empty()
            && base.password().is_none();
        if !bare {
            return Err(ConfigError::Url);
        }
        let host_name = base.host_str().ok_or(ConfigError::Url)?;
        match base.scheme() {
            "https" => {}
            "http" if is_loopback(host_name) => {}
            "http" => return Err(ConfigError::PlainHttp),
            _ => return Err(ConfigError::Url),
        }
        let host = match base.port() {
            Some(port) => format!("{host_name}:{port}"),
            None => host_name.to_owned(),
        };
        let bucket_ok = (3..=63).contains(&bucket.len())
            && bucket
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
        let region_ok = !region.is_empty()
            && region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !bucket_ok || !region_ok {
            return Err(ConfigError::Name);
        }
        Ok(Self {
            base,
            host,
            region: region.to_owned(),
            bucket: bucket.to_owned(),
        })
    }

    /// The bucket's name.
    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// The base URL, without the bucket.
    pub fn base(&self) -> &str {
        self.base.as_str()
    }
}

/// Whether `key` is a plain relative object key: 1 to 1024 bytes, no control character, and
/// `/`-separated segments that are neither empty, `.` nor `..`. A prefix is checked as a key
/// with its trailing `/` removed.
pub fn is_plain_key(key: &str) -> bool {
    (1..=1024).contains(&key.len())
        && !key.chars().any(char::is_control)
        && key
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

pub(crate) fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// A request the store declined: a 4xx, or 501 for an operation it does not implement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// The HTTP status.
    pub status: u16,
    /// The S3 error code from the body, empty when there is none.
    pub code: String,
}

/// What a call that changes something came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The store did it.
    Done,
    /// The store declined.
    Rejected(Rejected),
}

/// A call that gave no answer the lock check may use as evidence.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum S3Error {
    /// The request could not be sent or its response read.
    #[error("the object store could not be reached")]
    Transport,
    /// A status other than 2xx, 4xx or 501.
    #[error("the object store answered {status}")]
    Server {
        /// The HTTP status.
        status: u16,
    },
    /// A 2xx whose body or headers are not what the call expects.
    #[error("the object store's answer to {call} could not be read")]
    Malformed {
        /// The call.
        call: &'static str,
    },
    /// An object key or prefix that is empty, absolute, holds an empty, `.` or `..` segment or a
    /// control character, or is longer than 1024 bytes: refused before anything is sent, so no
    /// key can resolve outside its prefix or bucket.
    #[error("the object key is not a plain relative key")]
    InvalidKey,
    /// The store declined a read the caller needs.
    #[error("the object store declined {call} with {status} {code}")]
    Rejected {
        /// The call.
        call: &'static str,
        /// The HTTP status.
        status: u16,
        /// The S3 error code.
        code: String,
    },
}

/// One object version's metadata, from `HeadObject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectHead {
    /// The version's identifier; `None` on an unversioned bucket.
    pub version_id: Option<String>,
    /// When the version was created, to the second.
    pub last_modified: Timestamp,
    /// The version's retention, when it has one.
    pub retention: Option<(LockMode, Timestamp)>,
}

/// The bucket's lock configuration as read, with the XML it came in, which the probe writes back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockConfiguration {
    /// `ObjectLockEnabled` is `Enabled`.
    pub enabled: bool,
    /// The default rule, when there is one.
    pub default_rule: Option<(LockMode, RetentionPeriod)>,
    /// The body as the store returned it.
    pub xml: String,
}

/// A signed client for one bucket.
#[derive(Debug, Clone)]
pub struct S3 {
    http: reqwest::Client,
    endpoint: Endpoint,
    credentials: Credentials,
    clock: fn() -> Timestamp,
}

struct Answer {
    status: StatusCode,
    headers: reqwest::header::HeaderMap,
    body: Vec<u8>,
}

impl S3 {
    /// A client for `endpoint` with `credentials`, signing at the times `clock` gives.
    pub fn new(
        endpoint: Endpoint,
        credentials: Credentials,
        clock: fn() -> Timestamp,
    ) -> Result<Self, ConfigError> {
        let _already_installed = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(|_| ConfigError::Client)?;
        Ok(Self {
            http,
            endpoint,
            credentials,
            clock,
        })
    }

    /// The endpoint this client signs for.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The same endpoint with other credentials.
    pub fn with_credentials(&self, credentials: Credentials) -> Self {
        Self {
            credentials,
            ..self.clone()
        }
    }

    /// The current time by this client's clock.
    pub fn now(&self) -> Timestamp {
        (self.clock)()
    }

    /// `CreateBucket`, with object lock (and so versioning) when `object_lock`.
    pub async fn create_bucket(&self, object_lock: bool) -> Result<Outcome, S3Error> {
        let lock = [("x-amz-bucket-object-lock-enabled", "true")];
        let headers: &[(&str, &str)] = if object_lock { &lock } else { &[] };
        let answer = self.send(Method::PUT, None, &[], headers, &[]).await?;
        outcome(answer)
    }

    /// `PutObject` of `body` at `key`, with compliance or governance retention until a time when
    /// `retention` is given. The new version's identifier on success.
    pub async fn put_object(
        &self,
        key: &str,
        body: &[u8],
        retention: Option<(LockMode, Timestamp)>,
    ) -> Result<Result<Option<String>, Rejected>, S3Error> {
        let until;
        let mut headers: Vec<(&str, &str)> = Vec::new();
        if let Some((mode, at)) = retention {
            until = rfc3339(at);
            headers.push(("x-amz-object-lock-mode", mode_name(mode)));
            headers.push(("x-amz-object-lock-retain-until-date", &until));
        }
        let answer = self
            .send(Method::PUT, Some(key), &[], &headers, body)
            .await?;
        match outcome_of(answer)? {
            Ok(answer) => Ok(Ok(header(&answer.headers, "x-amz-version-id"))),
            Err(rejected) => Ok(Err(rejected)),
        }
    }

    /// `HeadObject` of `key`, of one version when `version_id` is given; `None` when there is none.
    pub async fn head_object(
        &self,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<Option<ObjectHead>, S3Error> {
        let query: Vec<(&str, &str)> = version_id.map(|v| ("versionId", v)).into_iter().collect();
        let answer = self.send(Method::HEAD, Some(key), &query, &[], &[]).await?;
        if answer.status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let answer = read_ok(answer, "HeadObject")?;
        let malformed = S3Error::Malformed { call: "HeadObject" };
        let last_modified = header(&answer.headers, "last-modified")
            .and_then(|text| jiff::fmt::rfc2822::parse(&text).ok())
            .map(|zoned| zoned.timestamp())
            .ok_or(malformed.clone())?;
        let retention = match (
            header(&answer.headers, "x-amz-object-lock-mode"),
            header(&answer.headers, "x-amz-object-lock-retain-until-date"),
        ) {
            (Some(mode), Some(until)) => Some((
                parse_mode(&mode).ok_or(malformed.clone())?,
                until.parse::<Timestamp>().map_err(|_| malformed.clone())?,
            )),
            _ => None,
        };
        Ok(Some(ObjectHead {
            version_id: header(&answer.headers, "x-amz-version-id"),
            last_modified,
            retention,
        }))
    }

    /// `GetObject` of `key`, of one version when `version_id` is given; `None` when there is none.
    pub async fn get_object(
        &self,
        key: &str,
        version_id: Option<&str>,
    ) -> Result<Option<Vec<u8>>, S3Error> {
        let query: Vec<(&str, &str)> = version_id.map(|v| ("versionId", v)).into_iter().collect();
        let answer = self.send(Method::GET, Some(key), &query, &[], &[]).await?;
        if answer.status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(read_ok(answer, "GetObject")?.body))
    }

    /// `GetObjectRetention` of one version; `None` when it has no retention.
    pub async fn get_object_retention(
        &self,
        key: &str,
        version_id: &str,
    ) -> Result<Option<(LockMode, Timestamp)>, S3Error> {
        let query = [("retention", ""), ("versionId", version_id)];
        let answer = self.send(Method::GET, Some(key), &query, &[], &[]).await?;
        if rejection(&answer).is_some() {
            return Ok(None);
        }
        let answer = read_ok(answer, "GetObjectRetention")?;
        let malformed = S3Error::Malformed {
            call: "GetObjectRetention",
        };
        let text = String::from_utf8(answer.body).map_err(|_| malformed.clone())?;
        let mode = element(&text, "Mode")
            .and_then(parse_mode)
            .ok_or(malformed.clone())?;
        let until = element(&text, "RetainUntilDate")
            .and_then(|until| until.parse::<Timestamp>().ok())
            .ok_or(malformed)?;
        Ok(Some((mode, until)))
    }

    /// `PutObjectRetention` of one version.
    pub async fn put_object_retention(
        &self,
        key: &str,
        version_id: &str,
        mode: LockMode,
        until: Timestamp,
    ) -> Result<Outcome, S3Error> {
        let body = format!(
            "<Retention xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Mode>{}</Mode>\
             <RetainUntilDate>{}</RetainUntilDate></Retention>",
            mode_name(mode),
            rfc3339(until)
        );
        let query = [("retention", ""), ("versionId", version_id)];
        let answer = self
            .send(Method::PUT, Some(key), &query, &[], body.as_bytes())
            .await?;
        outcome(answer)
    }

    /// `DeleteObject` of one version, asking to bypass governance retention when
    /// `bypass_governance`.
    pub async fn delete_object_version(
        &self,
        key: &str,
        version_id: &str,
        bypass_governance: bool,
    ) -> Result<Outcome, S3Error> {
        let bypass = [("x-amz-bypass-governance-retention", "true")];
        let headers: &[(&str, &str)] = if bypass_governance { &bypass } else { &[] };
        let query = [("versionId", version_id)];
        let answer = self
            .send(Method::DELETE, Some(key), &query, headers, &[])
            .await?;
        outcome(answer)
    }

    /// `DeleteObject` of `key` with no version: on a versioned bucket it adds a delete marker and
    /// removes nothing; on an unversioned one, such as R2's, it removes the object unless a lock
    /// holds it.
    pub async fn delete_object(&self, key: &str) -> Result<Outcome, S3Error> {
        let answer = self.send(Method::DELETE, Some(key), &[], &[], &[]).await?;
        outcome(answer)
    }

    /// `GetBucketVersioning`: whether versioning is `Enabled`. A provider that does not implement
    /// the call has none.
    pub async fn bucket_versioning(&self) -> Result<bool, S3Error> {
        let answer = self
            .send(Method::GET, None, &[("versioning", "")], &[], &[])
            .await?;
        if rejection(&answer).is_some() {
            return Ok(false);
        }
        let answer = read_ok(answer, "GetBucketVersioning")?;
        let text = String::from_utf8(answer.body).map_err(|_| S3Error::Malformed {
            call: "GetBucketVersioning",
        })?;
        Ok(element(&text, "Status") == Some("Enabled"))
    }

    /// `GetObjectLockConfiguration`; `None` when the bucket has none or the provider does not
    /// implement the call.
    pub async fn lock_configuration(&self) -> Result<Option<LockConfiguration>, S3Error> {
        let answer = self
            .send(Method::GET, None, &[("object-lock", "")], &[], &[])
            .await?;
        if rejection(&answer).is_some() {
            return Ok(None);
        }
        let answer = read_ok(answer, "GetObjectLockConfiguration")?;
        let malformed = S3Error::Malformed {
            call: "GetObjectLockConfiguration",
        };
        let xml = String::from_utf8(answer.body).map_err(|_| malformed.clone())?;
        let default_rule = match element(&xml, "DefaultRetention") {
            None => None,
            Some(rule) => {
                let mode = element(rule, "Mode")
                    .and_then(parse_mode)
                    .ok_or(malformed.clone())?;
                let period = match (element(rule, "Days"), element(rule, "Years")) {
                    (Some(days), None) => {
                        RetentionPeriod::Days(days.parse().map_err(|_| malformed.clone())?)
                    }
                    (None, Some(years)) => {
                        RetentionPeriod::Years(years.parse().map_err(|_| malformed.clone())?)
                    }
                    _ => return Err(malformed),
                };
                Some((mode, period))
            }
        };
        Ok(Some(LockConfiguration {
            enabled: element(&xml, "ObjectLockEnabled") == Some("Enabled"),
            default_rule,
            xml,
        }))
    }

    /// `PutObjectLockConfiguration` with `xml` as the body.
    pub async fn put_lock_configuration(&self, xml: &str) -> Result<Outcome, S3Error> {
        let answer = self
            .send(
                Method::PUT,
                None,
                &[("object-lock", "")],
                &[],
                xml.as_bytes(),
            )
            .await?;
        outcome(answer)
    }

    /// The bucket's settings as [`BucketLock`] evidence.
    pub async fn bucket_lock(&self) -> Result<(BucketLock, Option<LockConfiguration>), S3Error> {
        let versioning_enabled = self.bucket_versioning().await?;
        let configuration = self.lock_configuration().await?;
        let lock = BucketLock {
            versioning_enabled,
            object_lock_enabled: configuration.as_ref().is_some_and(|c| c.enabled),
            default_rule: configuration.as_ref().and_then(|c| c.default_rule),
        };
        Ok((lock, configuration))
    }

    async fn send(
        &self,
        method: Method,
        key: Option<&str>,
        query: &[(&str, &str)],
        extra_headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<Answer, S3Error> {
        if key.is_some_and(|key| !is_plain_key(key)) {
            return Err(S3Error::InvalidKey);
        }
        let path = match key {
            Some(key) => format!("/{}/{}", self.endpoint.bucket, sigv4::uri_encode(key, true)),
            None => format!("/{}", self.endpoint.bucket),
        };
        let mut url = self.endpoint.base.clone();
        url.set_path(&path);
        let encoded_query: Vec<String> = query
            .iter()
            .map(|(name, value)| {
                format!(
                    "{}={}",
                    sigv4::uri_encode(name, false),
                    sigv4::uri_encode(value, false)
                )
            })
            .collect();
        if !encoded_query.is_empty() {
            url.set_query(Some(&encoded_query.join("&")));
        }
        let at = self.now();
        let date = sigv4::amz_date(at);
        let payload = sigv4::sha256_hex(body);
        let checksum = base64::engine::general_purpose::STANDARD
            .encode(digest::digest(&digest::SHA256, body).as_ref());
        let mut headers: Vec<(&str, &str)> = vec![
            ("host", &self.endpoint.host),
            ("x-amz-date", &date),
            ("x-amz-content-sha256", &payload),
            ("x-amz-checksum-sha256", &checksum),
        ];
        if let Some(token) = self.credentials.session_token() {
            headers.push(("x-amz-security-token", token));
        }
        headers.extend_from_slice(extra_headers);
        let authorization = sigv4::authorization(
            &self.credentials,
            &Request {
                method: method.as_str(),
                path: url.path(),
                query,
                headers: &headers,
                payload_sha256: &payload,
            },
            &Scope {
                region: &self.endpoint.region,
                service: "s3",
                at,
            },
        );
        let mut request = self.http.request(method, url);
        for (name, value) in headers.iter().filter(|(name, _)| *name != "host") {
            request = request.header(*name, *value);
        }
        let response = request
            .header("authorization", authorization)
            .body(body.to_vec())
            .send()
            .await
            .map_err(|_| S3Error::Transport)?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .bytes()
            .await
            .map_err(|_| S3Error::Transport)?
            .to_vec();
        Ok(Answer {
            status,
            headers,
            body,
        })
    }
}

fn outcome(answer: Answer) -> Result<Outcome, S3Error> {
    Ok(match outcome_of(answer)? {
        Ok(_) => Outcome::Done,
        Err(rejected) => Outcome::Rejected(rejected),
    })
}

fn outcome_of(answer: Answer) -> Result<Result<Answer, Rejected>, S3Error> {
    if answer.status.is_success() {
        return Ok(Ok(answer));
    }
    match rejection(&answer) {
        Some(rejected) => Ok(Err(rejected)),
        None => Err(S3Error::Server {
            status: answer.status.as_u16(),
        }),
    }
}

fn read_ok(answer: Answer, call: &'static str) -> Result<Answer, S3Error> {
    match outcome_of(answer)? {
        Ok(answer) => Ok(answer),
        Err(Rejected { status, code }) => Err(S3Error::Rejected { call, status, code }),
    }
}

fn rejection(answer: &Answer) -> Option<Rejected> {
    let status = answer.status;
    (status.is_client_error() || status == StatusCode::NOT_IMPLEMENTED).then(|| Rejected {
        status: status.as_u16(),
        code: std::str::from_utf8(&answer.body)
            .ok()
            .and_then(|text| element(text, "Code"))
            .unwrap_or_default()
            .to_owned(),
    })
}

fn header(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// The text of the first `<name>` element, for the flat responses S3 returns here.
fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)?.checked_add(open.len())?;
    let rest = xml.get(start..)?;
    let end = rest.find(&close)?;
    rest.get(..end)
}

fn mode_name(mode: LockMode) -> &'static str {
    match mode {
        LockMode::Compliance => "COMPLIANCE",
        LockMode::Governance => "GOVERNANCE",
    }
}

fn parse_mode(text: &str) -> Option<LockMode> {
    match text {
        "COMPLIANCE" => Some(LockMode::Compliance),
        "GOVERNANCE" => Some(LockMode::Governance),
        _ => None,
    }
}

/// `2032-10-17T04:45:26Z`: whole seconds, which every provider accepts.
fn rfc3339(at: Timestamp) -> String {
    format!("{:.0}", at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(status: u16, body: &str) -> Answer {
        Answer {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::IM_A_TEAPOT),
            headers: reqwest::header::HeaderMap::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn a_4xx_or_a_501_is_a_rejection_and_anything_else_is_not() {
        let denied = "<Error><Code>AccessDenied</Code><Message>no</Message></Error>";
        assert_eq!(
            rejection(&answer(403, denied)),
            Some(Rejected {
                status: 403,
                code: "AccessDenied".to_owned()
            })
        );
        assert_eq!(
            rejection(&answer(501, "<Error><Code>NotImplemented</Code></Error>")),
            Some(Rejected {
                status: 501,
                code: "NotImplemented".to_owned()
            })
        );
        assert_eq!(
            rejection(&answer(404, "")).map(|r| r.code),
            Some(String::new())
        );
        for status in [200, 204, 301, 500, 502, 503] {
            assert_eq!(rejection(&answer(status, denied)), None, "{status}");
        }
        assert_eq!(outcome(answer(204, "")), Ok(Outcome::Done));
        assert_eq!(
            outcome(answer(503, "")),
            Err(S3Error::Server { status: 503 })
        );
        assert_eq!(
            read_ok(answer(501, ""), "GetBucketVersioning").map(|_| ()),
            Err(S3Error::Rejected {
                call: "GetBucketVersioning",
                status: 501,
                code: String::new()
            })
        );
    }

    #[test]
    fn element_reads_the_first_flat_element() {
        let xml = "<a><Mode>COMPLIANCE</Mode><Mode>GOVERNANCE</Mode><Empty></Empty></a>";
        assert_eq!(element(xml, "Mode"), Some("COMPLIANCE"));
        assert_eq!(element(xml, "Empty"), Some(""));
        assert_eq!(element(xml, "Days"), None);
        assert_eq!(element("<Days>7", "Days"), None);
    }
}
