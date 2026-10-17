//! Which transport a research call goes through (DEC-997 item 4, DEC-1120 item 5): the recorded
//! fake unless the vault holds the upstream's key and a hosted transport is supplied, and a
//! selection that says which and why without naming the key.

mod common;

use common::{
    FIXTURE_KEY, HostedDouble, REPORTED_PROVIDER, SLUG, UPSTREAM, parts, recorded_gateway, reply,
};
use mandate_canon::{Value, parse};
use mandate_gateway::fake::{Recorded, RecordedTransport};
use mandate_gateway::meter::Caps;
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

#[test]
fn the_default_gateway_calls_through_the_fake_with_the_placeholder_key() {
    let (mut gateway, selection) = recorded_gateway(vec![reply("{}")]);
    assert_eq!(selection.backend, Backend::RecordedFake);
    assert!(gateway.transport().recorded().is_some());
    assert!(gateway.transport_mut().recorded_mut().is_some());
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
    hosted.inner.script(vec![Recorded {
        latency_ms: 10,
        delivery: Delivery::Response {
            status: 200,
            body: reply("{}"),
            retry_after_ms: None,
        },
    }]);
    let mut fake = RecordedTransport::new();
    fake.script(vec![Recorded {
        latency_ms: 10,
        delivery: Delivery::ConnectFailed,
    }]);
    let (gateway, selection) = research_gateway(
        parts(Caps::demo()),
        Box::new(common::FixtureVault),
        Some(hosted),
        fake,
        UPSTREAM,
    );
    assert_eq!(selection.backend, Backend::OpenRouter);
    assert!(gateway.transport().recorded().is_none());
    assert!(matches!(gateway.transport(), ResearchTransport::Hosted(_)));
}

#[test]
fn a_hosted_transport_without_a_vaulted_key_is_never_used() {
    let (gateway, selection) = research_gateway(
        parts(Caps::demo()),
        Box::new(NoKeys),
        Some(HostedDouble::default()),
        RecordedTransport::new(),
        UPSTREAM,
    );
    assert_eq!(selection.backend, Backend::RecordedFake);
    assert!(gateway.transport().recorded().is_some());
    let (_, selection) = research_gateway(
        parts(Caps::demo()),
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
