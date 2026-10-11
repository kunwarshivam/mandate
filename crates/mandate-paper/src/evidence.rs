//! The exportable evidence a placing run leaves (E7-26, the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s G1b; the founder's run's step 6;
//! journal spec §2, §12): the agent, account and control streams of the journal the run
//! appended to, named as the journal names them, so `mandate journal export` reads each one
//! and `mandate journal verify` checks it. The runner exports nothing itself: an export is
//! journaled before it is served (`ExportCreated`, journal spec §9.13, API-16), which is the
//! CLI's door and never the runner's ([DEC-912](../../../docs/project/decisions/DEC-912.md)
//! item 4), and no stream name or code path here reaches a host or names the journal's DSN
//! (ES-19, `AGENTS.md` rule 7).

/// The streams the run's `journal export` reads ([DEC-912](../../../docs/project/decisions/DEC-912.md)
/// item 4): the three of the founder's run's step 6, in its order — the agent stream, the
/// account stream, the workspace control stream — named as journal spec §2 names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub streams: [String; 3],
}

/// Why the run names no export evidence ([DEC-912](../../../docs/project/decisions/DEC-912.md)
/// item 4): the refusal is a stop with nothing exported, with its own reason code
/// (ADR-0001 ES-09).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceRefusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// The run kept no journal — the dry run, which has no `--journal` — so it wrote nothing
    /// the export could read and names no stream.
    #[error("the run kept no journal, so no stream exports")]
    JournalAbsent,
}

impl EvidenceRefusal {
    /// The stable reason code of each refusal (ADR-0001 ES-09): `export_journal_absent`, and
    /// the stub's `unimplemented`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::JournalAbsent => "export_journal_absent",
        }
    }
}

/// The run's exportable evidence ([DEC-912](../../../docs/project/decisions/DEC-912.md) item 4):
/// the three streams of the journal the placing run appended to, in the founder's run's step 6
/// order — `agent:{workspace}:{agent}`, then `acct:{workspace}:{account_ref}`, then
/// `ctl:{workspace}` — derived from the run's own workspace, agent and account ref, so
/// `mandate journal export` takes each name as it is. A run with no journal wrote nothing, keeps
/// no evidence, and refuses: the dry run of the founder's run's step 4 exports nothing. The
/// evidence names streams only, never the journal's DSN (rule 7).
///
/// # Errors
/// [`EvidenceRefusal::JournalAbsent`] when the run kept no journal.
pub fn evidence(
    journal: Option<&str>,
    workspace: &str,
    agent: &str,
    account_ref: &str,
) -> Result<Evidence, EvidenceRefusal> {
    if journal.is_none() {
        return Err(EvidenceRefusal::JournalAbsent);
    }
    Ok(Evidence {
        streams: [
            format!("agent:{workspace}:{agent}"),
            format!("acct:{workspace}:{account_ref}"),
            format!("ctl:{workspace}"),
        ],
    })
}
