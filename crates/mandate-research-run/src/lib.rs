#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The agent harness's research worker ([agent harness spec](../../../docs/specs/agent-harness.md)
//! §4, §6.2 to §6.5, §7; backlog E19-2, E19-3 and the harness half of E17-2; DEC-431 item 20,
//! DEC-996): what a research run reads, how it asks the model gateway, and how the answer is read
//! back as typed data.
//!
//! # What lives here, and what never does
//!
//! - **The retrieval plan** ([`RetrievalPlan`]) and the one function that runs it ([`retrieve`]):
//!   fixed reads at a cut-off, every cap enforced, oldest items dropped first, and every drop
//!   recorded (§6.3). Every read is filtered on knowledge time again here, whatever the reader
//!   says, so a reader that leaks a later record still cannot show it to the model (HI-13, DP-1).
//! - **Read-only ports** ([`BarReader`], [`NewsReader`], [`FilingReader`], [`InstrumentReader`]):
//!   each takes `&self` and returns owned data, and none can place, cancel, approve, append, or
//!   change anything (HI-4). Lane I3 implements the news and filings readers.
//! - **The gateway port** ([`ModelGateway`]): one call per run, built from typed inputs only
//!   ([`ResearchCall`]), answered by exactly one of a reply or a typed failure (inference spec §3).
//!   Lane I1 implements it; until then tests use a recorded fake. [`settle`] re-checks the reply's
//!   call id, reported identity, and completion time against the request, so a late or
//!   substituted answer is a failure here too (INF-2, INF-3, caller side).
//! - **The strict parse** ([`parse_output`]): the model's bytes become [`CandidateOutput`] or an
//!   [`OutputRejection`], never anything in between. A candidate is still only an opinion: the
//!   runtime core checks it, fills its platform facts, and runs `mandate_research::admit`.
//!
//! **There is no journal here.** The crate depends on no journal crate (`xtask/layers.toml`'s
//! `forbidden_internal`), so the runtime core stays the agent stream's only writer (HI-16), and it
//! sits at layer 5, below `mandate-runtime`, so it cannot name `IntentSink` (HI-1). The capture
//! records it builds ([`RetrievedItem::record`]) are what the shell stores as artifacts and what the
//! core journals as `ObservationRecorded` before the call is sent (HI-2, DP-3).
//!
//! **No dollar figure of the agent or account has a field** in [`CallInputs`], and no credential,
//! vault reference, or personal-data field exists in any type here (HI-11, HI-12): what the model
//! may not see cannot be passed.
//!
//! Everything is pure: no clock, no randomness, no I/O of its own, `BTreeMap` and `BTreeSet` only,
//! and exact `mandate-num` arithmetic (ADR-0001 ES-21). Instants are arguments.

mod call;
mod error;
mod item;
mod output;
mod plan;
mod ports;
mod retrieve;

pub use call::{
    CallContext, CallFailure, CallInputs, CallOutcome, CallPlan, ErrorKind, GatewayReply,
    MemoryThesis, ModelPin, ResearchCall, Treatment, UntrustedItem, settle,
};
pub use error::ResearchRunError;
pub use item::{
    Allowlist, Bar, Filing, InstrumentFact, ItemId, ItemKind, NewsItem, SourceId, Symbol,
};
pub use output::{
    CandidateInvalidation, CandidateOutput, CandidateThesis, OutputBounds, OutputRejection,
    parse_output,
};
pub use plan::{RetrievalPlan, Scope};
pub use ports::{
    BarReader, FilingReader, InstrumentReader, ModelGateway, NewsReader, ReadError, Readers,
    TextQuery,
};
pub use retrieve::{Drop, DropReason, Retrieval, RetrievedItem, retrieve};
