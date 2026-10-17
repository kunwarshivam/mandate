//! The local S3 test store the conformance suite runs against (CI starts it with
//! `.github/scripts/start-minio.sh`): its address and its test-only root key from the environment,
//! fresh buckets per test, and a writer key narrowed by an STS session policy.

use std::env;
use std::sync::atomic::{AtomicU32, Ordering};

use jiff::Timestamp;
use mandate_objectstore::lock::{LockMode, RetentionPeriod};
use mandate_objectstore::s3::{Endpoint, Outcome, S3};
use mandate_objectstore::sigv4::{self, Credentials, Request, Scope};
use secrecy::SecretString;

/// The store's address, `http://127.0.0.1:9000` in CI.
pub const URL: &str = "MANDATE_S3_URL";
/// The test store's root access key, a fixed test value, never a real credential.
pub const ACCESS_KEY: &str = "MANDATE_S3_ACCESS_KEY_ID";
/// The test store's root secret, a fixed test value, never a real credential.
pub const SECRET_KEY: &str = "MANDATE_S3_SECRET_ACCESS_KEY";
/// Set in CI, so the suite fails rather than skips when the store is missing.
pub const REQUIRED: &str = "MANDATE_S3_REQUIRED";

pub const REGION: &str = "us-east-1";

/// The root client for a fresh bucket, or `None` (after saying so) when no store is configured.
pub fn root(label: &str) -> Option<S3> {
    let Ok(url) = env::var(URL) else {
        assert!(
            env::var_os(REQUIRED).is_none(),
            "{REQUIRED} is set but {URL} is not"
        );
        eprintln!("skipping: {URL} is unset (see crates/mandate-objectstore/README.md)");
        return None;
    };
    let id = env::var(ACCESS_KEY).unwrap_or_else(|_| panic!("{ACCESS_KEY} is set"));
    let secret = env::var(SECRET_KEY).unwrap_or_else(|_| panic!("{SECRET_KEY} is set"));
    let endpoint = Endpoint::new(&url, REGION, &bucket_name(label))
        .unwrap_or_else(|error| panic!("the test endpoint is valid: {error}"));
    Some(
        S3::new(
            endpoint,
            Credentials::new(id, SecretString::from(secret), None),
            Timestamp::now,
        )
        .unwrap_or_else(|error| panic!("the client builds: {error}")),
    )
}

/// A bucket name no earlier run used: compliance-locked objects outlive every test, so a test
/// never reuses a bucket.
fn bucket_name(label: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let now = Timestamp::now();
    format!(
        "{label}-{}-{}-{n}",
        now.as_second(),
        now.subsec_nanosecond()
    )
}

/// Creates the bucket, with object lock when `rule` is `Some`, and that default rule when the
/// inner value is `Some`.
pub async fn create(root: &S3, lock: Option<Option<(LockMode, RetentionPeriod)>>) {
    let created = root.create_bucket(lock.is_some()).await;
    assert_eq!(
        created,
        Ok(Outcome::Done),
        "create {}",
        root.endpoint().bucket()
    );
    if let Some(Some((mode, period))) = lock {
        let mode = match mode {
            LockMode::Compliance => "COMPLIANCE",
            LockMode::Governance => "GOVERNANCE",
        };
        let period = match period {
            RetentionPeriod::Days(days) => format!("<Days>{days}</Days>"),
            RetentionPeriod::Years(years) => format!("<Years>{years}</Years>"),
        };
        let xml = format!(
            "<ObjectLockConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <ObjectLockEnabled>Enabled</ObjectLockEnabled><Rule><DefaultRetention>\
             <Mode>{mode}</Mode>{period}</DefaultRetention></Rule></ObjectLockConfiguration>"
        );
        assert_eq!(root.put_lock_configuration(&xml).await, Ok(Outcome::Done));
    }
}

/// What the narrowed writer may do: write, read and manage its own objects' retention, and read
/// the bucket's settings, but neither bypass governance nor write the bucket's lock rule.
pub const WRITER_ACTIONS: &[&str] = &[
    "s3:GetObject",
    "s3:GetObjectVersion",
    "s3:PutObject",
    "s3:GetObjectRetention",
    "s3:PutObjectRetention",
    "s3:DeleteObject",
    "s3:DeleteObjectVersion",
    "s3:GetBucketVersioning",
    "s3:GetBucketObjectLockConfiguration",
    "s3:ListBucket",
    "s3:ListBucketVersions",
];

/// A temporary key from STS `AssumeRole`, limited to `actions` by a session policy.
pub async fn narrowed(root: &S3, actions: &[&str]) -> S3 {
    let url = env::var(URL).unwrap_or_else(|_| panic!("{URL} is set"));
    let id = env::var(ACCESS_KEY).unwrap_or_else(|_| panic!("{ACCESS_KEY} is set"));
    let secret = env::var(SECRET_KEY).unwrap_or_else(|_| panic!("{SECRET_KEY} is set"));
    let policy = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{"Effect": "Allow", "Action": actions, "Resource": ["arn:aws:s3:::*"]}]
    })
    .to_string();
    let form = [
        ("Action", "AssumeRole"),
        ("Version", "2011-06-15"),
        ("DurationSeconds", "3600"),
        ("Policy", policy.as_str()),
    ];
    let body = form
        .iter()
        .map(|(name, value)| format!("{name}={}", sigv4::uri_encode(value, false)))
        .collect::<Vec<_>>()
        .join("&");
    let host = url
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_owned();
    let at = Timestamp::now();
    let date = sigv4::amz_date(at);
    let payload = sigv4::sha256_hex(body.as_bytes());
    let content_type = "application/x-www-form-urlencoded";
    let headers = [
        ("content-type", content_type),
        ("host", host.as_str()),
        ("x-amz-content-sha256", payload.as_str()),
        ("x-amz-date", date.as_str()),
    ];
    let key = Credentials::new(id, SecretString::from(secret), None);
    let authorization = sigv4::authorization(
        &key,
        &Request {
            method: "POST",
            path: "/",
            query: &[],
            headers: &headers,
            payload_sha256: &payload,
        },
        &Scope {
            region: REGION,
            service: "sts",
            at,
        },
    );
    let response = reqwest::Client::new()
        .post(format!("{}/", url.trim_end_matches('/')))
        .header("content-type", content_type)
        .header("x-amz-content-sha256", payload)
        .header("x-amz-date", date)
        .header("authorization", authorization)
        .body(body)
        .send()
        .await
        .unwrap_or_else(|error| panic!("AssumeRole is sent: {error}"));
    let status = response.status();
    let text = response
        .text()
        .await
        .unwrap_or_else(|error| panic!("AssumeRole answers: {error}"));
    assert!(status.is_success(), "AssumeRole: {status} {text}");
    let field = |name: &str| -> String {
        let open = format!("<{name}>");
        let start = text
            .find(&open)
            .map(|i| i + open.len())
            .unwrap_or_else(|| panic!("{name} in {text}"));
        let end = text[start..]
            .find('<')
            .unwrap_or_else(|| panic!("{name} ends"));
        text[start..start + end].to_owned()
    };
    root.with_credentials(Credentials::new(
        field("AccessKeyId"),
        SecretString::from(field("SecretAccessKey")),
        Some(SecretString::from(field("SessionToken"))),
    ))
}
