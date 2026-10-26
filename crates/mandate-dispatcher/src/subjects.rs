//! The streams whose alerts and approval requests are the dispatcher's causes (DEC-2516 item 1,
//! DEC-2500 item 7): the configured streams, a floor never a filter, and every stream the
//! workspace's control stream names. The workspace's own control stream; the agent stream of every
//! agent it ever deployed; the account stream of every account a connection was requested for
//! (rule 131: its executor opens the stream at the request, before the connection is established,
//! and may alert on it even if it never is) or established for. An agent deployed or a connection
//! requested after the configuration was written is followed from the next step on, so its alerts
//! are never missed. A stopped agent stays followed, since its stream
//! may still raise an alert.

use std::collections::BTreeSet;

use mandate_canon::{Value, parse};
use mandate_journal::{Environment, StoredEvent, StreamId};

/// Every subject stream: `floor`, the workspace's control stream, and each agent and account
/// stream `control`'s rows in `environment` name. A row that does not read names nothing, and is
/// never a reason to drop a stream already known (`AGENTS.md` rule 3).
#[must_use]
pub fn subjects(
    workspace: &str,
    control: &[StoredEvent],
    environment: Environment,
    floor: &[StreamId],
) -> Vec<StreamId> {
    let mut named: BTreeSet<String> = floor.iter().map(|s| s.as_str().to_owned()).collect();
    named.insert(format!("ctl:{workspace}"));
    for row in control
        .iter()
        .filter(|row| row.environment == environment.as_str())
    {
        let Some(payload) = parse(&row.body)
            .ok()
            .and_then(|body| body.get("payload").cloned())
        else {
            continue;
        };
        let member = |name: &str| payload.get(name).and_then(Value::as_str).map(str::to_owned);
        let stream = match row.event_type.as_str() {
            "AgentDeployed" => member("agent_id").map(|agent| format!("agent:{workspace}:{agent}")),
            "ConnectionRequested" | "ConnectionEstablished" => {
                member("account_ref").map(|account| format!("acct:{workspace}:{account}"))
            }
            _ => None,
        };
        named.extend(stream);
    }
    named
        .into_iter()
        .filter_map(|id| StreamId::parse(&id))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mandate_canon::Digest;
    use mandate_journal::{Environment, StoredEvent, StreamId};
    use proptest::prelude::*;

    use super::subjects;

    const WS: &str = "ws1";

    /// One control-stream row the generator lays down, and what it should make the dispatcher
    /// follow, written out by the generator rather than read back from the row.
    #[derive(Debug, Clone)]
    enum Laid {
        Deployed(String),
        Established(String),
        Requested(String),
        Other,
        Unreadable,
        Live(String),
    }

    fn row(seq: u64, event_type: &str, environment: &str, payload: &str) -> StoredEvent {
        let body = format!(
            r#"{{"event_type":"{event_type}","payload":{payload},"environment":"{environment}"}}"#
        );
        StoredEvent {
            stream_id: format!("ctl:{WS}"),
            seq,
            event_id: format!("01J8Z3M{seq:019}"),
            event_type: event_type.to_owned(),
            schema_version: 2,
            environment: environment.to_owned(),
            recorded_at: "2026-10-26T10:00:00.000000000Z".to_owned(),
            prev_hash: Digest::of(b"prev"),
            hash: Digest::of(body.as_bytes()),
            body: body.into_bytes(),
        }
    }

    fn laid_row(seq: u64, laid: &Laid) -> StoredEvent {
        match laid {
            Laid::Deployed(agent) => row(
                seq,
                "AgentDeployed",
                "paper",
                &format!(r#"{{"agent_id":"{agent}","connection_id":"c1"}}"#),
            ),
            Laid::Established(account) => row(
                seq,
                "ConnectionEstablished",
                "paper",
                &format!(r#"{{"connection_id":"c1","account_ref":"{account}"}}"#),
            ),
            Laid::Requested(account) => row(
                seq,
                "ConnectionRequested",
                "paper",
                &format!(r#"{{"connection_id":"c2","account_ref":"{account}","broker":"alpaca"}}"#),
            ),
            Laid::Other => row(seq, "OwnerCommandIssued", "paper", r#"{"command":"pause"}"#),
            Laid::Unreadable => {
                let mut broken = row(seq, "AgentDeployed", "paper", "{}");
                broken.body = b"not json".to_vec();
                broken
            }
            Laid::Live(agent) => row(
                seq,
                "AgentDeployed",
                "live",
                &format!(r#"{{"agent_id":"{agent}"}}"#),
            ),
        }
    }

    fn named(streams: &[StreamId]) -> BTreeSet<String> {
        streams.iter().map(|s| s.as_str().to_owned()).collect()
    }

    #[test]
    fn the_control_stream_names_every_agent_and_account_stream_beside_the_floor() {
        let control = [
            laid_row(1, &Laid::Established("ACCT1".to_owned())),
            laid_row(2, &Laid::Deployed("agent-a".to_owned())),
            laid_row(3, &Laid::Other),
            laid_row(4, &Laid::Deployed("agent-b".to_owned())),
            laid_row(5, &Laid::Requested("ACCT2".to_owned())),
        ];
        let floor: Vec<StreamId> = StreamId::parse("agent:ws1:agent-z").into_iter().collect();
        let got = named(&subjects(WS, &control, Environment::Paper, &floor));
        let want: BTreeSet<String> = [
            "ctl:ws1",
            "agent:ws1:agent-a",
            "agent:ws1:agent-b",
            "agent:ws1:agent-z",
            "acct:ws1:ACCT1",
            "acct:ws1:ACCT2",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(got, want);
    }

    fn laid() -> impl Strategy<Value = Laid> {
        let name = prop_oneof![Just("a"), Just("b"), Just("c")].prop_map(str::to_owned);
        prop_oneof![
            name.clone().prop_map(Laid::Deployed),
            name.clone()
                .prop_map(|n| Laid::Established(n.to_uppercase())),
            name.clone()
                .prop_map(|n| Laid::Requested(format!("R{}", n.to_uppercase()))),
            Just(Laid::Other),
            Just(Laid::Unreadable),
            name.prop_map(Laid::Live),
        ]
    }

    proptest! {
        /// Whatever the control stream holds: every agent deployed and every account requested or
        /// established in the dispatcher's environment is followed, with the control stream and the floor,
        /// and nothing else is. The oracle is the list the generator laid down, not the rows.
        #[test]
        fn exactly_the_named_streams_and_the_floor_are_followed(
            rows in proptest::collection::vec(laid(), 0..12),
            floored in any::<bool>(),
        ) {
            let control: Vec<StoredEvent> = rows
                .iter()
                .zip(1u64..)
                .map(|(laid, seq)| laid_row(seq, laid))
                .collect();
            let floor: Vec<StreamId> = if floored {
                StreamId::parse("acct:ws1:FLOOR").into_iter().collect()
            } else {
                Vec::new()
            };
            let mut want: BTreeSet<String> = BTreeSet::from(["ctl:ws1".to_owned()]);
            if floored {
                want.insert("acct:ws1:FLOOR".to_owned());
            }
            for laid in &rows {
                match laid {
                    Laid::Deployed(agent) => {
                        want.insert(format!("agent:ws1:{agent}"));
                    }
                    Laid::Established(account) | Laid::Requested(account) => {
                        want.insert(format!("acct:ws1:{account}"));
                    }
                    Laid::Other | Laid::Unreadable | Laid::Live(_) => {}
                }
            }
            prop_assert_eq!(named(&subjects(WS, &control, Environment::Paper, &floor)), want);
        }
    }
}
