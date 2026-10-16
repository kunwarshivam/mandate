//! The rehearsal server's OAuth (DEC-922 item 3, DEC-926): the protected-resource and
//! authorization-server metadata the O1a discovery reads, public-client registration with one
//! loopback redirect, the authorization page and its single-use request, and a token endpoint
//! that checks PKCE `S256`, the client, the redirect and the resource, and redeems a code once.
//! Every MCP request needs a token the server issued; without one it draws the challenge that
//! starts discovery. Oracles: RFC 6749, 7591, 7636, 8414 and 9728 member names written here, the
//! S256 challenge computed with `sha2` and a bit-by-bit base64url of the test's own, and the
//! core driven directly.

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;

use common::wire::{Reply, Wire, http};
use common::{AGENTIC, ref_id};
use mandate_rh_sim::{Script, ServerError, SimServer, State};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

type Outcome = Result<(), ServerError>;

const FORM: &str = "application/x-www-form-urlencoded";
const JSON: &str = "application/json";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

/// The script every test serves: one agentic account, SPY quoted at 500 in the regular session,
/// and two scripted fills, a SPY sell at 501 listed before a SPY buy at 499.5, so a buy can only
/// take the buy's.
fn script() -> Script {
    Script::parse(
        &json!({
            "accounts": [{"number": AGENTIC, "agentic_allowed": true, "cash": "750.25",
                "buying_power": "10000", "pattern_day_trader": false}],
            "session": "regular",
            "quotes": {"SPY": "500"},
            "fills": [{"symbol": "SPY", "side": "sell", "price": "501"},
                {"symbol": "SPY", "side": "buy", "price": "499.5"}],
        })
        .to_string(),
    )
    .unwrap()
}

fn rehearsal() -> SimServer {
    SimServer::rehearse(script(), 0).unwrap()
}

fn base(server: &SimServer) -> String {
    format!("http://{}", server.addr().unwrap())
}

/// Unpadded base64url, one bit at a time: the test's own encoder.
fn b64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let bits: String = bytes.iter().map(|b| format!("{b:08b}")).collect();
    let sextet = |c: &[u8]| {
        let text = format!("{:0<6}", String::from_utf8(c.to_vec()).unwrap());
        char::from(ALPHABET[usize::from_str_radix(&text, 2).unwrap()])
    };
    bits.as_bytes().chunks(6).map(sextet).collect()
}

fn challenge(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

/// Every byte but the unreserved ones percent-encoded.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' => {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                at += 3;
            }
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            b => {
                out.push(b);
                at += 1;
            }
        }
    }
    String::from_utf8(out).unwrap()
}

fn form(pairs: &[(&str, &str)]) -> String {
    let encoded: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect();
    encoded.join("&")
}

/// Every value of `name` in a query.
fn members(query: &str, name: &str) -> Vec<String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(key, _)| decode(key) == name)
        .map(|(_, value)| decode(value))
        .collect()
}

fn json_of(reply: &Reply) -> Value {
    assert_eq!(reply.header("content-type"), Some(JSON), "{}", reply.body);
    serde_json::from_str(&reply.body).unwrap()
}

fn register(server: &SimServer, redirect: &str) -> Reply {
    let asked = json!({"client_name": "Mandate", "redirect_uris": [redirect],
        "grant_types": ["authorization_code"], "response_types": ["code"],
        "token_endpoint_auth_method": "none"});
    let url = format!("{}/oauth/register", base(server));
    http("POST", &url, Some(JSON), &asked.to_string())
}

fn client_id(server: &SimServer, redirect: &str) -> String {
    let reply = register(server, redirect);
    assert_eq!(reply.status, 201, "{}", reply.body);
    json_of(&reply)["client_id"].as_str().unwrap().to_owned()
}

/// The authorization request's members, `resource` the server's own MCP endpoint.
fn authorize_query(server: &SimServer, client: &str, redirect: &str, state: &str) -> String {
    authorize_for(server, client, redirect, state, VERIFIER)
}

/// [`authorize_query`] with the challenge of `verifier`.
fn authorize_for(
    server: &SimServer,
    client: &str,
    redirect: &str,
    state: &str,
    verifier: &str,
) -> String {
    let resource = server.url().unwrap();
    form(&[
        ("response_type", "code"),
        ("client_id", client),
        ("redirect_uri", redirect),
        ("code_challenge", &challenge(verifier)),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("resource", &resource),
    ])
}

/// The request id the authorization page's form carries.
fn request_id(page: &Reply) -> String {
    assert_eq!(page.status, 200, "{}", page.body);
    let marker = "name=\"request\" value=\"";
    let at = page.body.find(marker).unwrap() + marker.len();
    let rest = &page.body[at..];
    rest[..rest.find('"').unwrap()].to_owned()
}

/// The page, then the owner's decision; the redirect's query.
fn decide(server: &SimServer, query: &str, decision: &str) -> Reply {
    let page = http(
        "GET",
        &format!("{}/oauth/authorize?{query}", base(server)),
        None,
        "",
    );
    let request = request_id(&page);
    let body = form(&[("request", &request), ("decision", decision)]);
    http(
        "POST",
        &format!("{}/oauth/approve", base(server)),
        Some(FORM),
        &body,
    )
}

fn redirected_query(reply: &Reply, redirect: &str) -> String {
    assert_eq!(reply.status, 302, "{}", reply.body);
    let location = reply.header("location").unwrap();
    let query = location.strip_prefix(&format!("{redirect}?")).unwrap();
    query.to_owned()
}

fn token(server: &SimServer, pairs: &[(&str, &str)]) -> Reply {
    let url = format!("{}/oauth/token", base(server));
    http("POST", &url, Some(FORM), &form(pairs))
}

struct Granted {
    code: String,
    client: String,
    redirect: String,
}

fn granted(server: &SimServer) -> Granted {
    granted_for(server, VERIFIER)
}

/// A code granted for the challenge of `verifier`. Its redirect writes the unreserved `state`
/// as it is, unescaped.
fn granted_for(server: &SimServer, verifier: &str) -> Granted {
    let redirect = "http://127.0.0.1:49152/callback".to_owned();
    let client = client_id(server, &redirect);
    let query = authorize_for(server, &client, &redirect, "state-1", verifier);
    let answer = redirected_query(&decide(server, &query, "approve"), &redirect);
    assert!(answer.contains("state=state-1"), "{answer}");
    assert_eq!(members(&answer, "state"), ["state-1"]);
    let code = members(&answer, "code").pop().unwrap();
    Granted {
        code,
        client,
        redirect,
    }
}

fn redeem<'a>(server: &SimServer, grant: &'a Granted, verifier: &'a str) -> Reply {
    let resource = server.url().unwrap();
    token(
        server,
        &[
            ("grant_type", "authorization_code"),
            ("code", &grant.code),
            ("redirect_uri", &grant.redirect),
            ("client_id", &grant.client),
            ("code_verifier", verifier),
            ("resource", &resource),
        ],
    )
}

fn access_token(server: &SimServer) -> String {
    let grant = granted(server);
    let reply = redeem(server, &grant, VERIFIER);
    assert_eq!(reply.status, 200, "{}", reply.body);
    let answer = json_of(&reply);
    assert_eq!(answer["token_type"], "Bearer");
    answer["access_token"].as_str().unwrap().to_owned()
}

#[test]
fn an_unauthenticated_mcp_request_draws_the_challenge_naming_the_resource_metadata() -> Outcome {
    let server = rehearsal();
    let mut wire = Wire::new(&server.url()?);
    let reply = wire.request("initialize", json!({}));
    assert_eq!(reply.status, 401, "{}", reply.body);
    let metadata = format!("{}/.well-known/oauth-protected-resource/mcp", base(&server));
    assert_eq!(
        reply.header("www-authenticate"),
        Some(format!("Bearer resource_metadata=\"{metadata}\"").as_str())
    );
    wire.bearer = Some("rh-sim-token-forged".to_owned());
    assert_eq!(wire.request("initialize", json!({})).status, 401);
    Ok(())
}

#[test]
fn the_metadata_names_the_endpoint_and_one_loopback_authorization_server() -> Outcome {
    let server = rehearsal();
    let base = base(&server);
    let url = format!("{base}/.well-known/oauth-protected-resource/mcp");
    let resource = json_of(&http("GET", &url, None, ""));
    let issuer = format!("{base}/oauth");
    assert_eq!(
        resource,
        json!({"resource": server.url()?, "authorization_servers": [issuer]})
    );
    let url = format!("{base}/.well-known/oauth-authorization-server/oauth");
    let metadata = json_of(&http("GET", &url, None, ""));
    assert_eq!(metadata["issuer"], issuer);
    assert_eq!(
        metadata["authorization_endpoint"],
        format!("{issuer}/authorize")
    );
    assert_eq!(metadata["token_endpoint"], format!("{issuer}/token"));
    assert_eq!(
        metadata["registration_endpoint"],
        format!("{issuer}/register")
    );
    assert_eq!(metadata["response_types_supported"], json!(["code"]));
    assert_eq!(
        metadata["code_challenge_methods_supported"],
        json!(["S256"])
    );
    assert_eq!(
        metadata["grant_types_supported"],
        json!(["authorization_code"])
    );
    assert_eq!(
        metadata["token_endpoint_auth_methods_supported"],
        json!(["none"])
    );
    let unknown = http("GET", &format!("{base}/.well-known/other"), None, "");
    assert_eq!(unknown.status, 404);
    Ok(())
}

#[test]
fn registration_takes_a_public_client_with_one_loopback_redirect_only() -> Outcome {
    let server = rehearsal();
    let redirect = "http://127.0.0.1:49152/callback";
    let reply = register(&server, redirect);
    assert_eq!(reply.status, 201, "{}", reply.body);
    let answer = json_of(&reply);
    assert!(
        answer["client_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert_eq!(answer["redirect_uris"], json!([redirect]));
    assert_eq!(answer["token_endpoint_auth_method"], "none");
    assert_eq!(answer["grant_types"], json!(["authorization_code"]));
    assert_eq!(answer["response_types"], json!(["code"]));
    assert_eq!(answer.get("client_secret"), None);
    for bad in [
        "https://evil.example/callback",
        "http://localhost:49152/callback",
        "http://127.0.0.1:49152/other",
        "http://127.0.0.1/callback",
    ] {
        assert_eq!(register(&server, bad).status, 400, "{bad}");
    }
    let url = format!("{}/oauth/register", base(&server));
    let secret = json!({"redirect_uris": [redirect], "token_endpoint_auth_method":
        "client_secret_basic", "grant_types": ["authorization_code"], "response_types": ["code"]});
    assert_eq!(
        http("POST", &url, Some(JSON), &secret.to_string()).status,
        400
    );
    let two = json!({"redirect_uris": [redirect, redirect], "token_endpoint_auth_method": "none"});
    assert_eq!(http("POST", &url, Some(JSON), &two.to_string()).status, 400);
    assert_eq!(http("POST", &url, Some(JSON), "not json").status, 400);
    Ok(())
}

#[test]
fn the_authorization_page_refuses_any_request_off_the_registered_client() -> Outcome {
    let server = rehearsal();
    let redirect = "http://127.0.0.1:49152/callback";
    let client = client_id(&server, redirect);
    let good = authorize_query(&server, &client, redirect, "s");
    let page = http(
        "GET",
        &format!("{}/oauth/authorize?{good}", base(&server)),
        None,
        "",
    );
    assert!(
        page.header("content-type")
            .unwrap()
            .starts_with("text/html")
    );
    request_id(&page);
    let resource = server.url()?;
    let challenge = challenge(VERIFIER);
    let full = [
        ("response_type", "code"),
        ("client_id", client.as_str()),
        ("redirect_uri", redirect),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", "s"),
        ("resource", resource.as_str()),
    ];
    let mut cases: Vec<Vec<(&str, &str)>> = Vec::new();
    for skip in 0..full.len() {
        let mut missing = full.to_vec();
        missing.remove(skip);
        cases.push(missing);
    }
    let swaps = [
        (0, "token"),
        (1, "rh-sim-client-99"),
        (2, "http://127.0.0.1:49153/callback"),
        (3, ""),
        (4, "plain"),
        (5, ""),
        (6, "http://127.0.0.1:1/mcp"),
    ];
    for (at, value) in swaps {
        let mut swapped = full.to_vec();
        swapped[at].1 = value;
        cases.push(swapped);
    }
    let mut doubled = full.to_vec();
    doubled.push(("state", "t"));
    cases.push(doubled);
    for case in cases {
        let url = format!("{}/oauth/authorize?{}", base(&server), form(&case));
        let reply = http("GET", &url, None, "");
        assert_eq!(reply.status, 400, "{case:?}");
        assert_eq!(reply.header("location"), None, "{case:?}");
    }
    Ok(())
}

#[test]
fn approval_redirects_once_with_the_code_state_and_issuer_and_denial_with_an_error() -> Outcome {
    let server = rehearsal();
    let redirect = "http://127.0.0.1:49152/callback";
    let client = client_id(&server, redirect);
    let query = authorize_query(&server, &client, redirect, "state with space&more");
    let page = http(
        "GET",
        &format!("{}/oauth/authorize?{query}", base(&server)),
        None,
        "",
    );
    let request = request_id(&page);
    let approve = format!("{}/oauth/approve", base(&server));
    let body = form(&[("request", &request), ("decision", "approve")]);
    let answer = redirected_query(&http("POST", &approve, Some(FORM), &body), redirect);
    assert_eq!(members(&answer, "state"), ["state with space&more"]);
    assert_eq!(
        members(&answer, "iss"),
        [format!("{}/oauth", base(&server))]
    );
    assert_eq!(members(&answer, "code").len(), 1);
    assert_eq!(members(&answer, "error").len(), 0);
    let again = http("POST", &approve, Some(FORM), &body);
    assert_eq!(again.status, 400, "a request is decided once");
    let denied = redirected_query(&decide(&server, &query, "deny"), redirect);
    assert_eq!(members(&denied, "error"), ["access_denied"]);
    assert_eq!(members(&denied, "code").len(), 0);
    let odd = form(&[("request", "rh-sim-request-0"), ("decision", "approve")]);
    assert_eq!(http("POST", &approve, Some(FORM), &odd).status, 400);
    Ok(())
}

#[test]
fn the_token_endpoint_checks_pkce_and_redeems_a_code_once() -> Outcome {
    let server = rehearsal();
    let grant = granted(&server);
    let wrong = redeem(&server, &grant, &"x".repeat(43));
    assert_eq!(wrong.status, 400);
    assert_eq!(json_of(&wrong)["error"], "invalid_grant");
    let grant = granted(&server);
    let reply = redeem(&server, &grant, VERIFIER);
    assert_eq!(reply.status, 200, "{}", reply.body);
    let answer = json_of(&reply);
    assert_eq!(answer["token_type"], "Bearer");
    assert!(
        answer["access_token"]
            .as_str()
            .is_some_and(|t| !t.is_empty())
    );
    assert_eq!(answer.get("refresh_token"), None);
    let replay = redeem(&server, &grant, VERIFIER);
    assert_eq!(replay.status, 400, "a code is redeemed once");
    assert_eq!(json_of(&replay)["error"], "invalid_grant");
    let resource = server.url()?;
    let fields = |grant: &Granted, at: usize, value: &str| {
        let mut pairs = vec![
            ("grant_type", "authorization_code".to_owned()),
            ("code", grant.code.clone()),
            ("redirect_uri", grant.redirect.clone()),
            ("client_id", grant.client.clone()),
            ("code_verifier", VERIFIER.to_owned()),
            ("resource", resource.clone()),
        ];
        pairs[at].1 = value.to_owned();
        pairs
    };
    for (at, value) in [
        (0, "client_credentials"),
        (2, "http://127.0.0.1:49153/callback"),
        (3, "rh-sim-client-99"),
        (5, "http://127.0.0.1:1/mcp"),
    ] {
        let grant = granted(&server);
        let pairs = fields(&grant, at, value);
        let borrowed: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(token(&server, &borrowed).status, 400, "{at} {value}");
        let after = redeem(&server, &grant, VERIFIER);
        assert_eq!(
            after.status, 400,
            "a refused redemption spends the code: {at}"
        );
    }
    Ok(())
}

#[test]
fn an_issued_token_opens_the_mcp_session_and_a_scripted_fill_follows_the_place() -> Outcome {
    let server = rehearsal();
    let token = access_token(&server);
    let mut wire = Wire::connect_with(&server.url()?, &token);
    assert_eq!(wire.list().len(), 9);
    let order = wire.call(
        "place_equity_order",
        json!({"account_number": AGENTIC, "symbol": "SPY", "side": "buy", "type": "limit",
            "quantity": "2", "limit_price": "500", "time_in_force": "gfd",
            "market_hours": "regular_hours", "ref_id": ref_id(1)}),
    );
    assert_eq!(order["isError"], false, "{order}");
    assert_eq!(order["structuredContent"]["state"], "confirmed");
    let id = order["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let held = server.drive(|sim| sim.orders(AGENTIC))?.unwrap();
    let placed = held.iter().find(|o| o.id == id).unwrap();
    assert_eq!(placed.state, State::Filled);
    assert_eq!(placed.executions.len(), 1);
    assert_eq!(placed.executions[0].price.to_string(), "499.5");
    let second = wire.call(
        "place_equity_order",
        json!({"account_number": AGENTIC, "symbol": "SPY", "side": "buy", "type": "limit",
            "quantity": "1", "limit_price": "500", "time_in_force": "gfd",
            "market_hours": "regular_hours", "ref_id": ref_id(2)}),
    );
    let id = second["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let held = server.drive(|sim| sim.orders(AGENTIC))?.unwrap();
    let unfilled = held.iter().find(|o| o.id == id).unwrap();
    assert_eq!(
        unfilled.state,
        State::Confirmed,
        "each scripted fill is used once"
    );
    Ok(())
}

#[test]
fn the_server_started_without_the_rehearsal_needs_no_token() -> Outcome {
    let server = SimServer::start(script().sim().unwrap(), mandate_rh_sim::Variant::Honest)?;
    let mut wire = Wire::connect(&server.url()?);
    assert_eq!(wire.list().len(), 9);
    let url = format!("{}/oauth/register", base(&server));
    assert_eq!(http("POST", &url, Some(JSON), "{}").status, 404);
    Ok(())
}

/// What a browser and `reqwest` send: a `GET` with no `content-length` has no body and is
/// answered; a chunked body is never read, and the connection closes unanswered.
#[test]
fn a_request_without_a_length_has_no_body_and_a_chunked_one_is_refused() -> Outcome {
    let server = rehearsal();
    let authority = server.addr()?.to_string();
    let send = |request: String| {
        let mut stream = TcpStream::connect(&authority).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut answer = String::new();
        drop(stream.read_to_string(&mut answer));
        answer
    };
    let get = format!(
        "GET /.well-known/oauth-protected-resource/mcp HTTP/1.1\r\nhost: {authority}\r\n\r\n"
    );
    assert!(send(get).starts_with("HTTP/1.1 200 OK\r\n"));
    let chunked = format!(
        "POST /oauth/register HTTP/1.1\r\nhost: {authority}\r\ntransfer-encoding: chunked\r\n\
         content-length: 2\r\n\r\n{{}}"
    );
    assert_eq!(send(chunked), "");
    Ok(())
}

/// Every answer's status line carries the reason phrase of its code.
#[test]
fn every_answer_carries_its_reason_phrase() -> Outcome {
    let server = rehearsal();
    let base = base(&server);
    let redirect = "http://127.0.0.1:49152/callback";
    assert_eq!(register(&server, redirect).line, "HTTP/1.1 201 Created");
    assert_eq!(register(&server, "bad").line, "HTTP/1.1 400 Bad Request");
    let metadata = format!("{base}/.well-known/oauth-protected-resource/mcp");
    assert_eq!(http("GET", &metadata, None, "").line, "HTTP/1.1 200 OK");
    let unknown = format!("{base}/other");
    assert_eq!(
        http("GET", &unknown, None, "").line,
        "HTTP/1.1 404 Not Found"
    );
    let mut wire = Wire::new(&server.url()?);
    let challenge = wire.request("initialize", json!({}));
    assert_eq!(challenge.line, "HTTP/1.1 401 Unauthorized");
    let client = client_id(&server, redirect);
    let query = authorize_query(&server, &client, redirect, "s");
    assert_eq!(
        decide(&server, &query, "approve").line,
        "HTTP/1.1 302 Found"
    );
    Ok(())
}

/// Each client id, code and token is the server's own: two of a kind are never equal, so one
/// client's code or token cannot be another's.
#[test]
fn every_client_code_and_token_is_distinct() -> Outcome {
    let server = rehearsal();
    let redirect = "http://127.0.0.1:49152/callback";
    assert_ne!(client_id(&server, redirect), client_id(&server, redirect));
    let (first, second) = (granted(&server), granted(&server));
    assert_ne!(first.code, second.code);
    assert_ne!(first.client, second.client);
    let tokens: Vec<String> = [first, second]
        .iter()
        .map(|grant| json_of(&redeem(&server, grant, VERIFIER))["access_token"].to_string())
        .collect();
    assert_ne!(tokens[0], tokens[1]);
    Ok(())
}

/// Registration refuses grant types or response types other than the login's, each alone.
#[test]
fn registration_refuses_other_grant_or_response_types_each_alone() -> Outcome {
    let server = rehearsal();
    let url = format!("{}/oauth/register", base(&server));
    let redirect = "http://127.0.0.1:49152/callback";
    for (grants, types) in [
        (json!(["implicit"]), json!(["code"])),
        (json!(["authorization_code"]), json!(["token"])),
        (
            json!(["authorization_code", "refresh_token"]),
            json!(["code"]),
        ),
    ] {
        let asked = json!({"redirect_uris": [redirect], "token_endpoint_auth_method": "none",
            "grant_types": grants, "response_types": types});
        let reply = http("POST", &url, Some(JSON), &asked.to_string());
        assert_eq!(reply.status, 400, "{asked}");
    }
    let bare = json!({"redirect_uris": [redirect], "token_endpoint_auth_method": "none"});
    assert_eq!(
        http("POST", &url, Some(JSON), &bare.to_string()).status,
        201
    );
    Ok(())
}

/// RFC 7636 §4.1: a verifier shorter than 43 or longer than 128 characters, or with a character
/// outside the unreserved set, is refused even when its challenge matches.
#[test]
fn a_verifier_of_the_wrong_length_or_alphabet_is_refused_though_its_challenge_matches() -> Outcome {
    let server = rehearsal();
    for verifier in [
        "a".repeat(42),
        "a".repeat(129),
        format!("{}+", "a".repeat(42)),
    ] {
        let grant = granted_for(&server, &verifier);
        let reply = redeem(&server, &grant, &verifier);
        assert_eq!(reply.status, 400, "{verifier}");
        assert_eq!(json_of(&reply)["error"], "invalid_grant");
    }
    for verifier in ["a".repeat(43), "a".repeat(128), "aA0-._~".repeat(7)] {
        let grant = granted_for(&server, &verifier);
        assert_eq!(redeem(&server, &grant, &verifier).status, 200, "{verifier}");
    }
    Ok(())
}
