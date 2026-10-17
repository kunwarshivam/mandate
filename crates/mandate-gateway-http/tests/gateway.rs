//! The gateway end to end over the HTTPS transport (DEC-2200): a pinned reply comes back `ok`
//! through a real TLS connection, the request on the wire is locked to one upstream with the key
//! only in its header, and a reply that reports another model or provider is still refused and
//! its endpoint benched (INF-2; the pinning checks stay the gateway's, whatever the transport).

mod common;

use common::fixtures::{REPORTED_PROVIDER, SLUG, TEST_KEY, gateway, reply, request};
use common::server::{Reply, pki, serve};
use common::{PATH, trusting};
use mandate_gateway::Outcome;

#[test]
fn a_pinned_reply_over_https_is_ok_and_the_wire_request_is_locked_to_one_upstream() {
    let pki = pki();
    let server = serve(&pki, vec![Reply::ok(&reply(SLUG, REPORTED_PROVIDER))]);
    let host = format!("localhost:{}", server.port);
    let (mut gateway, model) = gateway(&host, trusting(&pki));
    let record = gateway.call(&request(&model, 1)).unwrap();
    assert_eq!(record.outcome, Outcome::Ok, "{record:?}");
    assert!(record.output.is_some());
    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].path, PATH);
    assert_eq!(
        seen[0].header("authorization"),
        Some(format!("Bearer {TEST_KEY}").as_str())
    );
    let body: serde_json::Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(body["provider"]["only"], serde_json::json!(["anthropic"]));
    assert_eq!(
        body["provider"]["allow_fallbacks"],
        serde_json::json!(false)
    );
    assert_eq!(body["model"], serde_json::json!(SLUG));
    assert!(
        !String::from_utf8_lossy(&seen[0].body).contains(TEST_KEY),
        "the key never goes in the body"
    );
}

#[test]
fn a_substituted_model_over_https_is_identity_mismatch_and_benches_the_endpoint() {
    let pki = pki();
    let server = serve(
        &pki,
        vec![Reply::ok(&reply(
            "anthropic/claude-haiku-4.5",
            REPORTED_PROVIDER,
        ))],
    );
    let host = format!("localhost:{}", server.port);
    let (mut gateway, model) = gateway(&host, trusting(&pki));
    let record = gateway.call(&request(&model, 1)).unwrap();
    assert_eq!(record.outcome, Outcome::IdentityMismatch);
    assert_eq!(record.output, None);
    let next = gateway.call(&request(&model, 2)).unwrap();
    assert_eq!(
        next.outcome,
        Outcome::PolicyDenied,
        "the benched endpoint is not called again"
    );
    assert_eq!(server.seen().len(), 1);
}
