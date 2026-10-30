//! `POST /v1/workspaces/{ws}/agents/{agent}/messages/{message_id}/confirm` (workspace API §4.6.2's
//! *Confirm an order*, [DEC-2859](../../../docs/project/decisions/DEC-2859.md) item 8): the owner's
//! tap on the chat's `confirm_order` card, which alone submits the buy the owner asked for.
//!
//! The card is the platform's own record: the route reads its instrument and quantity from the
//! journaled `ChatMessagePosted`, never from the request, so the tap submits exactly what the owner
//! was shown. The member must be the one whose thread holds the card; then the *Make an owner
//! request* row authorizes the tap afresh and the owner-request route's own checks run (§3.11 item
//! 13), so a session signed out, a stopped agent, a live environment or an instrument no longer
//! pinned refuses it with nothing committed. The `OwnerRequestSubmitted` names the card's message
//! as its `causation_id`, under §3.4's id for the key `confirm-{message_id}`, so a second tap,
//! whatever key it carries, replays the first and submits nothing more.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_api::idempotency::IdempotencyKey;
use mandate_api::problem::ProblemCode;
use mandate_authn::Request;
use mandate_canon::Value;
use mandate_identity::Permission;
use mandate_journal::StoredEvent;

use crate::backtests::paper;
use crate::chat_run::Thread;
use crate::http::{App, problem, problem_body};
use crate::owner_requests::{live_deployment, positive, submit};
use crate::reads::envelope;
use crate::routes::{Refused, Unreadable, authorized, path_refused};
use crate::writes::{Gate, principal_text, strictly};

/// The card kind the chat's `order.buy` posts (agent harness spec §15.13).
pub const CONFIRM_ORDER: &str = "confirm_order";

/// The route: the session and the strict empty body, the chat row, the card in the member's own
/// thread, then the owner-request row and its checks, and the commit under the card's own key.
pub async fn confirm_order(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String)>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path((workspace, agent, message))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, ()) = match strictly(&app, &headers, body, |_| Ok(())).await {
        Ok(admitted) => admitted,
        Err(refusal) => return *refusal,
    };
    let reader = match authorized(
        &app,
        &gate.admitted,
        gate.now,
        &workspace,
        Permission::ChatThread,
        Unreadable::Command,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let thread = Thread::agent(&agent, &principal_text(&gate).unwrap_or_default());
    let Some((instrument, quantity)) = confirmable(&reader.control, &thread, &message) else {
        return Refused::NotFound.response();
    };
    let reader = match authorized(
        &app,
        &gate.admitted,
        gate.now,
        &workspace,
        Permission::OwnerRequest,
        Unreadable::Command,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(deployment) = live_deployment(&reader.control, &agent) else {
        return Refused::NotFound.response();
    };
    if !paper(&reader.control) {
        return problem(
            StatusCode::CONFLICT,
            problem_body(ProblemCode::LiveUnavailable),
        );
    }
    let Some(gate) = card_keyed(gate, &message) else {
        return Refused::NotFound.response();
    };
    submit(
        &app,
        &gate,
        &reader,
        (deployment, instrument, quantity),
        Some(&message),
    )
    .await
}

/// `gate` under the card's own idempotency key, `confirm-{message}`, so every tap of one card
/// derives one event id; `None` for a message id no key can hold.
fn card_keyed(gate: Gate, message: &str) -> Option<Gate> {
    let key = IdempotencyKey::parse(&format!("confirm-{message}")).ok()?;
    Some(Gate { key, ..gate })
}

/// The instrument and the positive quantity of the `confirm_order` card the agent posted as
/// `message` in `thread`, or `None`: no such message, another member's or agent's, the owner's own,
/// a message with another card or none, or a card whose members do not read.
pub(crate) fn confirmable(
    control: &[StoredEvent],
    thread: &Thread,
    message: &str,
) -> Option<(String, String)> {
    let row = control
        .iter()
        .find(|row| row.event_id == message && row.event_type == "ChatMessagePosted")?;
    let payload = envelope(row)?.get("payload").cloned()?;
    if !thread.holds(Some(&payload))
        || payload.get("author").and_then(Value::as_str) != Some("agent")
    {
        return None;
    }
    let card = payload.get("card")?;
    let text = |name: &str| card.get(name).and_then(Value::as_str);
    if text("kind") != Some(CONFIRM_ORDER) || text("side") != Some("buy") {
        return None;
    }
    let quantity = text("qty").filter(|q| positive(q))?;
    Some((text("instrument_id")?.to_owned(), quantity.to_owned()))
}

#[cfg(test)]
mod tests {
    use mandate_canon::{Digest, parse, to_canonical};
    use mandate_journal::StoredEvent;

    use super::confirmable;
    use crate::chat_run::Thread;

    const CARD: &str = r#"{"kind":"confirm_order","side":"buy","qty":"2","instrument_id":"i-1"}"#;

    fn posted(id: &str, event_type: &str, author: &str, member: &str, card: &str) -> StoredEvent {
        let text = format!(
            r#"{{"event_type":"{event_type}","payload":{{"agent_id":"agt","setup_id":null,"member":"{member}","author":"{author}","card":{card}}}}}"#
        );
        let body = parse(text.as_bytes())
            .map(|v| to_canonical(&v))
            .unwrap_or_default();
        StoredEvent {
            stream_id: "ctl:w".to_owned(),
            seq: 1,
            event_id: id.to_owned(),
            event_type: event_type.to_owned(),
            schema_version: 1,
            environment: "paper".to_owned(),
            recorded_at: String::new(),
            prev_hash: Digest::ZERO,
            hash: Digest::of(&body),
            body,
        }
    }

    /// Only the agent's own `confirm_order` buy, in the member's own thread with this agent, with
    /// a positive quantity, is a card a tap confirms; the message is found by its own id.
    #[test]
    fn a_tap_confirms_only_the_agents_buy_card_in_the_members_own_thread() {
        let thread = Thread::agent("agt", "alice");
        let card = |id: &str, event_type: &str, author: &str, member: &str, card: &str| {
            confirmable(&[posted(id, event_type, author, member, card)], &thread, id)
        };
        assert_eq!(
            card("m1", "ChatMessagePosted", "agent", "alice", CARD),
            Some(("i-1".to_owned(), "2".to_owned()))
        );
        let rows = [posted("m1", "ChatMessagePosted", "agent", "alice", CARD)];
        assert_eq!(confirmable(&rows, &thread, "m2"), None, "another message");
        assert_eq!(
            confirmable(&rows, &Thread::agent("agt", "bob"), "m1"),
            None,
            "another member's thread"
        );
        assert_eq!(
            confirmable(&rows, &Thread::agent("other", "alice"), "m1"),
            None,
            "another agent's thread"
        );
        for (why, event_type, author, body) in [
            ("another record", "ChatTurnRecorded", "agent", CARD),
            (
                "the owner's own message",
                "ChatMessagePosted",
                "owner",
                CARD,
            ),
            (
                "another card",
                "ChatMessagePosted",
                "agent",
                r#"{"kind":"proposal","side":"buy","qty":"2","instrument_id":"i-1"}"#,
            ),
            (
                "a sale",
                "ChatMessagePosted",
                "agent",
                r#"{"kind":"confirm_order","side":"sell","qty":"2","instrument_id":"i-1"}"#,
            ),
            (
                "no quantity",
                "ChatMessagePosted",
                "agent",
                r#"{"kind":"confirm_order","side":"buy","qty":"0","instrument_id":"i-1"}"#,
            ),
            (
                "no instrument",
                "ChatMessagePosted",
                "agent",
                r#"{"kind":"confirm_order","side":"buy","qty":"2"}"#,
            ),
            ("no card", "ChatMessagePosted", "agent", "null"),
        ] {
            assert_eq!(card("m1", event_type, author, "alice", body), None, "{why}");
        }
    }
}
