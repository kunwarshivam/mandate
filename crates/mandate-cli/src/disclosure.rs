//! `mandate disclosure accept`: the owner's acceptance, on the host, of a disclosure the go-live
//! screen shows (compliance doc, "Risk disclosures accepted before going live"; journal spec §9.2
//! `DisclosureAccepted`; DEC-2134). The document is one of the repository's disclosure files
//! (`fixtures/disclosures/`), every one a placeholder pending counsel (DEC-79). Its version is the
//! SHA-256 of its canonical JSON, stored before the acceptance names it, so the record proves which
//! text was accepted.
//!
//! Like every gesture (DEC-530 item 4), given no `--code` it shows its code and commits nothing;
//! given the code, it commits one `DisclosureAccepted` with `cli_confirm` step-up (V-005 counts only
//! an acceptance with step-up). Accepting a disclosure moves no money and changes no limit.

use std::io::Write;

use clap::{Args, Subcommand};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::ArtifactStore;
use mandate_time::UtcNanos;

use crate::control::{
    Confirmation, ControlError, ControlJournal, Decision, Ids, Now, Owner, Repeat, Shape,
    Submitted, code_of, commit_choice, decide, object, text,
};
use crate::gestures::GestureOwnerArgs;
use crate::inbox::InstantIds;
use crate::postgres::JournalArgs;
use crate::register::open;
use crate::version::refused;

/// The record an acceptance commits.
const ACCEPTED: &str = "DisclosureAccepted";

/// The disclosures the host can accept, by document id, each as its file holds it.
const DOCUMENTS: [(&str, &str); 1] = [(
    "go_live_risk",
    include_str!("../../../fixtures/disclosures/go_live_risk.json"),
)];

#[derive(Debug, Subcommand)]
pub enum DisclosureCommand {
    /// Show a disclosure's acceptance code, or accept it with that code, in paper.
    Accept(AcceptArgs),
}

#[derive(Debug, Args)]
pub struct AcceptArgs {
    /// The document's id, as its file names it: `go_live_risk`.
    pub document: String,
    /// The code `disclosure accept` showed for this document; without it, nothing is committed.
    #[arg(long, value_name = "CODE")]
    pub code: Option<String>,
    #[command(flatten)]
    pub owner: GestureOwnerArgs,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// One disclosure: its id, its canonical bytes, and its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub id: &'static str,
    pub canonical: Vec<u8>,
    pub version: String,
}

/// The disclosure `id` names, or `document_unknown`.
///
/// # Errors
/// `document_unknown` for an id no file holds, or a file that is not JSON naming its own id.
pub fn document(id: &str) -> Result<Document, ControlError> {
    let (id, text) = DOCUMENTS
        .iter()
        .find(|(known, _)| *known == id)
        .ok_or_else(|| refused("document_unknown"))?;
    let parsed = parse(text.as_bytes()).map_err(|_| refused("document_unknown"))?;
    if parsed.get("document").and_then(Value::as_str) != Some(id) {
        return Err(refused("document_unknown"));
    }
    let canonical = to_canonical(&parsed);
    let version = format!("sha256:{}", Digest::of(&canonical).to_hex());
    Ok(Document {
        id,
        canonical,
        version,
    })
}

/// The code accepting `document` takes: bound to its id and version, so a changed text is a new
/// code.
///
/// # Errors
/// As [`object`].
pub fn code(document: &Document) -> Result<String, ControlError> {
    let gesture = object(vec![
        ("document", text(document.id)),
        ("gesture", text("disclosure_accept")),
        ("version", text(&document.version)),
    ])?;
    Ok(code_of(&gesture))
}

/// Accepts `document` as `owner` with `code`: stores the document, then commits one
/// `DisclosureAccepted` naming its version, with `cli_confirm` evidence minted only once the code
/// matched.
///
/// # Errors
/// `code_mismatch` for any other code; [`ControlError::Journal`] when the store or the journal
/// cannot take it.
pub fn accept(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    ids: &mut dyn Ids,
    owner: &Owner,
    (document, typed): (&Document, &str),
    now: Now,
) -> Result<Submitted, ControlError> {
    if typed != code(document)? {
        return Err(refused("code_mismatch"));
    }
    let authenticated_at = UtcNanos::from_parts(now.secs, 0)
        .map_err(|_| ControlError::Journal(format!("the second {}", now.secs)))?;
    let step_up = object(vec![
        ("assertion_id", text(&ids.assertion_id())),
        ("authenticated_at", text(&authenticated_at.to_string())),
        ("method", text("cli_confirm")),
    ])?;
    let key = vec![
        ("document", text(document.id)),
        ("step_up", step_up),
        ("user", text(&owner.user)),
        ("version", text(&document.version)),
    ];
    let decided = match decide(
        journal,
        owner,
        ACCEPTED,
        key,
        Confirmation::Fixed(true),
        Repeat::FindsEarlier,
    )? {
        Decision::Committed(earlier) => return Ok(earlier),
        Decision::Fresh(decided) => decided,
    };
    store
        .put_artifact(&document.canonical)
        .map_err(|e| ControlError::Journal(format!("the artifact store: {}", e.code())))?;
    let shape = Shape {
        schema_version: 1,
        artifact_refs: vec![document.version.clone()],
        config_refs: Vec::new(),
    };
    commit_choice(journal, owner, decided, shape, now)
}

/// Runs `disclosure accept`: without `--code`, prints the document's version and its code and
/// returns `None`; with it, accepts the document and prints the event id and `seq`.
///
/// # Errors
/// The owner flags' refusals, `document_unknown`, then [`accept`]'s.
pub fn run(
    args: &AcceptArgs,
    now: Now,
    report: &mut impl Write,
) -> anyhow::Result<Option<Submitted>> {
    let owner = args.owner.owner()?;
    let document = document(&args.document)?;
    let Some(typed) = &args.code else {
        writeln!(
            report,
            "document {} version {}",
            document.id, document.version
        )?;
        writeln!(report, "code {}", code(&document)?)?;
        return Ok(None);
    };
    let (mut journal, mut store) = open(&args.target)?;
    let mut ids = InstantIds::new(&owner, now);
    let submitted = accept(
        &mut journal,
        &mut store,
        &mut ids,
        &owner,
        (&document, typed),
        now,
    )?;
    let Submitted { event_id, seq } = &submitted;
    writeln!(report, "accepted as event {event_id} at seq {seq}")?;
    Ok(Some(submitted))
}
