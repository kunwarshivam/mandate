//! R2's lock rules read through a loopback fake of Cloudflare's API: the request is the documented
//! `GET .../accounts/{account_id}/r2/buckets/{bucket}/lock` with the read token as a bearer, and
//! whatever the rules say, the check refuses them for §6.2 (DEC-973 item 5).

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use mandate_objectstore::check::CheckError;
use mandate_objectstore::lock::{Refusal, Refused};
use mandate_objectstore::r2::{CLOUDFLARE_API, R2LockApi, check_r2};
use mandate_objectstore::s3::{ConfigError, S3Error};
use secrecy::SecretString;

/// Serves one request with `status` and `body`, and hands back the request line and headers.
fn fake(status: &str, body: &str) -> (String, mpsc::Receiver<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("address: {error}"));
    let (sent, received) = mpsc::channel();
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .unwrap_or_else(|error| panic!("clone: {error}")),
        );
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            lines.push(line.trim_end().to_owned());
        }
        let _ = reader.take(0).read_to_end(&mut Vec::new());
        let _ = stream.write_all(response.as_bytes());
        let _ = sent.send(lines);
    });
    (format!("http://{address}/client/v4"), received)
}

fn api(base: &str) -> R2LockApi {
    R2LockApi::new(
        base,
        "acct123",
        "journal-cold",
        SecretString::from("read-token"),
    )
    .unwrap_or_else(|error| panic!("valid: {error}"))
}

fn refused(result: Result<impl std::fmt::Debug, CheckError>) -> BTreeSet<Refusal> {
    match result {
        Err(CheckError::Refused(Refused(set))) => set,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn an_indefinite_rule_is_read_and_still_refused() {
    let (base, request) = fake(
        "200 OK",
        r#"{"success":true,"errors":[],"messages":[],"result":{"rules":[{"id":"all","enabled":true,"condition":{"type":"Indefinite"}}]}}"#,
    );
    let api = api(&base);
    assert_eq!(
        api.url(),
        format!("{base}/accounts/acct123/r2/buckets/journal-cold/lock")
    );
    assert_eq!(
        refused(check_r2(&api, "cold/").await),
        BTreeSet::from([Refusal::GovernanceOnly])
    );
    let lines = request
        .recv()
        .unwrap_or_else(|error| panic!("request seen: {error}"));
    assert_eq!(
        lines.first().map(String::as_str),
        Some("GET /client/v4/accounts/acct123/r2/buckets/journal-cold/lock HTTP/1.1")
    );
    assert!(
        lines
            .iter()
            .any(|line| line.eq_ignore_ascii_case("authorization: Bearer read-token")),
        "{lines:?}"
    );
}

#[tokio::test]
async fn no_rules_is_refused_as_unlocked() {
    let (base, _request) = fake("200 OK", r#"{"success":true,"result":{"rules":[]}}"#);
    assert_eq!(
        refused(check_r2(&api(&base), "cold/").await),
        BTreeSet::from([Refusal::NoLock, Refusal::GovernanceOnly])
    );
}

#[tokio::test]
async fn an_api_failure_is_not_evidence() {
    let (base, _request) = fake("403 Forbidden", r#"{"success":false,"result":null}"#);
    assert!(matches!(
        check_r2(&api(&base), "cold/").await,
        Err(CheckError::Store(S3Error::Rejected { status: 403, .. }))
    ));
    let (base, _request) = fake("200 OK", "not json");
    assert!(matches!(
        check_r2(&api(&base), "cold/").await,
        Err(CheckError::Store(S3Error::Malformed { .. }))
    ));
    let (base, _request) = fake("500 Internal Server Error", "{}");
    assert_eq!(
        check_r2(&api(&base), "cold/").await.map(|_| ()),
        Err(CheckError::Store(S3Error::Server { status: 500 }))
    );
    assert_eq!(
        check_r2(&api("http://127.0.0.1:1"), "cold/")
            .await
            .map(|_| ()),
        Err(CheckError::Store(S3Error::Transport))
    );
}

#[test]
fn the_api_base_is_https_or_loopback() {
    assert!(
        R2LockApi::new(
            CLOUDFLARE_API,
            "a1",
            "journal-cold",
            SecretString::from("t")
        )
        .is_ok()
    );
    assert_eq!(
        R2LockApi::new(
            "http://api.cloudflare.com/client/v4",
            "a1",
            "journal-cold",
            SecretString::from("t")
        )
        .map(|_| ()),
        Err(ConfigError::PlainHttp)
    );
    for (account, bucket) in [
        ("", "journal-cold"),
        ("a/1", "journal-cold"),
        ("a1", "Journal"),
    ] {
        assert_eq!(
            R2LockApi::new(CLOUDFLARE_API, account, bucket, SecretString::from("t")).map(|_| ()),
            Err(ConfigError::Name),
            "{account} {bucket}"
        );
    }
    let printed = format!("{:?}", api("https://api.cloudflare.com/client/v4"));
    assert!(!printed.contains("read-token"), "{printed}");
}
