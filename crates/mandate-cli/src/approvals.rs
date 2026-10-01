//! `mandate approvals list`, `show`, `approve`, and `skip` (backlog E8-1 to E8-3; the M7 brief's
//! "The CLI control surface"; mandate spec §6.4; DEC-155 item 5, DEC-257 item 5).
//!
//! The inbox is the owner's own journal: `list` and `show` read an agent stream's approval events
//! and commit nothing. `approve` and `skip` each commit exactly one `ApprovalResponseSubmitted` to
//! the workspace control stream, one approval per command, with no batch form. `approve` needs the
//! confirmation code `show` printed, bound to the request's content hash, and records `cli_confirm`
//! step-up evidence authenticated when the owner ran it; `skip` takes the same arguments without
//! the code and needs no step-up (PX-7, PX-10). Whatever the CLI checks first, the runtime checks
//! again (DEC-155 item 5).

use mandate_canon::{Digest, Value, to_canonical};

use crate::control::{
    ControlError, ControlJournal, Ids, Now, Owner, Submitted, agent_stream, code_of, commit,
    envelopes, object, seconds, step_up, text,
};

/// One approval as `list` shows it: opaque ids and times, never the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub approval: String,
    pub agent: String,
    pub state: State,
    /// The deadline, as the request's risk-clock second.
    pub deadline_s: i64,
    /// Seconds left before the deadline at the moment `list` ran; 0 once it has passed.
    pub remaining_s: i64,
}

/// Where an approval stands, from the agent stream's events alone (the M7 brief's lifecycle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Pending,
    /// An admitted grant whose re-validation acted: the executor's binding gate still decides.
    Acted,
    /// Skipped by the owner, by re-validation, or by the deadline, or cancelled.
    Skipped(Ended),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ended {
    ByOwner,
    OnRevalidation,
    TimedOut,
    Canceled,
}

/// One request as the agent stream records it, and where it stands.
struct Request {
    approval: String,
    content: Value,
    content_hash: String,
    deadline_s: i64,
    state: State,
}

fn member<'v>(value: &'v Value, path: &str) -> Option<&'v Value> {
    path.split('.').try_fold(value, |v, k| v.get(k))
}

fn member_text<'v>(value: &'v Value, path: &str) -> Option<&'v str> {
    member(value, path).and_then(Value::as_str)
}

/// Every request on `agent`'s stream and its state, folded from the approval events in `seq`
/// order. A `refused` or `counted` answer leaves a request pending (EI-6), as the runtime's fold
/// does.
fn requests(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agent: &str,
) -> Result<Vec<Request>, ControlError> {
    let mut requests: Vec<Request> = Vec::new();
    for event in envelopes(journal, &agent_stream(owner, agent)?)? {
        let event_type = member_text(&event, "event_type").unwrap_or_default();
        if event_type == "ApprovalRequested" {
            let deadline_s = member(&event, "payload.deadline")
                .and_then(Value::as_int)
                .and_then(|secs| i64::try_from(secs).ok())
                .ok_or_else(|| ControlError::Journal("a request without a deadline".into()))?;
            requests.push(Request {
                approval: member_text(&event, "event_id")
                    .unwrap_or_default()
                    .to_owned(),
                content: member(&event, "payload.content")
                    .cloned()
                    .unwrap_or(Value::Null),
                content_hash: member_text(&event, "payload.content_hash")
                    .unwrap_or_default()
                    .to_owned(),
                deadline_s,
                state: State::Pending,
            });
            continue;
        }
        let Some(approval) = member_text(&event, "payload.approval") else {
            continue;
        };
        let ended = match event_type {
            "ApprovalTimedOut" => Some(State::Skipped(Ended::TimedOut)),
            "ApprovalCanceled" => Some(State::Skipped(Ended::Canceled)),
            "ApprovalRevalidated" => Some(match member_text(&event, "payload.result") {
                Some("act") => State::Acted,
                _ => State::Skipped(Ended::OnRevalidation),
            }),
            "ApprovalResponded"
                if member_text(&event, "payload.result") == Some("admitted")
                    && member_text(&event, "payload.verdict") == Some("skipped") =>
            {
                Some(State::Skipped(Ended::ByOwner))
            }
            _ => None,
        };
        if let Some(state) = ended
            && let Some(request) = requests
                .iter_mut()
                .find(|r| r.approval == approval && r.state == State::Pending)
        {
            request.state = state;
        }
    }
    Ok(requests)
}

/// How many of `agent`'s approvals are still pending, for `mandate agent status`.
///
/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read.
pub(crate) fn pending_count(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agent: &str,
) -> Result<usize, ControlError> {
    Ok(requests(journal, owner, agent)?
        .iter()
        .filter(|r| r.state == State::Pending)
        .count())
}

/// Every approval of `agents`, pending ones by deadline and then resolved ones, as the D5 inbox
/// orders them.
///
/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read.
pub fn list(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agents: &[&str],
    now: Now,
) -> Result<Vec<Listed>, ControlError> {
    let mut listed = Vec::new();
    for agent in agents {
        for request in requests(journal, owner, agent)? {
            let remaining_s = match request.state {
                State::Pending => request.deadline_s.saturating_sub(now.secs).max(0),
                State::Acted | State::Skipped(_) => 0,
            };
            listed.push(Listed {
                approval: request.approval,
                agent: (*agent).to_owned(),
                state: request.state,
                deadline_s: request.deadline_s,
                remaining_s,
            });
        }
    }
    listed.sort_by(|a, b| {
        (a.state != State::Pending, a.deadline_s, &a.approval).cmp(&(
            b.state != State::Pending,
            b.deadline_s,
            &b.approval,
        ))
    });
    Ok(listed)
}

/// What `show` renders: the request's content object exactly as committed, and the code the owner
/// re-types to approve it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub approval: String,
    pub content: Value,
    pub content_hash: String,
    /// `mandate_approval::confirmation_code` of the content hash.
    pub code: String,
    pub deadline_s: i64,
}

const NOT_PENDING: ControlError = ControlError::Refused {
    reason: "not_pending",
};

/// # Errors
/// [`ControlError::Refused`] with `not_pending` for an approval the agent stream does not hold as
/// pending, and `content_mismatch` for a request whose stated hash is not the hash of its content
/// object; [`ControlError::Journal`] when the journal cannot be read.
pub fn show(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agent: &str,
    approval: &str,
) -> Result<Shown, ControlError> {
    let request = requests(journal, owner, agent)?
        .into_iter()
        .find(|r| r.approval == approval && r.state == State::Pending)
        .ok_or(NOT_PENDING)?;
    let digest = Digest::of(&to_canonical(&request.content));
    if request.content_hash != format!("sha256:{}", digest.to_hex()) {
        return Err(ControlError::Refused {
            reason: "content_mismatch",
        });
    }
    Ok(Shown {
        approval: request.approval,
        code: code_of(&request.content),
        content: request.content,
        content_hash: request.content_hash,
        deadline_s: request.deadline_s,
    })
}

/// The request an answer may still be committed for: pending, and with its deadline ahead of the
/// owner's clock. Both are conveniences; the runtime judges both again (DEC-155 item 5).
fn answerable(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agent: &str,
    approval: &str,
    now: Now,
) -> Result<Shown, ControlError> {
    let shown = show(journal, owner, agent, approval)?;
    if now.secs >= shown.deadline_s {
        return Err(ControlError::Refused { reason: "late" });
    }
    Ok(shown)
}

fn response(
    owner: &Owner,
    agent: &str,
    shown: &Shown,
    verdict: &str,
    evidence: Value,
    now: Now,
) -> Result<Value, ControlError> {
    object(vec![
        ("agent", text(agent)),
        ("approval", text(&shown.approval)),
        ("verdict", text(verdict)),
        ("content_hash", text(&shown.content_hash)),
        ("submitted_at", seconds(now.secs)?),
        ("step_up", evidence),
        ("responder", text(&owner.user)),
        ("role", text("approver")),
    ])
}

/// Commits the owner's grant. Refused locally, committing nothing, when the approval is not
/// pending (`not_pending`), its deadline has passed at `now` (`late`), or `code` is not the one for
/// its content hash (`content_mismatch`).
///
/// # Errors
/// [`ControlError::Refused`] as above; [`ControlError::Journal`] when the append fails.
pub fn approve(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    agent: &str,
    approval: &str,
    code: &str,
    now: Now,
) -> Result<Submitted, ControlError> {
    let shown = answerable(journal, owner, agent, approval, now)?;
    if code != shown.code {
        return Err(ControlError::Refused {
            reason: "content_mismatch",
        });
    }
    let evidence = step_up(ids, now)?;
    let payload = response(owner, agent, &shown, "approved", evidence, now)?;
    commit(
        journal,
        ids,
        owner,
        "ApprovalResponseSubmitted",
        payload,
        now,
    )
}

/// Commits the owner's skip. Refused locally, committing nothing, when the approval is not pending
/// or its deadline has passed, since the approval is then already skipped.
///
/// # Errors
/// As [`approve`], less the code.
pub fn skip(
    journal: &mut dyn ControlJournal,
    ids: &mut dyn Ids,
    owner: &Owner,
    agent: &str,
    approval: &str,
    now: Now,
) -> Result<Submitted, ControlError> {
    let shown = answerable(journal, owner, agent, approval, now)?;
    let payload = response(owner, agent, &shown, "skipped", Value::Null, now)?;
    commit(
        journal,
        ids,
        owner,
        "ApprovalResponseSubmitted",
        payload,
        now,
    )
}

/// What the runtime recorded for a submitted answer: the `ApprovalResponded` whose `causation_id`
/// is it, and the `ApprovalRevalidated` that followed an admitted grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing recorded yet. Never reported as approved (rule 3).
    NotRecorded,
    Admitted {
        revalidation: Option<Revalidated>,
    },
    Counted,
    Refused {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revalidated {
    Act,
    Skip { reason: String },
}

/// # Errors
/// [`ControlError::Journal`] when the journal cannot be read.
pub fn outcome(
    journal: &dyn ControlJournal,
    owner: &Owner,
    agent: &str,
    submitted: &Submitted,
) -> Result<Outcome, ControlError> {
    let events = envelopes(journal, &agent_stream(owner, agent)?)?;
    let Some((at, copy)) = events.iter().enumerate().find(|(_, e)| {
        member_text(e, "event_type") == Some("ApprovalResponded")
            && member_text(e, "causation_id") == Some(submitted.event_id.as_str())
    }) else {
        return Ok(Outcome::NotRecorded);
    };
    let reason = || {
        member_text(copy, "payload.reason")
            .unwrap_or_default()
            .to_owned()
    };
    Ok(match member_text(copy, "payload.result") {
        Some("admitted") => {
            let approval = member_text(copy, "payload.approval");
            let revalidation = events
                .iter()
                .skip(at)
                .find(|e| {
                    member_text(e, "event_type") == Some("ApprovalRevalidated")
                        && member_text(e, "payload.approval") == approval
                })
                .map(|e| match member_text(e, "payload.result") {
                    Some("act") => Revalidated::Act,
                    _ => Revalidated::Skip {
                        reason: member_text(e, "payload.reason")
                            .unwrap_or_default()
                            .to_owned(),
                    },
                });
            Outcome::Admitted { revalidation }
        }
        Some("counted") => Outcome::Counted,
        _ => Outcome::Refused { reason: reason() },
    })
}

/// The line `approve` prints for an outcome. Only an admitted grant whose re-validation acted says
/// it was sent, and nothing the runtime has not recorded is ever called approved (rule 3, the M7
/// brief's "What `approve` prints").
///
/// # Errors
/// None: every outcome has a line. The `Result` is the stub API's shape.
pub fn message(outcome: &Outcome) -> Result<String, ControlError> {
    Ok(match outcome {
        Outcome::NotRecorded => "Not recorded yet. If the runtime does not record it before the \
                                 deadline, the action is skipped."
            .to_owned(),
        Outcome::Admitted {
            revalidation: Some(Revalidated::Act),
        } => "Admitted and sent to the executor; the gate still decides.".to_owned(),
        Outcome::Admitted {
            revalidation: Some(Revalidated::Skip { reason }),
        } => format!("Admitted, then skipped on re-validation ({reason}); nothing was sent."),
        Outcome::Admitted { revalidation: None } => {
            "Admitted, not yet re-validated; nothing was sent.".to_owned()
        }
        Outcome::Counted => {
            "Counted, short of the approvers required; nothing was sent.".to_owned()
        }
        Outcome::Refused { reason } => format!(
            "Refused ({reason}); nothing was sent. You may answer again before the deadline."
        ),
    })
}
