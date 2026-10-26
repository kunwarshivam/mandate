//! The deployment's production ports over the journal (DEC-2378 S2): [`Facts`], the one
//! `mandate-gate-context` fold the process feeds as it tails its streams, and [`Flatten`], the
//! flatten planner that sells the agent's sub-ledger from it.
//!
//! The daemon reaches no broker and no market data (`AGENTS.md` rule 12), so every fact a port
//! reads is a journaled row the process has already read. A flatten never waits on the facts an
//! opening needs: with none of them journaled it still sells everything the agent's own fills
//! hold and cancels every order of the agent's that no terminal state has ended (rule 13).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mandate_accounting::InstrumentId;
use mandate_canon::{Key, Object, Value, parse};
use mandate_domain::AssetClass;
use mandate_gate_context::{Freshness, Journaled, Row};
use mandate_journal::StoredEvent;
use mandate_runtime::{EventDraft, FlattenLeg, FlattenPlan, FlattenPlanner, FlattenRequest};
use mandate_spec::document::Mandate;

use crate::runtime::{Refs, Wiring};

mod plan;

pub use plan::Plan;

/// How fresh, in risk-clock seconds, the facts an opening reads must be (DEC-2378 item 4): a mark
/// within a minute, as the executor marks each deployed instrument at every tick, and the
/// account's snapshot within an hour, since the plan reads only its equity for validation and the
/// executor's binding gate reads its own account.
pub const FRESH: Freshness = Freshness {
    mark_max_age_s: 60,
    snapshot_max_age_s: 3600,
};

/// `base` with the deployment's production ports (DEC-2378 S3): the shared fold for `agent`,
/// ticked by the scheduler's `clock` stream; the flatten over it with the asset classes of
/// `document`'s pinned universe; the order plan over the stored mandate; the view and every
/// draft's references naming the deployed `version`. The gate's dry run stays `base`'s, which
/// denies every opening until its deferred checks can be journaled (DEC-2378 item 11). A
/// document that does not parse, which `deployment::document` has already refused, keeps
/// `base`'s plan and view.
pub fn wired(
    mut base: Wiring,
    version: &str,
    document: &Value,
    clock: &str,
    agent: &str,
) -> Wiring {
    let facts = Facts::new(clock, agent);
    base.flatten = Box::new(Flatten::new(facts.clone(), classes(document)));
    if let Ok(mandate) = Mandate::parse(document) {
        base.view.version = version.to_owned();
        base.view.working_universe = mandate
            .universe
            .pinned_instruments
            .iter()
            .filter_map(|pinned| InstrumentId::new(pinned.asset_id.as_str()).ok())
            .collect();
        base.view.approval.timeout_s = i64::from(mandate.autonomy.approval.timeout_s);
        base.plan = Box::new(Plan::new(facts.clone(), mandate, FRESH));
    }
    base.refs = Box::new(MandateRefs(version.to_owned()));
    base.facts = Some(facts);
    base
}

/// Every draft's references: the deployed mandate version, at schema version 1 (journal spec
/// §9). A draft whose version needs more, such as `ModelOutputRecorded` version 2's model
/// registry, is answered by the wiring that writes it (DEC-2383).
struct MandateRefs(String);

impl Refs for MandateRefs {
    fn refs(&self, _draft: &EventDraft) -> Option<(u64, Object)> {
        let mut refs = Object::new();
        refs.insert(
            Key::new("mandate_version").ok()?,
            Value::Str(self.0.clone()),
        );
        Some((1, refs))
    }
}

/// Each pinned instrument's asset class, by asset id, from the stored mandate `document`; none
/// when the document does not parse, which `deployment::document` has already refused.
pub fn classes(document: &Value) -> BTreeMap<String, AssetClass> {
    Mandate::parse(document)
        .map(|mandate| {
            mandate
                .universe
                .pinned_instruments
                .iter()
                .map(|pinned| (pinned.asset_id.as_str().to_owned(), pinned.asset_class))
                .collect()
        })
        .unwrap_or_default()
}

/// The shared fold of one deployment's streams, cloned into every port that reads it. Each row
/// reaches the fold once, in its stream's order, however often the process reads its stream.
#[derive(Clone)]
pub struct Facts(Rc<RefCell<Fed>>);

struct Fed {
    unplanned: u32,
    clock: String,
    agent: String,
    journaled: Option<Journaled>,
    heads: BTreeMap<String, u64>,
}

impl Facts {
    /// The fold for `agent`, ticked by the scheduler's `clock` stream, before its account is known.
    pub fn new(clock: &str, agent: &str) -> Self {
        Self(Rc::new(RefCell::new(Fed {
            unplanned: 0,
            clock: clock.to_owned(),
            agent: agent.to_owned(),
            journaled: None,
            heads: BTreeMap::new(),
        })))
    }

    /// Follows `account`, the stream the control stream names for the deployment's connection
    /// (DEC-1014). The first call starts the fold and forgets every stream's head, so the rows read
    /// before the account was known reach it when they are next fed. A later call changes nothing:
    /// the process follows one account for its whole life.
    pub fn follow(&self, account: &str) {
        let Ok(mut fed) = self.0.try_borrow_mut() else {
            return;
        };
        if fed.journaled.is_some() {
            return;
        }
        let journaled = Journaled::new(account, &fed.clock, &fed.agent);
        fed.journaled = Some(journaled);
        fed.heads.clear();
    }

    /// Folds the rows past the fold's head for their stream, in order. Nothing is folded before
    /// the account is followed. A row whose body does not parse is folded with no payload, so one
    /// of a type the fold reads holds every later opening, and one of any other type changes
    /// nothing.
    pub fn feed(&self, rows: &[StoredEvent]) {
        let Ok(mut fed) = self.0.try_borrow_mut() else {
            return;
        };
        let Fed {
            journaled, heads, ..
        } = &mut *fed;
        let Some(journaled) = journaled.as_mut() else {
            return;
        };
        for stored in rows {
            let head = heads.entry(stored.stream_id.clone()).or_insert(0);
            if stored.seq <= *head {
                continue;
            }
            *head = stored.seq;
            let payload = parse(&stored.body)
                .ok()
                .and_then(|body| body.get("payload").cloned())
                .unwrap_or(Value::Null);
            let row = Row {
                stream: &stored.stream_id,
                event_type: &stored.event_type,
                payload: &payload,
            };
            let _refused_rows_hold_every_later_opening = journaled.fold(row);
        }
    }

    /// How many held instruments a flatten has left unplanned since the last call, and resets the
    /// count: the process raises `flatten_unplanned` for each nonzero answer (DEC-2378 item 8).
    pub fn take_unplanned(&self) -> u32 {
        self.0
            .try_borrow_mut()
            .map(|mut fed| std::mem::take(&mut fed.unplanned))
            .unwrap_or(0)
    }

    fn note_unplanned(&self, count: u32) {
        if let Ok(mut fed) = self.0.try_borrow_mut() {
            fed.unplanned = fed.unplanned.saturating_add(count);
        }
    }

    /// `read` over the fold, or `None` before the account is followed.
    pub fn read<T>(&self, read: impl FnOnce(&Journaled) -> T) -> Option<T> {
        let fed = self.0.try_borrow().ok()?;
        fed.journaled.as_ref().map(read)
    }
}

/// The flatten planner (trading-domain spec §5.5): exactly the agent's sub-ledger, and only the
/// agent's own working orders, never the account's. An owner exit names one instrument and
/// touches only it. No sell is deferred here: a risk exit and a kill switch are exempt from the
/// session rules, an owner exit outside the regular session carries the owner's confirmed bid,
/// and the broker holds what it will not take (`AGENTS.md` rule 13). A held instrument it cannot
/// plan, one with no asset class in the universe or an id that does not parse, is counted for the
/// process's `flatten_unplanned` alert rather than dropped silently; the executor still flattens
/// its own sub-ledger of it.
pub struct Flatten {
    facts: Facts,
    classes: BTreeMap<String, AssetClass>,
}

impl Flatten {
    /// The planner over `facts`, with each universe instrument's asset class from the mandate
    /// (`classes`, keyed by asset id).
    pub fn new(facts: Facts, classes: BTreeMap<String, AssetClass>) -> Self {
        Self { facts, classes }
    }
}

impl FlattenPlanner for Flatten {
    fn plan(&self, request: &FlattenRequest) -> FlattenPlan {
        let scoped = |instrument: &str| {
            request
                .instrument
                .as_ref()
                .is_none_or(|named| named.as_str() == instrument)
        };
        let (held, working) = self
            .facts
            .read(|journaled| (journaled.held(), journaled.working()))
            .unwrap_or_default();
        let scoped_held: Vec<_> = held
            .into_iter()
            .filter(|(instrument, _)| scoped(instrument))
            .collect();
        let wanted = scoped_held.len();
        let sells: Vec<FlattenLeg> = scoped_held
            .into_iter()
            .filter_map(|(instrument, qty)| {
                Some(FlattenLeg {
                    asset_class: *self.classes.get(&instrument)?,
                    instrument: InstrumentId::new(&instrument).ok()?,
                    qty,
                    deferred_to_regular_session: false,
                })
            })
            .collect();
        let unplanned = wanted.saturating_sub(sells.len());
        self.facts
            .note_unplanned(u32::try_from(unplanned).unwrap_or(u32::MAX));
        let cancel_client_order_ids = working
            .into_iter()
            .filter(|(_, instrument)| scoped(instrument))
            .map(|(order, _)| order)
            .collect();
        FlattenPlan {
            cancel_client_order_ids,
            sells,
            purpose: request.initiator.sell_purpose(),
            confirmation: request.confirmation.clone(),
        }
    }
}
