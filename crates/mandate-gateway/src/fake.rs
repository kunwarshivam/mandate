//! The recorded-response transport: the default everywhere except a real cell with a vaulted key
//! (DEC-997 item 4). It replays recorded deliveries and opens no connection. It logs every
//! request it is handed, so a test can show what reached the wire and when.

use std::collections::{BTreeMap, VecDeque};

use mandate_canon::Digest;
use mandate_time::UtcNanos;

use crate::provider::{Clock, Delivery, HttpRequest, ProviderKey, Transport, after_ms};

/// One recorded delivery and how long it took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub latency_ms: u64,
    pub delivery: Delivery,
}

/// One request the transport was handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub host: String,
    pub body_digest: Digest,
    pub at: UtcNanos,
    pub deadline: UtcNanos,
    pub with_key: bool,
}

/// Replays recorded deliveries: first those recorded for the exact request body, then the
/// scripted queue in order. With neither, the attempt fails to connect.
#[derive(Debug, Clone, Default)]
pub struct RecordedTransport {
    by_request: BTreeMap<Digest, VecDeque<Recorded>>,
    script: VecDeque<Recorded>,
    sent: Vec<Sent>,
}

impl RecordedTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Deliveries for a request whose body has this digest, in order.
    pub fn record(&mut self, body_digest: Digest, deliveries: Vec<Recorded>) {
        self.by_request
            .entry(body_digest)
            .or_default()
            .extend(deliveries);
    }

    /// Deliveries for any request, in order, after the recorded ones.
    pub fn script(&mut self, deliveries: Vec<Recorded>) {
        self.script.extend(deliveries);
    }

    /// Drops every scripted delivery not yet replayed.
    pub fn clear_script(&mut self) {
        self.script.clear();
    }

    /// Every request handed to the transport, in order.
    pub fn sent(&self) -> &[Sent] {
        &self.sent
    }
}

impl Transport for RecordedTransport {
    fn send(
        &mut self,
        request: &HttpRequest,
        key: Option<&ProviderKey>,
        deadline: UtcNanos,
        clock: &mut dyn Clock,
    ) -> Delivery {
        let body_digest = Digest::of(&request.body);
        self.sent.push(Sent {
            host: request.host.clone(),
            body_digest,
            at: clock.now(),
            deadline,
            with_key: key.is_some(),
        });
        let next = self
            .by_request
            .get_mut(&body_digest)
            .and_then(VecDeque::pop_front)
            .or_else(|| self.script.pop_front());
        let Some(recorded) = next else {
            return Delivery::ConnectFailed;
        };
        let now = clock.now();
        if let Some(done) = after_ms(now, recorded.latency_ms) {
            clock.wait_until(done);
        }
        recorded.delivery
    }
}
