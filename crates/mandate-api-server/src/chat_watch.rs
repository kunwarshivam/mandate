//! The agent messages first ([DEC-2293](../../../docs/project/decisions/DEC-2293.md), agent
//! harness spec §15.13 item 9): one pass over the hosted workspace's agents, every
//! [`PASS`], posting into each owner's thread
//! - each fill of the agent's own orders, as a deterministic message with the fill as its source;
//! - each approval it waits on, as a card the web answers through the approval routes;
//! - once a UTC day after [`BRIEF_AFTER_SECS`], a proactive run of the chat model, which writes the
//!   owner a short note when there is something worth saying.
//!
//! What has been said is read back from the journal: a message names its cause in `sources`, so a
//! restart or a second pass never says it twice. A stopped agent says nothing, and only an active
//! one starts a proactive run (DEC-2293 item 8).

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use mandate_canon::Value;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;
use tokio::runtime::Handle;

use crate::chat_run::{self, Start, Thread, append_system};
use crate::http::App;
use crate::reads::{Deployment, account_ref, agent_orders, deployed, envelope, mode};
use crate::routes::{document, object, string};

/// How often a pass runs.
pub const PASS: Duration = Duration::from_secs(15);

/// The second of the UTC day after which an agent's daily note may run: 21:00, after the US
/// regular session closes.
pub const BRIEF_AFTER_SECS: i64 = 21 * 3_600;

/// The `sources` ref prefix of a message that a record caused.
const EVENT_REF: &str = "ev:";

/// The `sources` label of the daily note's marker, `brief:{date}`.
const BRIEF_REF: &str = "brief:";

/// Runs a pass every [`PASS`] until the process ends.
pub async fn watch(app: Arc<App>) {
    loop {
        let pass = Arc::clone(&app);
        let _ = tokio::task::spawn_blocking(move || {
            let handle = Handle::current();
            pass_once(&pass, &handle)
        })
        .await;
        tokio::time::sleep(PASS).await;
    }
}

/// One pass on a blocking thread: the number of messages it posted and runs it started.
pub fn pass_once(app: &Arc<App>, handle: &Handle) -> usize {
    let Ok(workspace) = app.hosted.workspace.to_ulid_text() else {
        return 0;
    };
    let Some(now) = app.clock.now() else {
        return 0;
    };
    let control = handle
        .block_on(app.journal.rows(&format!("ctl:{workspace}")))
        .unwrap_or_default();
    let Some(environment) = control.first().map(|row| row.environment.clone()) else {
        return 0;
    };
    let mut posted: usize = 0;
    for deployment in deployed(&control) {
        if deployment.4 {
            continue;
        }
        let pass = Pass {
            app,
            handle,
            workspace: &workspace,
            environment: &environment,
            control: &control,
            deployment: &deployment,
            now,
        };
        posted = posted.saturating_add(pass.agent());
    }
    posted
}

struct Pass<'a> {
    app: &'a Arc<App>,
    handle: &'a Handle,
    workspace: &'a str,
    environment: &'a str,
    control: &'a [StoredEvent],
    deployment: &'a Deployment,
    now: UtcNanos,
}

impl Pass<'_> {
    fn agent(&self) -> usize {
        let agent = &self.deployment.0;
        let members = owners(self.control, agent);
        let agent_rows = self
            .handle
            .block_on(
                self.app
                    .journal
                    .rows(&format!("agent:{}:{agent}", self.workspace)),
            )
            .unwrap_or_default();
        let mut said: usize = 0;
        for member in &members {
            let thread = Thread::agent(agent, member);
            let known = said_in(self.control, &thread);
            for (cause, text, card) in self.fills().into_iter().chain(self.approvals(&agent_rows)) {
                if known.contains(&format!("{EVENT_REF}{cause}")) {
                    continue;
                }
                let sources = [("What happened".to_owned(), format!("{EVENT_REF}{cause}"))];
                if self.post(&thread, &text, card, &sources, &cause) {
                    said = said.saturating_add(1);
                }
            }
            if self.brief_due(&known, &agent_rows) && self.brief(&thread) {
                said = said.saturating_add(1);
            }
        }
        said
    }

    fn post(
        &self,
        thread: &Thread,
        text: &str,
        card: Value,
        sources: &[(String, String)],
        cause: &str,
    ) -> bool {
        let payload = thread.message_payload("agent", Some(text), Some(card), sources, None, true);
        append_system(
            self.app,
            self.handle,
            (self.workspace, self.environment),
            ("ChatMessagePosted", Some(cause)),
            object(payload),
        )
        .is_some()
    }

    /// Each fill of the agent's own orders: its event id, the message, and its card.
    fn fills(&self) -> Vec<(String, String, Value)> {
        let Some(doc) = document(self.app, self.deployment) else {
            return Vec::new();
        };
        let Some(reference) = account_ref(self.control, &doc.connection_id) else {
            return Vec::new();
        };
        let account = self
            .handle
            .block_on(
                self.app
                    .journal
                    .rows(&format!("acct:{}:{reference}", self.workspace)),
            )
            .unwrap_or_default();
        let own: BTreeSet<String> = agent_orders(&account, &self.deployment.0)
            .iter()
            .filter_map(|o| {
                o.get("client_order_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect();
        account
            .iter()
            .filter(|row| matches!(row.event_type.as_str(), "FillApplied" | "LateFillApplied"))
            .filter_map(|row| Some((row, envelope(row)?.get("payload")?.clone())))
            .filter(|(_, p)| {
                p.get("client_order_id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| own.contains(id))
            })
            .map(|(row, p)| {
                let text = |name: &str| p.get(name).and_then(Value::as_str).unwrap_or_default();
                let symbol = self.symbol(text("instrument_id"));
                let side = text("side");
                let verb = if side == "sell" { "Sold" } else { "Bought" };
                let message = format!(
                    "{verb} {} {symbol} at {}.",
                    text("qty_gross"),
                    text("price")
                );
                let card = object(vec![
                    ("kind", string("fill")),
                    ("side", string(side)),
                    ("symbol", string(&symbol)),
                    ("qty", string(text("qty_gross"))),
                    ("price", string(text("price"))),
                ]);
                (row.event_id.clone(), message, card)
            })
            .collect()
    }

    /// Each approval the agent waits on: its request's event id, the message, and its card.
    fn approvals(&self, agent_rows: &[StoredEvent]) -> Vec<(String, String, Value)> {
        crate::reads::approvals(&self.deployment.0, agent_rows)
            .into_iter()
            .filter(|view| view.state.name() == "pending")
            .map(|view| {
                let member = |name: &str| {
                    view.order
                        .get(name)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                let symbol = self.symbol(&member("instrument"));
                let message = format!(
                    "I'd like your OK to {} {} {symbol}.",
                    member("side"),
                    member("qty")
                );
                let card = object(vec![
                    ("kind", string("approval")),
                    ("approval_id", string(&view.approval_id)),
                    ("symbol", string(&symbol)),
                    ("order", view.order.clone()),
                ]);
                (view.approval_id.clone(), message, card)
            })
            .collect()
    }

    /// The symbol of `instrument`: the mandate's pinned instrument, or the catalogue's, or the id.
    fn symbol(&self, instrument: &str) -> String {
        let pinned = self.app.artifacts.as_deref().and_then(|store| {
            let (document, _) = crate::versions::stored(store, &self.deployment.2)?;
            document
                .get("universe")?
                .get("pinned_instruments")?
                .as_array()?
                .iter()
                .find(|i| i.get("asset_id").and_then(Value::as_str) == Some(instrument))?
                .get("symbol")?
                .as_str()
                .map(str::to_owned)
        });
        pinned
            .or_else(|| {
                let store = self.app.artifacts.as_deref()?;
                crate::catalogue::pinnable_snapshots(self.control, store)?
                    .into_iter()
                    .find(|s| s.instrument_id == instrument)
                    .map(|s| s.symbol)
            })
            .unwrap_or_else(|| instrument.to_owned())
    }

    /// Whether today's note is due: after [`BRIEF_AFTER_SECS`] on a weekday, the agent active,
    /// a chat model configured, and none yet today.
    fn brief_due(&self, known: &BTreeSet<String>, agent_rows: &[StoredEvent]) -> bool {
        let secs = self.now.secs();
        let day = secs.div_euclid(86_400);
        let weekday = day.saturating_add(3).rem_euclid(7);
        let active = mode(agent_rows).is_none_or(|m| m == "active");
        self.app.chat.gateway.is_some()
            && !self.app.chat.models.is_empty()
            && secs.rem_euclid(86_400) >= BRIEF_AFTER_SECS
            && weekday < 5
            && active
            && !known.contains(&format!("{BRIEF_REF}{}", self.now.date()))
    }

    /// Starts today's note: a marker message, so it starts once, then the proactive run.
    fn brief(&self, thread: &Thread) -> bool {
        let date = self.now.date().to_string();
        let run = format!("run_{}", self.app.chat.ulid(self.now).to_ascii_lowercase());
        if !self.app.chat.claim(&thread.key(), &run) {
            return false;
        }
        let sources = [("Today's note".to_owned(), format!("{BRIEF_REF}{date}"))];
        let payload = thread.message_payload(
            "agent",
            None,
            Some(object(vec![
                ("kind", string("brief")),
                ("date", string(&date)),
            ])),
            &sources,
            Some(&run),
            true,
        );
        let Some(marker) = append_system(
            self.app,
            self.handle,
            (self.workspace, self.environment),
            ("ChatMessagePosted", None),
            object(payload),
        ) else {
            self.app.chat.release(&thread.key());
            return false;
        };
        chat_run::start_blocking(
            Arc::clone(self.app),
            self.handle,
            Start {
                workspace: self.workspace.to_owned(),
                thread: thread.clone(),
                run,
                gate: None,
                now: self.now,
                deployment: Some(self.deployment.clone()),
                cause: marker.event_id,
                message: "It is the end of the trading day. Look at today's orders, fills, \
                          positions and news for this agent. If something happened that the \
                          owner would want to know, write them a short note of two or three \
                          sentences with chat.reply. If nothing did, reply nothing."
                    .to_owned(),
                proactive: true,
            },
        );
        true
    }
}

/// The members whose thread with `agent` hears from it: whoever deployed it, and every member
/// who has written in a thread with it.
fn owners(control: &[StoredEvent], agent: &str) -> BTreeSet<String> {
    control
        .iter()
        .filter(|row| {
            matches!(
                row.event_type.as_str(),
                "AgentDeployed" | "ChatMessagePosted"
            )
        })
        .filter_map(envelope)
        .filter(|e| {
            let p = e.get("payload");
            let agent_id = p.and_then(|p| p.get("agent_id")).and_then(Value::as_str);
            let owner = e.get("event_type").and_then(Value::as_str) == Some("AgentDeployed")
                || p.and_then(|p| p.get("author")).and_then(Value::as_str) == Some("owner");
            agent_id == Some(agent) && owner
        })
        .filter_map(|e| {
            let actor = e.get("actor")?;
            (actor.get("kind").and_then(Value::as_str) == Some("user"))
                .then(|| actor.get("id").and_then(Value::as_str).map(str::to_owned))
                .flatten()
        })
        .collect()
}

/// The `sources` refs of every agent message already in `thread`.
fn said_in(control: &[StoredEvent], thread: &Thread) -> BTreeSet<String> {
    control
        .iter()
        .filter(|row| row.event_type == "ChatMessagePosted")
        .filter_map(envelope)
        .filter_map(|e| e.get("payload").cloned())
        .filter(|p| thread.holds(Some(p)))
        .filter(|p| p.get("author").and_then(Value::as_str) == Some("agent"))
        .flat_map(|p| {
            p.get("sources")
                .and_then(Value::as_array)
                .unwrap_or_default()
                .iter()
                .filter_map(|s| s.get("ref").and_then(Value::as_str).map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect()
}
