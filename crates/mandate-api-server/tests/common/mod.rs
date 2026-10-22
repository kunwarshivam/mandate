//! A server on a background thread, and a blocking HTTP/1.1 client that sends raw requests, so
//! the tests see the wire bytes and need no HTTP client crate.

#![allow(dead_code, reason = "each test file uses part of the shared helpers")]

pub mod control;
pub mod passkey;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use axum::Router;

/// One answer, as the wire carried it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    /// Every value of header `name`, matched case-insensitively.
    pub fn all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// The one value of header `name`, or `None`.
    pub fn header(&self, name: &str) -> Option<&str> {
        let all = self.all(name);
        assert!(all.len() <= 1, "header {name} is repeated: {all:?}");
        all.first().copied()
    }

    /// Whether any header's name starts with `Access-Control-`.
    pub fn has_cors(&self) -> bool {
        self.headers
            .iter()
            .any(|(n, _)| n.to_ascii_lowercase().starts_with("access-control-"))
    }
}

/// Serves `router` on a loopback port from a thread of its own, for the rest of the test run.
pub fn serve(router: Router) -> SocketAddr {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("a loopback port");
            tx.send(listener.local_addr().expect("its address"))
                .expect("the test is waiting");
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .expect("serving");
        });
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("the server started")
}

/// Sends `method path` with `headers` and `body` and reads the whole answer.
pub fn send(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Reply {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a timeout");
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: api.test\r\nConnection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    stream.write_all(request.as_bytes()).expect("write");
    stream.write_all(body).expect("write the body");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read");
    parse(&raw)
}

/// A GET with no headers.
pub fn get(addr: SocketAddr, path: &str) -> Reply {
    send(addr, "GET", path, &[], b"")
}

fn parse(raw: &[u8]) -> Reply {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a header block");
    let head = std::str::from_utf8(&raw[..split]).expect("ASCII headers");
    let mut lines = head.split("\r\n");
    let status_line = lines.next().expect("a status line");
    let status = status_line
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("a status code");
    let headers: Vec<(String, String)> = lines
        .map(|line| {
            let (name, value) = line.split_once(':').expect("name: value");
            (name.trim().to_owned(), value.trim().to_owned())
        })
        .collect();
    let rest = raw[split + 4..].to_vec();
    let chunked = headers
        .iter()
        .any(|(n, v)| n.eq_ignore_ascii_case("transfer-encoding") && v.contains("chunked"));
    let body = if chunked { dechunk(&rest) } else { rest };
    Reply {
        status,
        headers,
        body,
    }
}

fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    loop {
        let end = raw
            .windows(2)
            .position(|w| w == b"\r\n")
            .expect("a chunk size line");
        let size = usize::from_str_radix(std::str::from_utf8(&raw[..end]).expect("hex"), 16)
            .expect("a chunk size");
        if size == 0 {
            return body;
        }
        body.extend_from_slice(&raw[end + 2..end + 2 + size]);
        raw = &raw[end + 4 + size..];
    }
}

/// base64url without padding (RFC 4648 §5), written out so the tests check the server with an
/// encoder of their own.
pub fn b64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let take = chunk.len() + 1;
        for i in 0..take {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

/// A software ES256 issuer: its key set as JSON, and tokens it signs.
pub struct TestIssuer {
    pair: ring::signature::EcdsaKeyPair,
    rng: ring::rand::SystemRandom,
    pub issuer: String,
    pub kid: String,
}

impl TestIssuer {
    pub fn new(kid: &str) -> Self {
        let rng = ring::rand::SystemRandom::new();
        let alg = &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING;
        let pkcs8 = ring::signature::EcdsaKeyPair::generate_pkcs8(alg, &rng).expect("a key");
        let pair = ring::signature::EcdsaKeyPair::from_pkcs8(alg, pkcs8.as_ref(), &rng)
            .expect("the key pair");
        Self {
            pair,
            rng,
            issuer: "https://issuer.test/auth/v1".to_owned(),
            kid: kid.to_owned(),
        }
    }

    pub fn jwks_json(&self) -> String {
        use ring::signature::KeyPair;
        let point = self.pair.public_key().as_ref();
        format!(
            r#"{{"keys":[{{"kty":"EC","crv":"P-256","kid":"{}","use":"sig","x":"{}","y":"{}"}}]}}"#,
            self.kid,
            b64url(&point[1..33]),
            b64url(&point[33..65])
        )
    }

    pub fn jwks(&self) -> mandate_authn::Jwks {
        mandate_authn::Jwks::parse(&self.jwks_json()).expect("the key set")
    }

    /// An access token for `subject` from `issuer`, for audience `authenticated`, expiring at the
    /// Unix second `exp`.
    pub fn token_from(&self, issuer: &str, subject: &str, exp: i64) -> String {
        let header = format!(r#"{{"alg":"ES256","typ":"JWT","kid":"{}"}}"#, self.kid);
        let payload = format!(
            r#"{{"iss":"{issuer}","sub":"{subject}","aud":"authenticated","exp":{exp},"role":"authenticated"}}"#
        );
        let input = format!(
            "{}.{}",
            b64url(header.as_bytes()),
            b64url(payload.as_bytes())
        );
        let signature = self
            .pair
            .sign(&self.rng, input.as_bytes())
            .expect("a signature");
        format!("{input}.{}", b64url(signature.as_ref()))
    }

    pub fn token(&self, subject: &str, exp: i64) -> String {
        self.token_from(&self.issuer.clone(), subject, exp)
    }
}

/// A clock the test moves.
pub struct ManualClock(pub std::sync::Mutex<i64>);

impl ManualClock {
    pub fn at(secs: i64) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self(std::sync::Mutex::new(secs)))
    }

    pub fn set(&self, secs: i64) {
        *self.0.lock().expect("unpoisoned") = secs;
    }

    pub fn secs(&self) -> i64 {
        *self.0.lock().expect("unpoisoned")
    }
}

impl mandate_api_server::clock::Clock for ManualClock {
    fn now(&self) -> Option<mandate_time::UtcNanos> {
        mandate_time::UtcNanos::from_parts(self.secs(), 0).ok()
    }
}

/// `UtcNanos` at the Unix second `secs`.
pub fn at(secs: i64) -> mandate_time::UtcNanos {
    mandate_time::UtcNanos::from_parts(secs, 0).expect("a time")
}

/// Lowercase hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Subject links read from the test's own file at `.0`, where the deployment reads only
/// [`mandate_api_server::subject::SUBJECT_LINKS`].
pub struct LinksAt(pub std::path::PathBuf);

impl mandate_api_server::subject::SubjectLinks for LinksAt {
    fn principal(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<mandate_identity::PrincipalId, mandate_api_server::subject::LinkRefusal> {
        mandate_api_server::subject::principal_at(&self.0, issuer, subject)
    }
}

/// The workspace the test deployment hosts, and its organization.
pub const WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Y";
pub const ORG: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3X";

pub fn hosted() -> mandate_api_server::members::Hosted {
    mandate_api_server::members::Hosted {
        org: mandate_identity::OrgId::from_ulid_text(ORG).expect("a ULID"),
        workspace: mandate_identity::WorkspaceId::from_ulid_text(WORKSPACE).expect("a ULID"),
        internal_paper: false,
    }
}

/// What the next append of a [`MemJournal`] does instead of appending, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scripted {
    /// Nothing written: `Unavailable`.
    Unavailable,
    /// Nothing written: `Fenced`.
    Fenced,
    /// Nothing written: `HeadMismatch`.
    HeadMismatch,
    /// Nothing written: `Ambiguous`.
    Ambiguous,
    /// The draft is stored, and the answer is lost: `Ambiguous`.
    CommittedThenAmbiguous,
    /// Nothing written: `IdempotencyConflict`.
    Conflict,
    /// A concurrent copy of the same call, a second later, lands first: `IdempotencyConflict`.
    RaceSame,
    /// Another command lands under the same id first: `IdempotencyConflict`.
    RaceOther,
    /// A concurrent copy of the same call, identical, lands first: `AlreadyCommitted`.
    RaceIdentical,
    /// The draft is refused: `Invalid`.
    Invalid,
    /// Nothing written: the call fails (`JournalUnavailable`).
    Failed,
}

/// An in-memory journal: rows by stream, streams that fail to read, and appends that follow the
/// append protocol's epochs, heads, and idempotency (journal spec §5.1), or what is scripted.
#[derive(Default)]
pub struct MemJournal {
    pub streams:
        std::sync::Mutex<std::collections::BTreeMap<String, Vec<mandate_journal::StoredEvent>>>,
    pub failing: std::sync::Mutex<std::collections::BTreeSet<String>>,
    pub epochs: std::sync::Mutex<std::collections::BTreeMap<String, u64>>,
    pub script: std::sync::Mutex<std::collections::VecDeque<Scripted>>,
    /// Every append call: the stream, the expected head, and the epoch.
    pub appends: std::sync::Mutex<Vec<(String, u64, u64)>>,
}

impl MemJournal {
    /// Queues what the next appends do.
    pub fn script(&self, steps: &[Scripted]) {
        self.script
            .lock()
            .expect("unpoisoned")
            .extend(steps.iter().copied());
    }

    /// `stream`'s rows now.
    pub fn rows_of(&self, stream: &str) -> Vec<mandate_journal::StoredEvent> {
        self.streams
            .lock()
            .expect("unpoisoned")
            .get(stream)
            .cloned()
            .unwrap_or_default()
    }

    /// Stores `draft` on `stream` as its next row.
    fn store(
        &self,
        stream: &str,
        recorded_at: mandate_time::UtcNanos,
        draft: &[u8],
    ) -> mandate_journal::StoredEvent {
        let value = mandate_canon::parse(draft).expect("a JSON draft");
        let text = |key: &str| {
            value
                .get(key)
                .and_then(mandate_canon::Value::as_str)
                .expect(key)
                .to_owned()
        };
        let mut streams = self.streams.lock().expect("unpoisoned");
        let rows = streams.entry(stream.to_owned()).or_default();
        let prev_hash = rows
            .last()
            .map_or(mandate_canon::Digest::ZERO, |row| row.hash);
        let row = mandate_journal::StoredEvent {
            stream_id: stream.to_owned(),
            seq: u64::try_from(rows.len()).expect("a count") + 1,
            event_id: text("event_id"),
            event_type: text("event_type"),
            schema_version: value
                .get("schema_version")
                .and_then(mandate_canon::Value::as_int)
                .expect("a version"),
            environment: text("environment"),
            recorded_at: recorded_at.to_string(),
            prev_hash,
            hash: mandate_canon::Digest::of(draft),
            body: draft.to_vec(),
        };
        rows.push(row.clone());
        row
    }
}

impl mandate_api_server::journal::Journal for MemJournal {
    fn event<'a>(
        &'a self,
        event_id: &'a str,
    ) -> mandate_api_server::journal::Answer<'a, Option<mandate_journal::StoredEvent>> {
        let found = self
            .streams
            .lock()
            .expect("unpoisoned")
            .values()
            .flatten()
            .find(|row| row.event_id == event_id)
            .cloned();
        Box::pin(async move { Ok(found) })
    }

    fn rows<'a>(&'a self, stream: &'a str) -> mandate_api_server::journal::Rows<'a> {
        let failing = self.failing.lock().expect("unpoisoned").contains(stream);
        let rows = self.rows_of(stream);
        Box::pin(async move {
            if failing {
                Err(mandate_api_server::journal::JournalUnavailable)
            } else {
                Ok(rows)
            }
        })
    }

    fn take_ownership<'a>(
        &'a self,
        stream: &'a str,
    ) -> mandate_api_server::journal::Answer<'a, u64> {
        let mut epochs = self.epochs.lock().expect("unpoisoned");
        let epoch = epochs.entry(stream.to_owned()).or_default();
        *epoch += 1;
        let taken = *epoch;
        Box::pin(async move { Ok(taken) })
    }

    fn head<'a>(&'a self, stream: &'a str) -> mandate_api_server::journal::Answer<'a, u64> {
        let seq = self.rows_of(stream).last().map_or(0, |row| row.seq);
        Box::pin(async move { Ok(seq) })
    }

    fn append<'a>(
        &'a self,
        stream: &'a str,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: mandate_time::UtcNanos,
        draft: &'a [u8],
    ) -> mandate_api_server::journal::Answer<'a, mandate_journal::AppendOutcome> {
        use mandate_journal::AppendOutcome;
        self.appends.lock().expect("unpoisoned").push((
            stream.to_owned(),
            expected_head,
            writer_epoch,
        ));
        let scripted = self.script.lock().expect("unpoisoned").pop_front();
        let outcome = match scripted {
            Some(Scripted::Failed) => Err(mandate_api_server::journal::JournalUnavailable),
            Some(Scripted::Unavailable) => Ok(AppendOutcome::Unavailable),
            Some(Scripted::Fenced) => Ok(AppendOutcome::Fenced {
                current_epoch: writer_epoch + 1,
            }),
            Some(Scripted::HeadMismatch) => Ok(AppendOutcome::HeadMismatch {
                actual_seq: expected_head + 1,
                actual_hash: mandate_canon::Digest::ZERO,
            }),
            Some(Scripted::Ambiguous) => Ok(AppendOutcome::Ambiguous),
            Some(Scripted::Conflict) => Ok(AppendOutcome::IdempotencyConflict { stored_seq: 1 }),
            Some(Scripted::RaceSame) => {
                let copy = edited(draft, "submitted_at", |v| {
                    mandate_canon::Value::Int(
                        mandate_canon::Int::new(v.as_int().expect("a second") + 1).expect("an int"),
                    )
                });
                let row = self.store(stream, recorded_at, &copy);
                Ok(AppendOutcome::IdempotencyConflict {
                    stored_seq: row.seq,
                })
            }
            Some(Scripted::RaceOther) => {
                let other = edited(draft, "subject", |_| {
                    mandate_canon::Value::Str("other".into())
                });
                let row = self.store(stream, recorded_at, &other);
                Ok(AppendOutcome::IdempotencyConflict {
                    stored_seq: row.seq,
                })
            }
            Some(Scripted::RaceIdentical) => {
                let row = self.store(stream, recorded_at, draft);
                Ok(AppendOutcome::AlreadyCommitted(vec![row]))
            }
            Some(Scripted::Invalid) => Ok(AppendOutcome::Invalid {
                draft: 0,
                error: mandate_journal::Invalid {
                    reason: mandate_journal::InvalidReason::Schema,
                    path: "payload".to_owned(),
                },
            }),
            Some(Scripted::CommittedThenAmbiguous) => {
                self.store(stream, recorded_at, draft);
                Ok(AppendOutcome::Ambiguous)
            }
            None => Ok(self.protocol(stream, expected_head, writer_epoch, recorded_at, draft)),
        };
        Box::pin(async move { outcome })
    }
}

/// `draft` with its payload member `name` replaced by `change` of its value.
fn edited(
    draft: &[u8],
    name: &str,
    change: impl FnOnce(&mandate_canon::Value) -> mandate_canon::Value,
) -> Vec<u8> {
    let mut value = mandate_canon::parse(draft).expect("a JSON draft");
    let mandate_canon::Value::Object(fields) = &mut value else {
        panic!("an object");
    };
    let Some(mandate_canon::Value::Object(payload)) = fields.get_mut("payload") else {
        panic!("a payload");
    };
    let old = payload.get(name).cloned().expect(name);
    payload.insert(mandate_canon::Key::new(name).expect("a key"), change(&old));
    mandate_canon::to_canonical(&value)
}

impl MemJournal {
    /// Journal spec §5.1's order: idempotency by event id, then the epoch, then the head.
    fn protocol(
        &self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: mandate_time::UtcNanos,
        draft: &[u8],
    ) -> mandate_journal::AppendOutcome {
        use mandate_journal::AppendOutcome;
        let value = mandate_canon::parse(draft).expect("a JSON draft");
        let id = value
            .get("event_id")
            .and_then(mandate_canon::Value::as_str)
            .expect("an id");
        let rows = self.rows_of(stream);
        if let Some(stored) = rows.iter().find(|row| row.event_id == id) {
            return if stored.body == draft {
                AppendOutcome::AlreadyCommitted(vec![stored.clone()])
            } else {
                AppendOutcome::IdempotencyConflict {
                    stored_seq: stored.seq,
                }
            };
        }
        let current = self
            .epochs
            .lock()
            .expect("unpoisoned")
            .get(stream)
            .copied()
            .unwrap_or(0);
        if writer_epoch != current {
            return AppendOutcome::Fenced {
                current_epoch: current,
            };
        }
        let head = rows.last().map_or(0, |row| row.seq);
        if expected_head != head {
            return AppendOutcome::HeadMismatch {
                actual_seq: head,
                actual_hash: mandate_canon::Digest::ZERO,
            };
        }
        AppendOutcome::Committed(vec![self.store(stream, recorded_at, draft)])
    }
}
