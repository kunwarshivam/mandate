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
//! The model gateway ([inference spec](../../../docs/specs/inference.md), E15-6 to E15-10): the one
//! component that calls a model. Every call names a pinned registry entry (INF-1), goes only to an
//! endpoint that serves exactly that identity and that the workspace's policy allows (INF-2,
//! INF-12), has a finite deadline after which nothing it returns is used or cached (INF-3),
//! reserves its maximum cost under every cap before it is sent (INF-6, INF-7), and passes a
//! deterministic prompt guard first (INF-8). It returns exactly one [`CallRecord`]: a
//! schema-valid output of the pinned model, or a typed outcome with no output (INF-4). The caller
//! journals the record (`ModelInvocationRecorded`, journal spec §9.19) before it uses the output
//! (INF-10).
//!
//! **Opinions, never orders** (`AGENTS.md` rule 4, INF-5): a request carries no function or plugin,
//! and no tool but the pinned declarations an agentic turn offers ([`tools`], inference spec v0.4,
//! DEC-2203); an output is data validated against its pinned schema, and a tool call is data
//! validated against its tool's, which the gateway returns and never runs; nothing here sizes,
//! gates, or places an order, and the crate layering keeps it from reaching any crate that does,
//! or any mandate or policy writer (INF-16).
//!
//! **Budgets are settings** ([`budgets`], DEC-2207): the deployment's monthly cap folds from the
//! deployment meter stream across every workspace, and no figure is in the code.
//!
//! **Pure.** Time, transport, provider keys, and event ids come in through [`provider`]'s traits.
//! The [`fake::RecordedTransport`] is the default everywhere except a real cell with a vaulted key
//! (DEC-997 item 4); the [`openrouter::OpenRouter`] adapter sets DEC-997 item 2's routing lock.

pub mod budgets;
pub mod call;
pub mod fake;
pub mod frame;
pub mod guard;
pub mod kinds;
pub mod meter;
pub mod money;
pub mod openrouter;
pub mod policy;
pub mod provider;
pub mod record;
pub mod registry;
pub mod schema;
pub mod tools;
pub mod wire;

pub use call::{CallRequest, Gateway, RateCaps, RequestError, Settings};
pub use kinds::{KeyOwner, Outcome, Purpose};
pub use record::{CallRecord, CandidateKind, PayloadError, Verdict, VerdictResult};
