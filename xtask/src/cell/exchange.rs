//! The `exchange` drill (E10-22, DEC-2330 item 5, DEC-2063): the real token-exchange pod, as the
//! deployment manager starts it, redeems a code the cell's `oauth-fixture` issued against the
//! cell's vault. The owner signs in through the stub issuer and asks for a `connection` step-up
//! challenge; `mandate step-up confirm-connect` confirms it in a one-off Job; the API starts the
//! connect on that `cli_confirm` evidence, the fixture's authorize page gives a code, and the API's
//! callback writes it to the vault. The role provisioner then makes the connection's roles, the
//! manager starts the exchange, and the drill holds only once that pod has succeeded, the code is
//! taken, and the connection's token entry is format 1. The start is also the CLI and API's
//! contract test (DEC-2125): it passes only on the record the real CLI wrote. Paper and fixtures
//! only: the fixture's token is random, never a broker's, and never leaves the vault's pod.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::Digest;

use super::executor::{gestures, workspace_opened};
use super::scenario::{FOUNDER, Gesture, point_api};
use super::{
    API_LOCAL_PORT, FIXTURE_REDIRECT_URI, NAMESPACE, VAULT_INIT, Verdict, api_port_forward,
    in_vault_pod, kubeconfig, kubectl, open_session, reply_body, run_id, session_cookie, state,
    status_of, stub_token,
};

/// The disclosure the founder accepted (DEC-2622), which the connect's digest binds.
const DISCLOSURE: (&str, &str) = ("alpaca_oauth_token_breadth", "1");
/// The fixture's authority, as the API's authorize page names it (DEC-2633), and the `Host` the
/// fixture answers alone.
const FIXTURE_AUTHORITY: &str = "oauth-fixture:8091";
/// The local port the drill forwards to the fixture's Service.
const FIXTURE_LOCAL_PORT: u16 = 8791;
/// Polls, two seconds apart, for the manager's exchange pod to succeed.
const POLLS: u32 = 90;
const MANAGED_BY: &str = "app.kubernetes.io/managed-by=mandate-deploy-manager";
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The keys a token entry holds, format 1 (`mandate-connections`' `token_entry`), in the order its
/// encoding writes them.
const TOKEN_KEYS: [&str; 4] = ["access_token", "format", "scope", "token_type"];

/// The scopes a token entry records: the fixture grants `trading data`, which the exchange writes
/// sorted.
const TOKEN_SCOPES: [&str; 2] = ["data", "trading"];

/// The run's workspace: a ULID, fresh for each run id.
pub fn workspace_of(id: &str) -> String {
    format!("01KEXC{id:0>10.10}000000000X")
}

/// The connect's step-up digest, `sha256:` and the hex of SHA-256 over the canonical
/// `{action, broker, disclosure: {document, version}, environment, workspace_id}` (DEC-2621 item
/// 4), computed here, not read from the API.
pub fn connect_digest(workspace: &str) -> String {
    let (document, version) = DISCLOSURE;
    let object = format!(
        r#"{{"action":"connect","broker":"alpaca","disclosure":{{"document":"{document}","version":"{version}"}},"environment":"paper","workspace_id":"{workspace}"}}"#
    );
    let hex: String = sha2::Sha256::digest(object.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

/// The code the web app shows for a challenge: the first 40 bits of its `webauthn_challenge`
/// (base64url) as 8 Crockford base-32 characters, hyphenated as the web app writes it (DEC-2104
/// item 1).
pub fn shown_code(webauthn_challenge: &str) -> Option<String> {
    let mut bits: u64 = 0;
    for c in webauthn_challenge.bytes().take(8) {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u64::from(value);
    }
    if webauthn_challenge.len() < 8 {
        return None;
    }
    let first40 = bits >> 8;
    let code: String = (0..8)
        .map(|i| {
            let digit = usize::try_from((first40 >> (35 - 5 * i)) & 31).unwrap_or_default();
            char::from(CROCKFORD[digit])
        })
        .collect();
    Some(format!("{}-{}", &code[..4], &code[4..]))
}

/// A connection's segment in the vault's paths: `x` and its id in lowercase hex
/// (`mandate-vault`'s `connection_segment`), so an id with any character stays one segment.
pub fn segment(connection: &str) -> String {
    let hex: String = connection.bytes().map(|b| format!("{b:02x}")).collect();
    format!("x{hex}")
}

/// What is wrong with `entry`, the connection's token entry as the vault's pod hands it out with its
/// access token replaced by `present` when it has one, against format 1: nothing when it holds
/// exactly a non-empty bearer access token, format 1 and the two scopes.
pub fn token_problems(entry: &str) -> Vec<String> {
    let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(entry) else {
        return vec!["the token entry is not a JSON object".to_owned()];
    };
    let mut problems = Vec::new();
    let keys: Vec<&str> = fields.keys().map(String::as_str).collect();
    if keys != TOKEN_KEYS {
        problems.push(format!(
            "the token entry holds the keys {keys:?}, not {TOKEN_KEYS:?}"
        ));
    }
    if fields.get("format").and_then(Value::as_u64) != Some(1) {
        problems.push("the token entry is not format 1".to_owned());
    }
    if fields.get("token_type").and_then(Value::as_str) != Some("bearer") {
        problems.push("the token entry's type is not bearer".to_owned());
    }
    let scopes: Option<Vec<&str>> = fields
        .get("scope")
        .and_then(Value::as_array)
        .and_then(|s| s.iter().map(Value::as_str).collect());
    if scopes.as_deref() != Some(&TOKEN_SCOPES[..]) {
        problems.push(format!("the token entry's scopes are not {TOKEN_SCOPES:?}"));
    }
    if fields.get("access_token").and_then(Value::as_str) != Some("present") {
        problems.push("the token entry holds no access token".to_owned());
    }
    problems
}

/// Whether the connection's code is taken, from [`CODE_READ`]'s answer: its entry is the take's
/// tombstone, or gone; a code still readable, or no answer, is not taken.
pub fn code_taken(read: &str) -> bool {
    matches!(read.trim(), "taken" | "missing")
}

/// What a failed API call's report may say of its reply: the problem's `code` member alone, never
/// the body, which on a challenge carries the bytes the CLI's code is made from.
pub fn problem(reply: &str) -> String {
    serde_json::from_str::<Value>(reply_body(reply))
        .ok()
        .and_then(|body| body["code"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "no problem code".to_owned())
}

/// What a report may say of [`CODE_READ`]'s answer: one of its three words, never anything else
/// the pod printed.
pub fn code_state(read: &str) -> &'static str {
    match read.trim() {
        "taken" => "taken",
        "readable" => "readable",
        "missing" => "missing",
        _ => "unreadable",
    }
}

/// The phase of the token-exchange pod the manager started for `connection`, from `kubectl get
/// pods -o json` over the manager's token-exchange pods: the newest generation's, matched by its
/// `mandate.dev/subject` annotation; `None` while there is none.
pub fn exchange_phase(pods: &str, connection: &str) -> Option<String> {
    let pods: Value = serde_json::from_str(pods).ok()?;
    pods["items"]
        .as_array()?
        .iter()
        .filter(|pod| {
            pod["metadata"]["annotations"]["mandate.dev/subject"].as_str() == Some(connection)
        })
        .max_by_key(|pod| {
            pod["metadata"]["labels"]["mandate.dev/generation"]
                .as_str()
                .and_then(|g| g.parse::<u64>().ok())
                .unwrap_or_default()
        })
        .and_then(|pod| pod["status"]["phase"].as_str().map(str::to_owned))
}

/// The code and state the fixture's authorize page hands back in its redirect to `redirect_uri`:
/// `location` must be that URI with exactly `code` and `state` in its query, each non-empty.
pub fn authorize_redirect(location: &str, redirect_uri: &str) -> Option<(String, String)> {
    let query = location.strip_prefix(redirect_uri)?.strip_prefix('?')?;
    let mut code = None;
    let mut state = None;
    for pair in query.split('&') {
        match pair.split_once('=')? {
            ("code", value) if code.is_none() && !value.is_empty() => code = Some(value.to_owned()),
            ("state", value) if state.is_none() && !value.is_empty() => {
                state = Some(value.to_owned())
            }
            _ => return None,
        }
    }
    Some((code?, state?))
}

/// The path and query of the fixture's authorize page the start sends the browser to, which the
/// drill asks through its forward: `None` for a page on any other authority.
pub fn fixture_path(authorization_url: &str) -> Option<&str> {
    authorization_url
        .strip_prefix("http://")?
        .strip_prefix(FIXTURE_AUTHORITY)
        .filter(|rest| rest.starts_with("/alpaca/oauth/authorize?"))
}

/// The value of the `Location` header of an HTTP/1.1 reply.
pub fn location_of(reply: &str) -> Option<&str> {
    reply.split("\r\n\r\n").next()?.lines().find_map(|l| {
        let (name, value) = l.split_once(':')?;
        name.eq_ignore_ascii_case("location").then(|| value.trim())
    })
}

/// The gesture that confirms the connect's challenge with the code shown, as the owner runs it on
/// the host (DEC-2125 item 3).
pub fn confirm_connect(workspace: &str, challenge: &str, code: &str) -> Gesture {
    let args: Vec<String> = [
        "step-up",
        "confirm-connect",
        "--workspace",
        workspace,
        "--principal",
        FOUNDER,
        "--challenge",
        challenge,
        "--code",
        code,
        "--journal",
        "$(MANDATE_JOURNAL_DSN)",
        "--store",
        "/work/store",
    ]
    .map(String::from)
    .to_vec();
    Gesture {
        container: "g01-step-up-confirm-connect".into(),
        shows: "the connect's step-up, confirmed with the code shown",
        command: None,
        args,
        expect: format!("confirmed {challenge} as event"),
    }
}

/// The vault pod's script that reports the connection's code entry: `taken` for the take's
/// tombstone, `readable` for a code still there, `missing` for none. The root token is its stdin's
/// one line, so it reaches no argument, and nothing of the entry leaves the pod.
const CODE_READ: &str = "read -r BAO_TOKEN && export BAO_TOKEN && \
     if [ \"$(bao kv get -mount=mandate -field=taken \"$1\" 2>/dev/null)\" = true ]; then echo taken; \
     elif bao kv get -mount=mandate \"$1\" >/dev/null 2>&1; then echo readable; else echo missing; fi";

/// The vault pod's script that hands out the connection's token entry with its access token
/// replaced by `present`, so the token never leaves the pod: `sed -n …p` prints only an entry
/// whose token it replaced, and nothing for any other, so an entry it fails to match is never
/// printed.
const TOKEN_READ: &str = "read -r BAO_TOKEN && export BAO_TOKEN && \
     bao kv get -mount=mandate -field=value \"$1\" 2>/dev/null | base64 -d 2>/dev/null | \
     sed -nE 's/\"access_token\":\"[^\"]+\"/\"access_token\":\"present\"/p'";

/// The drill. In seeded mode the connect's step-up is never confirmed in the CLI, so the start must
/// refuse it, `step_up_missing`, and the drill reports that.
pub fn drill(seeded: bool) -> Result<Verdict> {
    let id = run_id()?;
    let workspace = workspace_of(&id);
    point_api(&workspace)?;
    let (done, logs) = gestures(&id, "o", &workspace_opened(&workspace), &[])?;
    if !done {
        return Ok(Verdict::Broken(format!(
            "the workspace failed to open:\n{logs}"
        )));
    }
    api_port_forward(true)?;
    let (_, reply) = open_session(&stub_token()?)?;
    let cookie = session_cookie(&reply).context("the sign-in set no session cookie")?;
    let base = format!("/v1/workspaces/{workspace}");
    let digest = connect_digest(&workspace);
    let asked = json!({"action": {"kind": "connection", "digest": digest}});
    let challenges = format!("{base}/step-up/challenges");
    let (status, reply) = api(&cookie, &key(&id, "challenge"), &challenges, &asked)?;
    let challenge: Value = serde_json::from_str(reply_body(&reply)).unwrap_or_default();
    let (Some(challenge_id), Some(issued_at), Some(code)) = (
        challenge["challenge_id"].as_str(),
        challenge["issued_at"].as_str(),
        challenge["webauthn_challenge"]
            .as_str()
            .and_then(shown_code),
    ) else {
        return Ok(Verdict::Broken(format!(
            "the connection challenge answered {status}: {}",
            problem(&reply)
        )));
    };
    if !seeded {
        let confirm = confirm_connect(&workspace, challenge_id, &code);
        let (done, logs) = gestures(&id, "c", &[confirm], &[])?;
        if !done || !logs.contains(&digest) {
            return Ok(Verdict::Broken(format!(
                "confirm-connect failed or confirmed another digest than {digest}:\n{logs}"
            )));
        }
    }
    let (document, version) = DISCLOSURE;
    let start = json!({
        "broker": "alpaca",
        "environment": "paper",
        "disclosure": {"document": document, "version": version},
        "step_up": {"assertion_id": challenge_id, "authenticated_at": issued_at, "method": "cli_confirm"},
    });
    let starts = format!("{base}/connections/oauth/start");
    let (status, reply) = api(&cookie, &key(&id, "start"), &starts, &start)?;
    let started: Value = serde_json::from_str(reply_body(&reply)).unwrap_or_default();
    let (Some(connection), Some(path)) = (
        started["connection_id"].as_str(),
        started["authorization_url"].as_str().and_then(fixture_path),
    ) else {
        return Ok(Verdict::Broken(format!(
            "the connect's start answered {status}: {}",
            problem(&reply)
        )));
    };
    let mut forward = fixture_forward()?;
    let authorized = fixture_get(path);
    let _ = forward.kill();
    let reply = authorized?;
    let Some((code, state)) =
        location_of(&reply).and_then(|l| authorize_redirect(l, FIXTURE_REDIRECT_URI))
    else {
        return Ok(Verdict::Broken(format!(
            "the fixture's authorize page gave no code: {}",
            reply.lines().next().unwrap_or_default()
        )));
    };
    let callback = format!("/v1/oauth/alpaca/callback?code={code}&state={state}");
    let (status, _) = super::http(API_LOCAL_PORT, "GET", &callback, &[("Cookie", &cookie)], "")?;
    if status != 303 {
        return Ok(Verdict::Broken(format!(
            "the callback answered {status}, not 303"
        )));
    }
    let phase = exchanged(connection)?;
    if phase.as_deref() != Some("Succeeded") {
        return Ok(Verdict::Broken(format!(
            "the exchange pod for {connection} did not succeed: {}",
            phase.as_deref().unwrap_or("the manager started none")
        )));
    }
    let root = root_token()?;
    let seg = segment(connection);
    let taken = vault_read(CODE_READ, &format!("connections/{seg}/code"), &root)?;
    if !code_taken(&taken) {
        return Ok(Verdict::Broken(format!(
            "the code for {connection} is not taken: {}",
            code_state(&taken)
        )));
    }
    let entry = vault_read(TOKEN_READ, &format!("connections/{seg}/token"), &root)?;
    let problems = token_problems(&entry);
    if !problems.is_empty() {
        return Ok(Verdict::Broken(problems.join("; ")));
    }
    eprintln!(
        "    the manager's exchange for {connection} succeeded: code taken, token entry format 1"
    );
    Ok(Verdict::Held)
}

/// The `Idempotency-Key` of the run `id`'s `step`: one per call, as the API requires on each
/// command (workspace API spec API-4).
pub fn key(id: &str, step: &str) -> String {
    format!("exchange-{id}-{step}")
}

/// One `POST` to the API through its forward, as the signed-in owner from the app origin, under
/// the idempotency key `key`.
fn api(cookie: &str, key: &str, path: &str, body: &Value) -> Result<(u16, String)> {
    super::http(
        API_LOCAL_PORT,
        "POST",
        path,
        &[
            ("Cookie", cookie),
            ("Idempotency-Key", key),
            ("Content-Type", "application/json"),
            ("Origin", super::DEMO_APP_ORIGIN),
            ("X-Mandate-Request", "1"),
        ],
        &body.to_string(),
    )
}

/// Forwards [`FIXTURE_LOCAL_PORT`] to the fixture's Service with the host's `kubectl`, and waits
/// until it accepts a connection.
fn fixture_forward() -> Result<Child> {
    let config = kubeconfig()?;
    let mut child = Command::new("kubectl")
        .args([
            "--kubeconfig",
            &config,
            "--namespace",
            NAMESPACE,
            "port-forward",
            "--address",
            "127.0.0.1",
            "service/oauth-fixture",
            &format!("{FIXTURE_LOCAL_PORT}:8091"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("starting `kubectl port-forward` to the oauth fixture")?;
    for _ in 0..30 {
        if TcpStream::connect(("127.0.0.1", FIXTURE_LOCAL_PORT)).is_ok() {
            return Ok(child);
        }
        sleep(Duration::from_secs(1));
    }
    let _ = child.kill();
    bail!("the oauth fixture's forward never accepted a connection")
}

/// `GET path` to the fixture through its forward, with the `Host` it answers alone.
fn fixture_get(path: &str) -> Result<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", FIXTURE_LOCAL_PORT))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {FIXTURE_AUTHORITY}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    status_of(&reply).context("the fixture's reply is not HTTP/1.1")?;
    Ok(reply)
}

/// `Succeeded` once the manager's newest exchange pod for `connection` has succeeded, or the last
/// phase seen after [`POLLS`] polls: a generation that failed may be followed by a newer one.
fn exchanged(connection: &str) -> Result<Option<String>> {
    let selector = format!("{MANAGED_BY},mandate.dev/process=token-exchange");
    let mut phase = None;
    for _ in 0..POLLS {
        let pods = kubectl(&[
            "get",
            "pods",
            "--namespace",
            NAMESPACE,
            "--selector",
            &selector,
            "-o",
            "json",
        ])?;
        phase = exchange_phase(&pods, connection);
        if phase.as_deref() == Some("Succeeded") {
            return Ok(phase);
        }
        sleep(Duration::from_secs(2));
    }
    Ok(phase)
}

/// The cell vault's root token, from the init `cargo xtask cell up` saved.
fn root_token() -> Result<String> {
    let init = std::fs::read_to_string(state()?.join(VAULT_INIT)).context(
        "the exchange drill needs the cell's vault-init.json; run `cargo xtask cell up`",
    )?;
    let init: Value = serde_json::from_str(&init).context("reading the vault's init")?;
    init["root_token"]
        .as_str()
        .map(str::to_owned)
        .context("the vault's init holds no root token")
}

/// `script` in the vault's pod over the KV path `path`, the root token on its stdin.
fn vault_read(script: &str, path: &str, root: &str) -> Result<String> {
    let (_, out) = in_vault_pod(
        &["sh", "-c", script, "sh", path],
        format!("{root}\n").as_bytes(),
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: &str =
        r#"{"access_token":"present","format":1,"scope":["data","trading"],"token_type":"bearer"}"#;

    /// The digest the drill binds is `mandate-connections`' `oauth_connect_digest`: the value
    /// that function gives for `ws_01`, checked against it once when this test was written.
    #[test]
    fn the_connect_digest_is_the_start_route_s() {
        assert_eq!(
            connect_digest("ws_01"),
            "sha256:3df08a75075094b6caf39b686e7262825ead70b36711631621c47a14e32749c2"
        );
    }

    /// The workspace API spec's example challenge, whose first five bytes are `ab cd ef 12 34`.
    #[test]
    fn the_code_is_the_challenge_s_first_40_bits() {
        assert_eq!(
            shown_code("q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA").as_deref(),
            Some("NF6Y-Y4HM")
        );
        assert_eq!(shown_code("q83v"), None);
        assert_eq!(shown_code("q83vEj+WeJ"), None);
    }

    #[test]
    fn a_connection_s_segment_is_its_id_in_hex() {
        assert_eq!(segment("conn_1"), "x636f6e6e5f31");
    }

    #[test]
    fn only_a_format_one_bearer_entry_with_both_scopes_holds() {
        assert!(token_problems(ENTRY).is_empty());
        for broken in [
            ENTRY.replace("\"format\":1", "\"format\":2"),
            ENTRY.replace("bearer", "mac"),
            ENTRY.replace("[\"data\",\"trading\"]", "[\"trading\"]"),
            ENTRY.replace("present", ""),
            ENTRY.replace("}", ",\"refresh_token\":\"r\"}"),
            "not json".to_owned(),
            "[]".to_owned(),
        ] {
            assert!(!token_problems(&broken).is_empty(), "{broken}");
        }
    }

    #[test]
    fn a_code_is_taken_when_tombstoned_or_gone() {
        assert!(code_taken("taken\n"));
        assert!(code_taken("missing"));
        assert!(!code_taken("readable"));
        assert!(!code_taken(""));
    }

    #[test]
    fn the_newest_generation_s_phase_for_the_connection_is_read() {
        let pods = serde_json::json!({"items": [
            {"metadata": {"annotations": {"mandate.dev/subject": "c1"}, "labels": {"mandate.dev/generation": "1"}}, "status": {"phase": "Failed"}},
            {"metadata": {"annotations": {"mandate.dev/subject": "c1"}, "labels": {"mandate.dev/generation": "2"}}, "status": {"phase": "Succeeded"}},
            {"metadata": {"annotations": {"mandate.dev/subject": "c2"}, "labels": {"mandate.dev/generation": "3"}}, "status": {"phase": "Running"}},
        ]})
        .to_string();
        assert_eq!(exchange_phase(&pods, "c1").as_deref(), Some("Succeeded"));
        assert_eq!(exchange_phase(&pods, "c3"), None);
        assert_eq!(exchange_phase("not json", "c1"), None);
    }

    #[test]
    fn the_redirect_gives_its_code_and_state_alone() {
        let uri = FIXTURE_REDIRECT_URI;
        assert_eq!(
            authorize_redirect(&format!("{uri}?code=abc&state=xyz"), uri),
            Some(("abc".to_owned(), "xyz".to_owned()))
        );
        for bad in [
            format!("{uri}?code=abc"),
            format!("{uri}?code=&state=xyz"),
            format!("{uri}?code=a&state=b&extra=c"),
            format!("{uri}?code=a&code=b&state=c"),
            "http://elsewhere/callback?code=a&state=b".to_owned(),
            uri.to_owned(),
        ] {
            assert_eq!(authorize_redirect(&bad, uri), None, "{bad}");
        }
    }

    #[test]
    fn only_the_fixture_s_authorize_page_is_followed() {
        assert_eq!(
            fixture_path("http://oauth-fixture:8091/alpaca/oauth/authorize?a=b"),
            Some("/alpaca/oauth/authorize?a=b")
        );
        for other in [
            "https://paper-api.alpaca.markets/oauth/authorize?a=b",
            "https://oauth-fixture:8091/alpaca/oauth/authorize?a=b",
            "http://oauth-fixture:8092/alpaca/oauth/authorize?a=b",
            "http://oauth-fixture:8091/alpaca/oauth/token?a=b",
        ] {
            assert_eq!(fixture_path(other), None, "{other}");
        }
    }

    /// Each call's key is its own and is the API's shape: one segment of 16 to 64 characters.
    #[test]
    fn each_call_has_a_key_of_the_api_s_shape() {
        for step in ["challenge", "start"] {
            let key = key("1234567890", step);
            assert!((16..=64).contains(&key.len()), "{key}");
            assert!(key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
        }
        assert_ne!(key("1", "challenge"), key("1", "start"));
    }

    /// A failure report names the problem's code and never the reply's body.
    #[test]
    fn a_failure_report_names_the_problem_code_alone() {
        let refused =
            "HTTP/1.1 403 Forbidden\r\n\r\n{\"code\":\"step_up_missing\",\"detail\":\"x\"}";
        assert_eq!(problem(refused), "step_up_missing");
        let issued = "HTTP/1.1 201 Created\r\n\r\n{\"webauthn_challenge\":\"q83vEjRW\"}";
        assert_eq!(problem(issued), "no problem code");
        assert_eq!(code_state("readable\n"), "readable");
        assert_eq!(code_state("{\"code\":\"secret\"}"), "unreadable");
    }

    #[test]
    fn the_location_header_is_read_whatever_its_case() {
        let reply = "HTTP/1.1 303 See Other\r\nlocation: http://x/y?code=c\r\n\r\n";
        assert_eq!(location_of(reply), Some("http://x/y?code=c"));
        assert_eq!(location_of("HTTP/1.1 200 OK\r\n\r\n"), None);
    }

    /// The confirmation runs as the founder, the principal the stub sign-in's subject links to,
    /// and the code is passed as shown.
    #[test]
    fn the_confirmation_is_the_founder_s() {
        let gesture = confirm_connect("ws", "01CH", "ABCD-EFGH");
        assert!(
            gesture
                .args
                .windows(2)
                .any(|w| w == ["--principal", FOUNDER])
        );
        assert!(
            gesture
                .args
                .windows(2)
                .any(|w| w == ["--code", "ABCD-EFGH"])
        );
        assert_eq!(gesture.expect, "confirmed 01CH as event");
    }
}
