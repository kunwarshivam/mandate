//! `mandate workspace open` (first paper trade brief, D1c; DEC-527 item 7): commits the workspace
//! control stream's `StreamOpened`, once, as the journal vectors' `control_services` opener, in
//! `paper` or, with `--environment live`, in `live` (DEC-952). The stream's first event fixes its
//! environment, so a live workspace is its own workspace, and every other control command appends
//! to the stream this opens in its environment.
//!
//! `mandate workspace found` (lane ID1, DEC-991) writes the workspace's founding grant once: the
//! `MemberActivated` with reason `founding` that makes its owner a member with the owner bundle
//! (identity spec §3.2, §4.3, ID-13; journal spec §9.12), through the one membership writer,
//! `mandate_identity::found_workspace`. It is idempotent, so a Kubernetes Job may run it again:
//! a re-run for the same member finds the grant, commits nothing, and succeeds. With `--open` it
//! first opens a stream that holds no event, in `paper`, so a staging bootstrap Job founds a new
//! workspace in one idempotent step (DEC-2114); a stream holding anything is never opened again.

use std::io::Write;

use clap::{Args, Subcommand};

use mandate_canon::Value;
use mandate_identity::{
    Appended, ControlStream, ControlView, Founded as IdentityFounded, FoundingError,
    FoundingRefusal, MembershipEvent, MembershipRecord, PrincipalId, WriteError, control_entry,
    found_workspace,
};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_time::UtcNanos;

use crate::control::{
    ControlError, ControlJournal, Now, Owner, Submitted, backoff, control_stream, envelope,
    founding_draft, object, open_control_stream, text,
};
use crate::postgres::{JournalArgs, PgControlJournal, read_stream};

#[derive(Debug, Subcommand)]
pub enum WorkspaceCommand {
    /// Open the workspace control stream, in paper or live. Refused if the stream holds any event.
    Open(OpenArgs),
    /// Write the workspace's founding grant: its owner's membership with the owner bundle, once.
    /// A re-run for the same member commits nothing and succeeds; any other membership refuses it.
    Found(FoundArgs),
}

/// There is no `--user` and no role: the system writes the grant, and its roles are the owner
/// bundle, never chosen.
#[derive(Debug, Args)]
pub struct FoundArgs {
    /// The workspace whose control stream to write the founding grant on.
    #[arg(long, value_name = "ID")]
    pub workspace: String,
    /// The founder's user principal: an opaque ULID (identity spec §3.1), never a name, an
    /// address, or an identity-provider subject.
    #[arg(long, value_name = "ULID")]
    pub member: String,
    /// How the founder signs in: `passkey`, `oidc`, or `email_link` (identity spec §6.1).
    #[arg(long, value_name = "METHOD")]
    pub method: String,
    /// Open the control stream first, in `paper`, when it holds no event (DEC-2114).
    #[arg(long)]
    pub open: bool,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// What `found` asks for, as given.
#[derive(Debug, Clone, Copy)]
pub struct Founding<'a> {
    pub workspace: &'a str,
    pub member: &'a str,
    pub method: &'a str,
    /// Whether a stream holding no event is opened, in `paper`, before the grant.
    pub open: bool,
}

/// What `found` did: the `seq` of the founding grant, and whether this run committed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Founded {
    pub seq: u64,
    pub granted: bool,
}

/// There is no `--user`: the event's actor is the `control_services` opener, not the owner.
#[derive(Debug, Args)]
pub struct OpenArgs {
    /// The workspace whose control stream to open.
    #[arg(long, value_name = "ID")]
    pub workspace: String,
    /// `paper`, the default, or `live`: the workspace's environment, for life (DEC-952).
    #[arg(long, value_name = "ENV", default_value = "paper")]
    pub environment: String,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// Commits `StreamOpened` on `ctl:{workspace}` and prints the event id and `seq` it committed.
///
/// # Errors
/// A refusal whose message carries its code: `environment_invalid` for any environment but
/// `paper` or `live`, then `owner_workspace_invalid`, both before the journal is
/// opened or the store created, then `workspace_already_open` for a stream that holds any event,
/// before anything is appended, or the journal's error; no message names the DSN, and a refusal
/// prints nothing.
pub fn open(args: &OpenArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Submitted> {
    let refused = |reason| ControlError::Refused { reason };
    let environment = match args.environment.as_str() {
        "paper" => Environment::Paper,
        "live" => Environment::Live,
        _ => return Err(refused("environment_invalid").into()),
    };
    let owner = Owner {
        workspace: args.workspace.clone(),
        user: String::new(),
        environment,
    };
    let stream = control_stream(&owner).map_err(|_| refused("owner_workspace_invalid"))?;
    if !read_stream(&args.target.journal, &stream)?.is_empty() {
        return Err(refused("workspace_already_open").into());
    }
    let mut journal = PgControlJournal::open(&args.target)?;
    let submitted = open_control_stream(&mut journal, &owner, now)?;
    let Submitted { event_id, seq } = &submitted;
    writeln!(
        report,
        "opened {} as event {event_id} at seq {seq}",
        stream.as_str()
    )?;
    Ok(submitted)
}

/// Writes `args`' founding grant on the workspace's Postgres journal, reading the clock with
/// `clock` inside each attempt, and prints what it did.
///
/// # Errors
/// As [`found_in`], and the journal's error opening it; no message names the DSN.
pub fn found(
    args: &FoundArgs,
    clock: &mut dyn FnMut() -> UtcNanos,
    report: &mut impl Write,
) -> anyhow::Result<Founded> {
    let founding = Founding {
        workspace: &args.workspace,
        member: &args.member,
        method: &args.method,
        open: args.open,
    };
    checked(founding)?;
    let mut journal = PgControlJournal::open(&args.target)?;
    Ok(found_in(&mut journal, founding, clock, report)?)
}

/// Writes `founding`'s grant on `journal`, reading the clock with `clock` inside each attempt, and
/// prints what it did. A grant found already committed, by an earlier run or by this one's lost
/// answer, is reported with `granted: false`, and nothing is committed.
///
/// # Errors
/// [`ControlError::Refused`] with `method_invalid`, `member_invalid`, or
/// `owner_workspace_invalid` before the journal is read; then `workspace_not_open`,
/// `workspace_not_new`, or `members_exist`, committing nothing, or `membership_out_of_order` when
/// the writer's order rule refuses (journal spec §9.12 **Order**); [`ControlError::Journal`] when
/// the journal fails or does not settle.
pub fn found_in(
    journal: &mut dyn ControlJournal,
    founding: Founding<'_>,
    clock: &mut dyn FnMut() -> UtcNanos,
    report: &mut dyn Write,
) -> Result<Founded, ControlError> {
    let (method, member) = checked(founding)?;
    let mut owner = Owner {
        workspace: founding.workspace.to_owned(),
        user: String::new(),
        environment: Environment::Paper,
    };
    let stream = control_stream(&owner)?;
    let mut rows = journal.rows(&stream)?;
    if founding.open && rows.is_empty() {
        rows = opened(journal, &owner, &stream, clock, report)?;
    }
    let Some(opening) = rows.first() else {
        return Err(refused(FoundingRefusal::NotOpen.code()));
    };
    if opening.event_type != "StreamOpened" {
        return Err(ControlError::Journal(
            "the control stream's first event is not its opening".to_owned(),
        ));
    }
    owner.environment = Environment::parse(&opening.environment)
        .ok_or_else(|| ControlError::Journal("the stream's environment".to_owned()))?;
    let mut writer = FoundingStream {
        journal,
        owner: &owner,
        stream: &stream,
        member: founding.member,
        method,
        retries: 0,
    };
    let (record, granted) = match found_workspace(&mut writer, clock, member) {
        Ok(IdentityFounded::Granted(record)) => (record, true),
        Ok(IdentityFounded::AlreadyFounded(record)) => (record, false),
        Err(FoundingError::Refused(why)) => return Err(refused(why.code())),
        Err(FoundingError::Write(WriteError::Refused(_))) => {
            return Err(refused("membership_out_of_order"));
        }
        Err(FoundingError::Write(WriteError::Store(error))) => return Err(error),
        Err(FoundingError::Write(WriteError::Contended | WriteError::StreamFull)) => {
            return Err(ControlError::Journal(
                "the control stream did not settle; running it again finds the grant if it \
                 was committed"
                    .to_owned(),
            ));
        }
    };
    let seq = record.seq();
    let line = if granted {
        format!(
            "founded {} for member {} at seq {seq}",
            stream.as_str(),
            founding.member
        )
    } else {
        format!(
            "{} is already founded for member {} at seq {seq}; nothing was committed",
            stream.as_str(),
            founding.member
        )
    };
    writeln!(report, "{line}").map_err(|e| ControlError::Journal(format!("the report: {e}")))?;
    Ok(Founded { seq, granted })
}

/// Opens `stream`, which held no event, as `owner`'s `paper` control stream at the clock's
/// reading, prints it, and reads the stream again. A concurrent opening is the same event, derived
/// at head 0, so it settles as found rather than opening twice.
///
/// # Errors
/// As [`open_control_stream`], and the journal's error reading the stream.
fn opened(
    journal: &mut dyn ControlJournal,
    owner: &Owner,
    stream: &StreamId,
    clock: &mut dyn FnMut() -> UtcNanos,
    report: &mut dyn Write,
) -> Result<Vec<StoredEvent>, ControlError> {
    let at = clock();
    let Submitted { event_id, seq } = open_control_stream(
        journal,
        owner,
        Now {
            at,
            secs: at.secs(),
        },
    )?;
    writeln!(
        report,
        "opened {} as event {event_id} at seq {seq}",
        stream.as_str()
    )
    .map_err(|e| ControlError::Journal(format!("the report: {e}")))?;
    journal.rows(stream)
}

const METHODS: [&str; 3] = ["passkey", "oidc", "email_link"];

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

/// The method and member `founding` names, and its workspace's control stream, checked before
/// any journal is opened.
fn checked(founding: Founding<'_>) -> Result<(&'static str, PrincipalId), ControlError> {
    let method = METHODS
        .into_iter()
        .find(|m| *m == founding.method)
        .ok_or_else(|| refused("method_invalid"))?;
    let member =
        PrincipalId::from_ulid_text(founding.member).map_err(|_| refused("member_invalid"))?;
    let owner = Owner {
        workspace: founding.workspace.to_owned(),
        user: String::new(),
        environment: Environment::Paper,
    };
    control_stream(&owner).map_err(|_| refused("owner_workspace_invalid"))?;
    Ok((method, member))
}

/// The control stream as the one membership writer's port (DEC-646), over the CLI's journal: it
/// reads every row, so the founding decision sees the stream from its opening, and appends the
/// founding grant's draft at the head the attempt read. A fence, an unavailable journal, or a lost
/// answer reads as a moved head after a [`backoff`], so the writer re-reads and decides again: a
/// grant that did commit is then found, never written twice. So does an idempotency conflict: the
/// event id is the member's at that head, so a stored event under it is an earlier attempt's grant,
/// stamped at another instant and made visible only after this attempt's read.
struct FoundingStream<'a> {
    journal: &'a mut dyn ControlJournal,
    owner: &'a Owner,
    stream: &'a StreamId,
    member: &'a str,
    method: &'static str,
    retries: u32,
}

impl ControlStream for FoundingStream<'_> {
    type Error = ControlError;

    fn read(&mut self) -> Result<ControlView, ControlError> {
        let rows = self.journal.rows(self.stream)?;
        let head = rows.last().map_or(0, |row| row.seq);
        let tail = rows
            .iter()
            .map(|row| {
                let value = envelope(row)?;
                control_entry(row.seq, &value).map_err(|e| ControlError::Journal(e.to_string()))
            })
            .collect::<Result<_, _>>()?;
        Ok(ControlView { head, tail })
    }

    fn append(
        &mut self,
        expected_head: u64,
        record: &MembershipRecord,
    ) -> Result<Appended, ControlError> {
        let payload = founding_payload(record, self.member, self.method)?;
        let at = record.event_time();
        let (event_id, bytes) =
            founding_draft(self.owner, self.member, payload, at, expected_head)?;
        let epoch = self.journal.take_ownership(self.stream)?;
        match self
            .journal
            .append(self.stream, expected_head, epoch, at, &[&bytes])?
        {
            AppendOutcome::Committed(rows) | AppendOutcome::AlreadyCommitted(rows)
                if rows.iter().any(|row| row.event_id == event_id) =>
            {
                Ok(Appended::Committed)
            }
            AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_) => Err(
                ControlError::Journal("the stored event is missing".to_owned()),
            ),
            AppendOutcome::HeadMismatch { .. }
            | AppendOutcome::Fenced { .. }
            | AppendOutcome::Ambiguous
            | AppendOutcome::Unavailable
            | AppendOutcome::IdempotencyConflict { .. } => {
                self.retries = self.retries.saturating_add(1);
                self.journal.wait(backoff(self.retries, &event_id)?);
                Ok(Appended::HeadMoved)
            }
            AppendOutcome::Invalid { error, .. } => {
                Err(ControlError::Journal(format!("refused: {error:?}")))
            }
        }
    }
}

/// The founding grant's §9.12 payload: `record`'s activation, which must be `member`'s founding
/// grant, with the given sign-in method, no invitation, and no session (rules 99 and 105).
///
/// # Errors
/// [`ControlError::Journal`] for any other record, which this writer never writes.
fn founding_payload(
    record: &MembershipRecord,
    member: &str,
    method: &str,
) -> Result<Value, ControlError> {
    let named = PrincipalId::from_ulid_text(member).map_err(|_| refused("member_invalid"))?;
    let MembershipEvent::Activated {
        member: activated,
        invitation: None,
        roles,
        independent_approval_required,
        cool_off_ends_at,
    } = record.event()
    else {
        return Err(ControlError::Journal(
            "the founding writer writes only a founding grant".to_owned(),
        ));
    };
    if *activated != named {
        return Err(ControlError::Journal(
            "the founding grant names another member".to_owned(),
        ));
    }
    let mut codes = roles
        .iter()
        .map(|role| {
            role.code()
                .ok_or_else(|| ControlError::Journal("an org role".to_owned()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    codes.sort_unstable();
    object(vec![
        ("member", text(member)),
        ("invitation", Value::Null),
        ("reason", text("founding")),
        ("roles", Value::Array(codes.into_iter().map(text).collect())),
        ("method", text(method)),
        ("activated_at", text(&record.event_time().to_string())),
        (
            "independent_approval_required",
            Value::Bool(*independent_approval_required),
        ),
        ("cool_off_ends_at", text(&cool_off_ends_at.to_string())),
        ("session_ref", Value::Null),
    ])
}
