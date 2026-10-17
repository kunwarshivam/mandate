//! Which transport a research call goes through (DEC-997 item 4, DEC-1120 item 5): the recorded
//! fake unless the vault holds the upstream's key and a hosted transport is supplied, and a
//! selection that says which and why without naming the key.

mod common;

use common::{
    FIXTURE_KEY, HostedDouble, REPORTED_PROVIDER, SLUG, UPSTREAM, parts, recorded_gateway, reply,
};
use mandate_canon::{Value, parse};
use mandate_gateway::fake::{Recorded, RecordedTransport};
use mandate_gateway::money::Usage;
use mandate_gateway::openrouter::OpenRouter;
use mandate_gateway::provider::{Delivery, NoKeys, ProviderAdapter, SecretError, SecretSource};
use mandate_research_run::{
    Backend, NoHostedTransport, RECORDED_KEY, Reason, RecordedKey, ResearchTransport,
    recorded_reply, research_gateway, select,
};

struct Unreachable;

impl SecretSource for Unreachable {
    fn provider_key(
        &self,
        _upstream: &str,
    ) -> Result<mandate_gateway::provider::ProviderKey, SecretError> {
        Err(SecretError::Unavailable)
    }
}

#[test]
fn no_vaulted_key_selects_the_recorded_fake() {
    for vault in [&NoKeys as &dyn SecretSource, &Unreachable] {
        for hosted in [false, true] {
            let selection = select(vault, UPSTREAM, hosted);
            assert_eq!(selection.backend, Backend::RecordedFake);
            assert_eq!(selection.reason, Reason::NoVaultedKey);
            assert_eq!(selection.upstream, UPSTREAM);
        }
    }
    let other = select(&common::FixtureVault, "openai", true);
    assert_eq!(
        (other.backend, other.reason),
        (Backend::RecordedFake, Reason::NoVaultedKey),
        "a key for another upstream is no key for this one"
    );
}

#[test]
fn a_vaulted_key_without_a_hosted_transport_still_selects_the_fake() {
    let selection = select(&common::FixtureVault, UPSTREAM, false);
    assert_eq!(selection.backend, Backend::RecordedFake);
    assert_eq!(selection.reason, Reason::NoHostedTransport);
}

#[test]
fn a_vaulted_key_and_a_hosted_transport_select_openrouter() {
    let selection = select(&common::FixtureVault, UPSTREAM, true);
    assert_eq!(selection.backend, Backend::OpenRouter);
    assert_eq!(selection.reason, Reason::VaultedKey);
}

#[test]
fn the_selection_says_which_and_why_and_never_names_the_key() {
    let lines = [
        select(&NoKeys, UPSTREAM, true).describe(),
        select(&common::FixtureVault, UPSTREAM, false).describe(),
        select(&common::FixtureVault, UPSTREAM, true).describe(),
    ];
    assert!(lines[0].contains("recorded fake") && lines[0].contains("no vaulted"));
    assert!(lines[1].contains("recorded fake") && lines[1].contains("no hosted transport"));
    assert!(lines[2].contains("OpenRouter") && lines[2].contains(UPSTREAM));
    assert!(!lines[2].contains("recorded"));
    for line in &lines {
        assert!(!line.contains(FIXTURE_KEY), "{line}");
        assert!(!line.contains("fixture-key"), "{line}");
    }
    assert_ne!(lines[0], lines[1]);
}

fn request() -> mandate_gateway::CallRequest {
    mandate_gateway::CallRequest {
        call_id: mandate_canon::encode_ulid((1 << 100) + 9),
        workspace_id: common::WORKSPACE.to_owned(),
        agent_id: Some(common::AGENT.to_owned()),
        purpose: mandate_gateway::Purpose::Research,
        model: common::registry().1,
        inputs: ["context", "items", "memory"]
            .into_iter()
            .map(|slot| (slot.to_owned(), "{}".to_owned()))
            .collect(),
        as_of: common::at(common::CUT_OFF),
        deadline: common::at(common::CUT_OFF + 120),
        observations: Vec::new(),
        key_owner: mandate_gateway::KeyOwner::Platform,
        run_kind: None,
        turns: Vec::new(),
        tools: Vec::new(),
        spend_ceiling: None,
    }
}

fn ok_reply() -> Recorded {
    Recorded {
        latency_ms: 10,
        delivery: Delivery::Response {
            status: 200,
            body: reply(r#"{"invalidations":[],"theses":[]}"#),
            retry_after_ms: None,
        },
    }
}

#[test]
fn the_default_gateway_calls_through_the_fake_with_the_placeholder_key() {
    let (mut gateway, selection) =
        recorded_gateway(vec![reply(r#"{"invalidations":[],"theses":[]}"#)]);
    assert_eq!(selection.backend, Backend::RecordedFake);
    let record = gateway.call(&request()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        record.outcome,
        mandate_gateway::Outcome::Ok,
        "not credential_invalid"
    );
    let sent = gateway
        .transport()
        .recorded()
        .unwrap_or_else(|| panic!("the fake"))
        .sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].with_key, "the placeholder stands where a key would");
    assert_eq!(
        RecordedKey
            .provider_key(UPSTREAM)
            .map(|k| k.expose_secret().to_owned()),
        Ok(RECORDED_KEY.to_owned())
    );
    assert_eq!(
        RecordedKey
            .provider_key("anything")
            .map(|k| k.expose_secret().to_owned()),
        Ok(RECORDED_KEY.to_owned())
    );
}

#[test]
fn a_selected_openrouter_gateway_sends_through_the_hosted_transport_with_the_vault_s_key() {
    let mut hosted = HostedDouble::default();
    hosted.inner.script(vec![ok_reply()]);
    let mut fake = RecordedTransport::new();
    fake.script(vec![ok_reply()]);
    let (mut gateway, selection) = research_gateway(
        parts(common::budget()),
        Box::new(common::FixtureVault),
        Some(hosted),
        fake,
        UPSTREAM,
    );
    assert_eq!(selection.backend, Backend::OpenRouter);
    let record = gateway.call(&request()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(record.outcome, mandate_gateway::Outcome::Ok);
    assert!(gateway.transport().recorded().is_none());
    let ResearchTransport::Hosted(hosted) = gateway.transport() else {
        panic!("the hosted transport");
    };
    assert_eq!(hosted.inner.sent().len(), 1, "the call went out hosted");
    assert_eq!(
        hosted.keys,
        vec![FIXTURE_KEY.to_owned()],
        "with the vault's key, never the placeholder"
    );
}

#[test]
fn a_hosted_transport_without_a_vaulted_key_is_never_used() {
    let mut hosted = HostedDouble::default();
    hosted.inner.script(vec![ok_reply()]);
    let mut fake = RecordedTransport::new();
    fake.script(vec![ok_reply()]);
    let (mut gateway, selection) = research_gateway(
        parts(common::budget()),
        Box::new(NoKeys),
        Some(hosted),
        fake,
        UPSTREAM,
    );
    assert_eq!(selection.backend, Backend::RecordedFake);
    let record = gateway.call(&request()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(record.outcome, mandate_gateway::Outcome::Ok);
    assert_eq!(
        gateway.transport().recorded().map(|fake| fake.sent().len()),
        Some(1),
        "the call went to the fake"
    );
    let (_, selection) = research_gateway(
        parts(common::budget()),
        Box::new(common::FixtureVault),
        None::<NoHostedTransport>,
        RecordedTransport::new(),
        UPSTREAM,
    );
    assert_eq!(selection.reason, Reason::NoHostedTransport);
}

#[test]
fn a_recorded_reply_reads_back_through_the_openrouter_adapter() {
    let usage = Usage {
        input: 12,
        output: 34,
        cached: 0,
    };
    let body = recorded_reply(r#"{"theses":[]}"#, SLUG, REPORTED_PROVIDER, usage);
    let read = OpenRouter
        .reply(&body)
        .unwrap_or_else(|| panic!("the adapter reads a recorded reply"));
    assert_eq!(read.content.as_deref(), Some(r#"{"theses":[]}"#));
    assert_eq!(read.usage, usage);
    assert!(!read.refused);
    let reported = OpenRouter.reported(&body);
    assert_eq!(reported.model.as_deref(), Some(SLUG));
    assert_eq!(reported.provider.as_deref(), Some(REPORTED_PROVIDER));
    let value: Value = parse(&body).unwrap_or_else(|e| panic!("canonical JSON: {e:?}"));
    assert!(value.get("id").and_then(Value::as_str).is_some());
}

#[test]
fn a_reply_scripted_through_the_gateway_s_own_fake_is_the_one_it_replays() {
    let (mut gateway, _) = recorded_gateway(Vec::new());
    let fake = gateway
        .transport_mut()
        .recorded_mut()
        .unwrap_or_else(|| panic!("the default transport is the fake"));
    fake.script(vec![Recorded {
        latency_ms: 10,
        delivery: Delivery::Response {
            status: 200,
            body: reply(r#"{"invalidations":[],"theses":[]}"#),
            retry_after_ms: None,
        },
    }]);
    let request = request();
    let record = gateway.call(&request).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        record.outcome,
        mandate_gateway::Outcome::Ok,
        "the reply scripted through recorded_mut is the one the gateway read"
    );
    let sent = gateway
        .transport()
        .recorded()
        .map_or(0, |fake| fake.sent().len());
    assert_eq!(
        usize::try_from(record.attempts).unwrap_or(0),
        sent,
        "every attempt reached the fake the script was written to"
    );
    assert_eq!(
        gateway
            .transport_mut()
            .recorded_mut()
            .map(|fake| fake.sent().len()),
        Some(sent)
    );
}
