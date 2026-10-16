//! The rehearsal server's OAuth (DEC-922 item 3, [DEC-926]): just enough of an authorization
//! server that `mandate-mcp`'s real login (O1a discovery and registration, O1b PKCE, the
//! callback and the code exchange) runs against the simulator with no step bypassed.
//!
//! - **Metadata** (RFC 9728, RFC 8414): the protected resource is the MCP endpoint and its one
//!   authorization server is `http://127.0.0.1:<port>/oauth`, the issuer, on the same loopback
//!   listener.
//! - **Registration** (RFC 7591): a public client only (`token_endpoint_auth_method` `none`),
//!   with exactly one loopback redirect `http://127.0.0.1:<port>/callback` (RFC 8252 §7.3).
//! - **Authorization**: the request must carry every member the login sends, once, for a
//!   registered client and its own redirect, with an `S256` challenge and the endpoint as its
//!   `resource`; anything else is a `400` with no redirect (RFC 6749 §4.1.2.1). The page asks the
//!   owner, and its request is decided once: approval redirects with the code, the `state` and the
//!   `iss` (RFC 9207), denial with `access_denied`.
//! - **Token** (RFC 6749 §4.1.3, RFC 7636 §4.6): a code is spent by its first redemption,
//!   refused or not; the grant, the client, the redirect, the resource and the verifier's `S256`
//!   must all match. No refresh token is issued.
//! - **Bearer**: every MCP request needs a token this server issued; without one it answers `401`
//!   with the challenge that starts discovery.
//!
//! Identifiers, codes and tokens are counters, since the core reads no randomness: a test
//! double's credentials on loopback only, which guard fake money and are never a secret.
//!
//! [DEC-926]: ../../../docs/project/decisions/DEC-926.md

use std::net::SocketAddr;

use serde_json::{Map, Value, json};

const RESOURCE_METADATA: &str = "/.well-known/oauth-protected-resource/mcp";
const SERVER_METADATA: &str = "/.well-known/oauth-authorization-server/oauth";
const JSON: &str = "application/json";
const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
/// RFC 7636 §4.1's bounds on a verifier's length.
const VERIFIER_LENGTH: std::ops::RangeInclusive<usize> = 43..=128;
/// The length of BASE64URL(SHA-256(verifier)).
const CHALLENGE_LENGTH: usize = 43;

/// Where a request goes: answered here, or on to the MCP handler.
pub(crate) enum Routed {
    Answer(String),
    Mcp,
}

/// An authorization request the page shows, waiting for the owner's decision.
#[derive(Debug)]
struct Pending {
    id: String,
    client_id: String,
    redirect: String,
    challenge: String,
    state: String,
}

/// A code the owner's approval issued, not yet redeemed.
#[derive(Debug)]
struct Grant {
    code: String,
    client_id: String,
    redirect: String,
    challenge: String,
}

/// The authorization server's state: its clients, pending requests, codes and tokens.
#[derive(Debug)]
pub(crate) struct OAuth {
    base: String,
    clients: Vec<(String, String)>,
    pending: Vec<Pending>,
    grants: Vec<Grant>,
    tokens: Vec<String>,
    issued: u64,
}

impl OAuth {
    /// The authorization server of the listener at `addr`.
    pub(crate) fn new(addr: SocketAddr) -> Self {
        Self {
            base: format!("http://{addr}"),
            clients: Vec::new(),
            pending: Vec::new(),
            grants: Vec::new(),
            tokens: Vec::new(),
            issued: 0,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/mcp", self.base)
    }

    fn issuer(&self) -> String {
        format!("{}/oauth", self.base)
    }

    /// The next identifier of a kind: every kind draws on one counter.
    fn next(&mut self, kind: &str) -> String {
        self.issued = self.issued.saturating_add(1);
        format!("rh-sim-{kind}-{}", self.issued)
    }

    /// Answers the OAuth requests, and passes `POST /mcp` on.
    pub(crate) fn route(&mut self, head: &str, body: &[u8]) -> Routed {
        let line = head.lines().next().unwrap_or_default();
        let (method, target) = match line.split(' ').collect::<Vec<_>>().as_slice() {
            [method, target, "HTTP/1.1"] => (*method, *target),
            _ => return Routed::Answer(error(400, "invalid_request")),
        };
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let body = std::str::from_utf8(body).unwrap_or_default();
        let answer = match (method, path) {
            ("POST", "/mcp") => return Routed::Mcp,
            ("GET", RESOURCE_METADATA) => {
                let metadata = json!({"resource": self.endpoint(),
                    "authorization_servers": [self.issuer()]});
                respond(200, &[], JSON, &metadata.to_string())
            }
            ("GET", SERVER_METADATA) => {
                let issuer = self.issuer();
                let metadata = json!({"issuer": issuer,
                    "authorization_endpoint": format!("{issuer}/authorize"),
                    "token_endpoint": format!("{issuer}/token"),
                    "registration_endpoint": format!("{issuer}/register"),
                    "response_types_supported": ["code"],
                    "grant_types_supported": ["authorization_code"],
                    "code_challenge_methods_supported": ["S256"],
                    "token_endpoint_auth_methods_supported": ["none"],
                    "authorization_response_iss_parameter_supported": true});
                respond(200, &[], JSON, &metadata.to_string())
            }
            ("POST", "/oauth/register") => self.register(body),
            ("GET", "/oauth/authorize") => self.authorize(query),
            ("POST", "/oauth/approve") => self.approve(body),
            ("POST", "/oauth/token") => self.token(body),
            _ => error(404, "not_found"),
        };
        Routed::Answer(answer)
    }

    /// Whether the request carries `authorization: Bearer` with a token this server issued.
    pub(crate) fn admits(&self, head: &str) -> bool {
        let fields = head.lines().skip(1).filter_map(|line| line.split_once(':'));
        let mut named =
            fields.filter(|(name, _)| name.trim().eq_ignore_ascii_case("authorization"));
        let presented = named.next().and_then(|(_, value)| {
            let (scheme, token) = value.trim().split_once(' ')?;
            scheme.eq_ignore_ascii_case("Bearer").then_some(token)
        });
        presented.is_some_and(|token| self.tokens.iter().any(|issued| issued == token))
    }

    /// The `401` whose challenge names the protected-resource metadata (RFC 9728 §5.1).
    pub(crate) fn challenge(&self) -> String {
        let named = format!(
            "Bearer resource_metadata=\"{}{RESOURCE_METADATA}\"",
            self.base
        );
        let body = json!({"error": "invalid_token"}).to_string();
        respond(401, &[("www-authenticate", &named)], JSON, &body)
    }

    fn register(&mut self, body: &str) -> String {
        let Ok(Value::Object(asked)) = serde_json::from_str::<Value>(body) else {
            return error(400, "invalid_client_metadata");
        };
        let redirect = match asked.get("redirect_uris").and_then(Value::as_array) {
            Some(listed) => match listed.as_slice() {
                [Value::String(only)] if is_loopback_redirect(only) => only.clone(),
                _ => return error(400, "invalid_redirect_uri"),
            },
            None => return error(400, "invalid_redirect_uri"),
        };
        let public = asked.get("token_endpoint_auth_method") == Some(&json!("none"));
        let grants = listed_or_absent(&asked, "grant_types", "authorization_code");
        let types = listed_or_absent(&asked, "response_types", "code");
        if !public || !grants || !types {
            return error(400, "invalid_client_metadata");
        }
        let client_id = self.next("client");
        self.clients.push((client_id.clone(), redirect.clone()));
        let registered = json!({"client_id": client_id, "redirect_uris": [redirect],
            "grant_types": ["authorization_code"], "response_types": ["code"],
            "token_endpoint_auth_method": "none"});
        respond(201, &[], JSON, &registered.to_string())
    }

    fn authorize(&mut self, query: &str) -> String {
        let Some(pairs) = form(query) else {
            return error(400, "invalid_request");
        };
        let members = [
            "response_type",
            "client_id",
            "redirect_uri",
            "code_challenge",
            "code_challenge_method",
            "state",
            "resource",
        ];
        let Some(
            [
                response_type,
                client_id,
                redirect,
                challenge,
                method,
                state,
                resource,
            ],
        ) = once_each(&pairs, members)
        else {
            return error(400, "invalid_request");
        };
        let registered = self
            .clients
            .iter()
            .any(|(id, uri)| *id == client_id && *uri == redirect);
        let challenge_shape = challenge.len() == CHALLENGE_LENGTH
            && challenge.bytes().all(|b| BASE64URL.contains(&b));
        if !registered
            || response_type != "code"
            || method != "S256"
            || !challenge_shape
            || state.is_empty()
            || resource != self.endpoint()
        {
            return error(400, "invalid_request");
        }
        let id = self.next("request");
        self.pending.push(Pending {
            id: id.clone(),
            client_id,
            redirect,
            challenge,
            state,
        });
        let page = format!(
            "<!doctype html><title>mandate-rh-sim login</title>\
             <p>The simulated broker asks: let Mandate trade this simulated account?</p>\
             <form method=\"post\" action=\"/oauth/approve\">\
             <input type=\"hidden\" name=\"request\" value=\"{id}\">\
             <button name=\"decision\" value=\"approve\">Approve</button>\
             <button name=\"decision\" value=\"deny\">Deny</button></form>"
        );
        respond(200, &[], "text/html; charset=utf-8", &page)
    }

    fn approve(&mut self, body: &str) -> String {
        let Some(pairs) = form(body) else {
            return error(400, "invalid_request");
        };
        let Some([request, decision]) = once_each(&pairs, ["request", "decision"]) else {
            return error(400, "invalid_request");
        };
        if decision != "approve" && decision != "deny" {
            return error(400, "invalid_request");
        }
        let Some(at) = self.pending.iter().position(|p| p.id == request) else {
            return error(400, "invalid_request");
        };
        let pending = self.pending.remove(at);
        let issuer = self.issuer();
        let answer = if decision == "approve" {
            let code = self.next("code");
            let query = encode(&[("code", &code), ("state", &pending.state), ("iss", &issuer)]);
            self.grants.push(Grant {
                code,
                client_id: pending.client_id,
                redirect: pending.redirect.clone(),
                challenge: pending.challenge,
            });
            query
        } else {
            let pairs = [
                ("error", "access_denied"),
                ("state", pending.state.as_str()),
                ("iss", issuer.as_str()),
            ];
            encode(&pairs)
        };
        let location = format!("{}?{answer}", pending.redirect);
        respond(302, &[("location", &location)], "text/plain", "")
    }

    fn token(&mut self, body: &str) -> String {
        let Some(pairs) = form(body) else {
            return error(400, "invalid_request");
        };
        let members = [
            "grant_type",
            "code",
            "redirect_uri",
            "client_id",
            "code_verifier",
            "resource",
        ];
        let code = once(&pairs, "code");
        let spent = code.and_then(|code| {
            let at = self.grants.iter().position(|g| g.code == code)?;
            Some(self.grants.remove(at))
        });
        let Some([grant_type, _, redirect, client_id, verifier, resource]) =
            once_each(&pairs, members)
        else {
            return error(400, "invalid_request");
        };
        let Some(grant) = spent else {
            return error(400, "invalid_grant");
        };
        if grant_type != "authorization_code" {
            return error(400, "unsupported_grant_type");
        }
        let verifier_shape = VERIFIER_LENGTH.contains(&verifier.len())
            && verifier
                .bytes()
                .all(|b| b"-._~".contains(&b) || b.is_ascii_alphanumeric());
        let computed = base64url(mandate_canon::Digest::of(verifier.as_bytes()).as_bytes());
        if client_id != grant.client_id
            || redirect != grant.redirect
            || resource != self.endpoint()
            || !verifier_shape
            || computed != grant.challenge
        {
            return error(400, "invalid_grant");
        }
        let token = self.next("token");
        self.tokens.push(token.clone());
        let granted = json!({"access_token": token, "token_type": "Bearer", "expires_in": 3600});
        respond(
            200,
            &[("cache-control", "no-store")],
            JSON,
            &granted.to_string(),
        )
    }
}

/// An absent member, or exactly the one-element list `[only]`.
fn listed_or_absent(asked: &Map<String, Value>, key: &str, only: &str) -> bool {
    asked.get(key).is_none_or(|listed| *listed == json!([only]))
}

/// `http://127.0.0.1:<port>/callback`, the port written plainly and not zero.
fn is_loopback_redirect(uri: &str) -> bool {
    uri.strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.strip_suffix("/callback"))
        .and_then(|port| port.parse::<u16>().ok().filter(|p| p.to_string() == port))
        .is_some_and(|port| port != 0)
}

/// The value of `name` when it appears exactly once.
fn once(pairs: &[(String, String)], name: &str) -> Option<String> {
    let mut named = pairs.iter().filter(|(key, _)| key == name);
    match (named.next(), named.next()) {
        (Some((_, value)), None) => Some(value.clone()),
        _ => None,
    }
}

/// Each of `names` once, in order, or `None` if any is missing or repeated.
fn once_each<const N: usize>(pairs: &[(String, String)], names: [&str; N]) -> Option<[String; N]> {
    let values: Vec<String> = names
        .iter()
        .map(|name| once(pairs, name))
        .collect::<Option<_>>()?;
    values.try_into().ok()
}

/// An `application/x-www-form-urlencoded` text as its pairs, `None` when an escape is broken or
/// the result is not UTF-8.
fn form(text: &str) -> Option<Vec<(String, String)>> {
    text.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            Some((decode(key)?, decode(value)?))
        })
        .collect()
}

fn decode(text: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut rest = text.as_bytes();
    while let Some((&first, tail)) = rest.split_first() {
        rest = tail;
        match first {
            b'%' => {
                let hex = rest.get(..2)?;
                let hex = std::str::from_utf8(hex).ok()?;
                bytes.push(u8::from_str_radix(hex, 16).ok()?);
                rest = rest.get(2..)?;
            }
            b'+' => bytes.push(b' '),
            other => bytes.push(other),
        }
    }
    String::from_utf8(bytes).ok()
}

/// Pairs as a query, every byte but the unreserved ones percent-encoded.
fn encode(pairs: &[(&str, &str)]) -> String {
    let escape = |text: &str| -> String {
        text.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    char::from(b).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    };
    let encoded: Vec<String> = pairs
        .iter()
        .map(|(key, value)| format!("{}={}", escape(key), escape(value)))
        .collect();
    encoded.join("&")
}

/// Unpadded base64url (RFC 7636 appendix A).
fn base64url(bytes: &[u8]) -> String {
    let mut text = String::new();
    for group in bytes.chunks(3) {
        let mut word = [0_u8; 4];
        for (to, from) in word.iter_mut().skip(1).zip(group) {
            *to = *from;
        }
        let word = u32::from_be_bytes(word);
        for shift in [18_u32, 12, 6, 0]
            .into_iter()
            .take(group.len().saturating_add(1))
        {
            let at = usize::try_from(word.wrapping_shr(shift) & 0x3f).ok();
            if let Some(&letter) = at.and_then(|at| BASE64URL.get(at)) {
                text.push(char::from(letter));
            }
        }
    }
    text
}

/// An OAuth error answer (RFC 6749 §5.2), or a `404` for a path the server does not serve.
fn error(status: u16, code: &str) -> String {
    let body = json!({"error": code}).to_string();
    respond(status, &[], JSON, &body)
}

/// An HTTP answer that closes the connection.
pub(crate) fn respond(
    status: u16,
    headers: &[(&str, &str)],
    content_type: &str,
    body: &str,
) -> String {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        _ => "Not Found",
    };
    let extra: String = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect();
    format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\n{extra}\
         content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}
