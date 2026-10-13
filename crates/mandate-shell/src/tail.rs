//! The runtime's control-stream tail (E7-26, the runtime grant→submit continuation — stream L /
//! M7's remainder; the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s R0 rows; DEC-155
//! items 2 and 5; mandate spec §6.4; journal spec §2, §3, §9.7).
//!
//! `mandate-runtime`'s escalation half owns the answering itself: "The owner's answer arrives only
//! as a control-stream `ApprovalResponseSubmitted`, tailed and handed in as `Input::Journal`"
//! (`crates/mandate-runtime/src/escalation.rs`), and an admitted grant's re-validation "journals and
//! hands an intent, which is the bound order and nothing else (EI-4)". What that half lacks today is
//! the **tail**: the shell's `Session` "folds only its agent and account streams ... and reads no
//! control stream", and its blanket fold actor "is what admission never admits (EI-10), so the
//! control-stream tail that maps the envelope's `actor.kind` is stream L's, with the tail itself
//! (M7 brief, Decisions needed 5)" (`crates/mandate-shell/src/tracer.rs`). DEC-155 item 5 settles
//! where it lives: "The notifier driver and the control-stream tail belong in the shell." This
//! module is that tail's fold: the one read of the workspace control stream, turned into the
//! [`FoldedEvent`]s the runtime is handed as `Input::Journal`, one per row, verbatim.
//!
//! The tail **judges nothing** (DEC-138 item 3: the shell "holds no trading logic", and every
//! decision a crate's): it neither admits, re-validates, orders nor filters. Which rows it hands
//! is decided, not guessed — DEC-912 item 1's answer fold reads "in journal order: the first
//! answer naming the ask's approval decides it", and DEC-914 item 2 binds the records "verbatim,
//! in journal order ... never re-sorted by another key, never filtered to the answers alone, no
//! read of a row's members from anywhere but the row's own payload", so the tail hands **every
//! row of the read, in the journal's own order**: the owner's answer and command rows the
//! runtime's `is_owner_input` routes to escalation, and every other control-stream row beside
//! them — an `AgentDeployed` or an approval answer, the runtime's own arms decide what each acts
//! on, and a row the runtime holds inert stays inert. The envelope's `actor.kind` is mapped per
//! journal spec §3 (`system`, `agent`, `user`, `broker`, `platform_operator`, and the `client`
//! of v0.22), and a kind the tail cannot read gives anything but `User` — the [`FoldedEvent`]
//! contract's own fail-closed sentence — so no fold can admit a grant the envelope did not carry
//! a `user` for (EI-10, check 3).
//!
//! What the tail does **not** decide is where it is read from: the replay at the stream open, the
//! per-wake re-read of the live grant wait (DEC-914 item 2's "a read taken again on every wake"),
//! and the cursor each starts from are the implementation PR's call sites, written with it and
//! judged by its review against DEC-914 item 7's reservation. Nothing here reads a journal, a
//! clock, a host or an order (`AGENTS.md` rules 7 and 8): `rows` arrive as the journal's own
//! `ORDER BY seq` read handed in.

use mandate_journal::StoredEvent;
use mandate_runtime::FoldedEvent;

use crate::error::{Cause, ShellError};
use crate::stages::Stage;

/// The events the runtime is handed of one read of the workspace control stream (stream L / M7's
/// remainder; DEC-155 item 2; journal spec §2's copy rule): `rows` — the journal's own
/// `ORDER BY seq` read of `ctl:<workspace>` — mapped one-to-one into the [`FoldedEvent`]s the
/// runtime's `Input::Journal` carries, never re-sorted, never filtered, and with no row's members
/// read from anywhere but the row's own body.
///
/// Each event is its row, verbatim: the stream named is the row's own `stream_id` (so the
/// runtime's owner-input routing, `is_owner_input`, recognizes it by `ctl:`'s journal §2
/// prefix), the seq, event id and event type are the row's own, the payload and `causation_id`
/// are the row body's own members read from its canonical bytes — "what is folded is what was
/// committed", the same reading `Session::fold` gives the agent and account streams — and the
/// actor is the envelope's own `actor.kind` (journal spec §3), mapped per kind. A body the tail
/// cannot read, and a body holding no `payload` member, refuse as a difference with nothing
/// handed: never a guess (`AGENTS.md` rule 3; the same `Envelope` refusals the agent and account
/// folds give).
///
/// The actor mapping is total and fail-closed (the [`FoldedEvent`] contract: "Admission admits a
/// response only from a `user` (check 3, EI-10), so a reader that cannot tell gives anything but
/// `User`"): `user` folds `ActorKind::User`; `system`, `agent`, `broker` and `platform_operator`
/// fold their own kinds; and the `client` of journal spec v0.22 — "A client is never a `user`, so
/// a rule that requires a user actor" refuses — and any kind the tail cannot read fold
/// `ActorKind::System`, what admission never admits, so no reading of this map can carry a
/// grant the envelope's writer did not stand behind (EI-10, DEC-176: the refusing reading). The
/// mapping names no default arm and no defaulting combinator, so the crate's fail-closed
/// scan keeps it this way (`stages::fail_closed`).
///
/// # Errors
/// [`ShellError::Envelope`] for a row whose body cannot be read (`stored body`) or holds no
/// payload member (`payload`) — each a stop with nothing handed, the whole read refused rather
/// than a partial fold handed on.
pub fn tailed(rows: &[StoredEvent]) -> Result<Vec<FoldedEvent>, ShellError> {
    let _ = rows;
    Err(ShellError::Refused {
        stage: Stage::Journal,
        cause: Cause::Unimplemented { story: "E7-26" },
    })
}
