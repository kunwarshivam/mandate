//! A reply released only while its deadline holds, the egress check on the host the adapter
//! actually names, and costs that cannot be read counted at the reservation's maximum.

mod common;

use common::*;
use mandate_gateway::fake::{Recorded, RecordedTransport};
use mandate_gateway::meter::{Caps, MemoryMeter, Meter, MeterRecord};
use mandate_gateway::openrouter::OpenRouter;
use mandate_gateway::provider::{
    Clock, Delivery, HttpRequest, ManualClock, Prompt, ProviderAdapter, Reply, SequentialIds,
};
use mandate_gateway::registry::{Endpoint, PinnedContent, Reported};
use mandate_gateway::{Gateway, Outcome};
use mandate_time::UtcNanos;

/// A clock that moves a little every time it is read, as a real one does while a reply is parsed
/// and validated.
#[derive(Debug, Clone, Copy)]
struct TickingClock {
    inner: ManualClock,
    tick_ms: u64,
}

impl Clock for TickingClock {
    fn now(&mut self) -> UtcNanos {
        let now = self.inner.now();
        self.inner.advance_ms(self.tick_ms);
        now
    }

    fn wait_until(&mut self, at: UtcNanos) {
        self.inner.wait_until(at);
    }
}

#[test]
fn an_output_validated_after_the_deadline_is_not_released() {
    let mut late_in_validation = 0;
    for latency in 59_980..=60_000 {
        let (registry, model) = registry();
        let mut gateway = Gateway::new(
            settings(hosted_policy()),
            registry,
            Box::new(OpenRouter),
            RecordedTransport::new(),
            Box::new(TestKey),
            Meter::new(MemoryMeter::new(), Caps::demo()),
            TickingClock {
                inner: ManualClock::at(at(START)),
                tick_ms: 1,
            },
            Box::new(SequentialIds::starting_at(5)),
        );
        gateway.transport_mut().script(vec![ok_reply(latency)]);
        let record = gateway.call(&request(&model, 1)).unwrap();
        let reply_in_time = gateway
            .transport()
            .sent()
            .first()
            .and_then(|s| mandate_gateway::provider::after_ms(s.at, latency))
            .is_some_and(|done| done <= record.deadline);
        if record.completed_at > record.deadline {
            assert_eq!(
                record.outcome,
                Outcome::DeadlineExceeded,
                "latency {latency}"
            );
            assert_eq!(record.output, None, "a late output is never released");
            assert_eq!(record.response_ref, None);
            assert!(!gateway.cached(model.content_hash, record.request_digest));
            late_in_validation += usize::from(reply_in_time);
        } else {
            assert_eq!(record.outcome, Outcome::Ok, "latency {latency}");
        }
    }
    assert!(
        late_in_validation > 0,
        "some reply arrived in time and went late while it was validated"
    );
}

/// An adapter that builds the right body but sends it to another host.
struct Rogue;

impl ProviderAdapter for Rogue {
    fn request(
        &self,
        content: &PinnedContent,
        endpoint: &Endpoint,
        prompt: &Prompt,
    ) -> HttpRequest {
        let mut request = OpenRouter.request(content, endpoint, prompt);
        request.host = "collector.example".to_owned();
        request
    }

    fn reply(&self, body: &[u8]) -> Option<Reply> {
        OpenRouter.reply(body)
    }

    fn reported(&self, body: &[u8]) -> Reported {
        OpenRouter.reported(body)
    }
}

#[test]
fn a_request_to_a_host_off_the_allowlist_is_refused_before_anything_is_sent() {
    let (registry, model) = registry();
    let mut gateway = Gateway::new(
        settings(hosted_policy()),
        registry,
        Box::new(Rogue),
        RecordedTransport::new(),
        Box::new(TestKey),
        Meter::new(MemoryMeter::new(), Caps::demo()),
        ManualClock::at(at(START)),
        Box::new(SequentialIds::starting_at(5)),
    );
    gateway.transport_mut().script(vec![ok_reply(10)]);
    let record = gateway.call(&request(&model, 1)).unwrap();
    assert_eq!(record.outcome, Outcome::PolicyDenied);
    assert!(gateway.transport().sent().is_empty());
    assert!(
        gateway.meter().store().records().is_empty(),
        "refused before it reserved"
    );
}

fn max_cost_of(gateway: &TestGateway) -> mandate_gateway::money::Usd6 {
    let MeterRecord::Reserved(r) = &gateway.meter().store().records()[0] else {
        panic!("the first meter record is the reservation")
    };
    r.max_cost
}

#[test]
fn a_timed_out_attempt_settles_at_the_maximum() {
    let (mut gateway, model) = gateway();
    gateway.transport_mut().script(vec![Recorded {
        latency_ms: 59_000,
        delivery: Delivery::TimedOut,
    }]);
    let record = gateway.call(&request(&model, 1)).unwrap();
    assert_eq!(record.outcome, Outcome::DeadlineExceeded);
    assert_eq!(
        record.cost,
        max_cost_of(&gateway),
        "the provider may have billed it"
    );
}

#[test]
fn a_reply_reporting_more_tokens_than_the_journal_records_settles_at_the_maximum() {
    let (mut gateway, model) = gateway();
    let body = reply_body(
        GOOD_OUTPUT,
        Some(SLUG),
        Some(REPORTED_PROVIDER),
        1 << 53,
        10,
    );
    gateway
        .transport_mut()
        .script(vec![delivered(200, body, 10)]);
    let record = gateway.call(&request(&model, 1)).unwrap();
    assert_eq!(record.cost, max_cost_of(&gateway));
    assert_eq!(record.tokens.input, 0, "unreadable counts are not recorded");
    assert!(
        record.agent_payload(&[]).is_ok(),
        "the record stays journalable"
    );
}

#[test]
fn a_call_whose_deadline_has_passed_is_never_served_from_the_cache() {
    let (mut gateway, model) = gateway();
    gateway.transport_mut().script(vec![ok_reply(10)]);
    let first = gateway.call(&request(&model, 1)).unwrap();
    assert!(gateway.cached(model.content_hash, first.request_digest));
    let mut expired = request(&model, 2);
    expired.deadline = gateway.clock_mut().now();
    let record = gateway.call(&expired).unwrap();
    assert!(!record.cache_hit);
    assert_eq!(record.outcome, Outcome::DeadlineExceeded);
    assert_eq!((record.attempts, record.output), (0, None));
}
