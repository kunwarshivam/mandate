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
//! The workspace API server process ([workspace API spec](../../../docs/specs/workspace-api.md)
//! §3.11, story E21-31, [DEC-978](../../../docs/project/decisions/DEC-978.md)): HTTP over
//! `mandate-api`'s contract, behind the cell's one ingress.
//!
//! - [`config`]: the start checks. The process binds only loopback or its pod's own address, starts
//!   with no secret-named variable in its environment and no password in its database URL, and
//!   reads its session signing key from a credential file only its own user can read.
//! - [`lifecycle`]: readiness and the drain. A local predicate stands in for `mandate-process`
//!   (E21-28) behind [`lifecycle::Lifecycle`] until that crate lands.
//! - [`http`]: the router. `/livez` and `/readyz` read no data; every other path answers the one
//!   401 of API-1 until its route lands; CORS allows the one configured app origin (§3.3 item 5).
//! - [`writes`] and [`commands`]: pause, the kill switch, and an approval's Skip, recorded as the
//!   CLI records them on the workspace's control stream before the `202`, and replayed by key
//!   (DEC-1007).
//! - [`versions`] and [`version_routes`]: creating a version from its document, the confirmation
//!   screen, a new mandate's confirm, deploy, and go-live, judged and recorded as the CLI records
//!   them (DEC-2101). [`stepup`] is the port every step-up goes through, which counts no evidence
//!   until E9-4 binds it; [`objects`] is the content-addressed store those routes add records to.
//! - [`catalogue`]: the registered instruments and signal models a version may name, folded as
//!   the create folds them (DEC-2136).
//! - [`challenges`]: the step-up challenge route and the store of the challenges it issues, each
//!   issued under the context `authorize` returned for its action's row (DEC-2103).
//! - [`passkeys`]: one's own passkeys, the registration challenge and the list (§3.6, DEC-2124),
//!   each read from one fold of the member's passkeys on the control stream.
//! - [`confirm`]: the code the web app shows for a challenge, and the port that verifies a paper
//!   grant's `cli_confirm` evidence against the owner's `StepUpConfirmed` (DEC-2104).
//! - [`methods`]: the port's binding, each step-up method to its own verifier, and the passkey
//!   verifier (DEC-2126), which a connect start commits beside its request as `StepUpAsserted`.
//! - [`writes`] also records resume, Stop, and an owner exit as the CLI records them, with step-up
//!   forwarded only once the port verifies it, and [`status`] reports a command's phase from the
//!   owning streams' direct causation (DEC-2102).
//! - [`connect`]: the connect start, which commits the pending connection and issues the
//!   single-use `state` the callback redeems (DEC-2623).
//! - [`backtests`]: the backtest and evaluation reads' folds (DEC-1001, DEC-1002, DEC-1143).
//! - [`journal_reads`]: the journal's audit reads, a stream's events page (§4.8.1, E12-6).
//! - [`finish`]: the connect's finish, from the open request and the executor's check to
//!   `plan_finish`'s answer (DEC-2342), [`finish_records`], the records it appends, and
//!   [`finish_pass`], the pass that appends them and tears closed connects down, run by its own
//!   process from [`finish_binding`] (DEC-2348).
//! - [`connections`]: the connections read's fold, references only (DEC-2620).
//! - [`reduction`]: route 2's challenge route, its store and request keys, and the address and
//!   device its limits count against (DEC-2117, DEC-2119).
//! - [`me`]: the caller's own session, outside every workspace path (§4.10, DEC-2113).
//! - [`connect_binding`]: whether the start binds a connect, from its four settings (DEC-2338).
//! - [`vault_codes`]: the callback's vault port, which writes a code as the API's vault identity
//!   and nothing else (DEC-2338).
//!
//! It never places, cancels, or sizes an order, and never calls a broker or a runtime (§1.4):
//! `xtask/layers.toml` keeps it from every crate that could.

pub mod acknowledgments;
pub mod alerts;
pub mod backtests;
pub mod catalogue;
pub mod challenges;
pub mod clock;
pub mod commands;
pub mod config;
pub mod confirm;
pub mod connect;
pub mod connect_binding;
pub mod connections;
pub mod documents;
pub mod finish;
pub mod finish_binding;
pub mod finish_pass;
pub mod finish_records;
pub mod http;
pub mod journal;
pub mod journal_reads;
pub mod keys;
pub mod lifecycle;
pub mod me;
pub mod members;
pub mod methods;
pub mod objects;
pub mod opening;
pub mod operator;
pub mod passkeys;
pub mod reads;
pub mod real_money;
pub mod reduction;
pub mod routes;
pub mod session;
pub mod standing;
pub mod status;
pub mod stepup;
pub mod subject;
pub mod token;
pub mod vault_codes;
pub mod vault_fingerprints;
pub mod vault_teardown;
pub mod version_routes;
pub mod versions;
pub mod writes;
