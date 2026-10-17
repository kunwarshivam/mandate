//! `mandate identity link` (lane ID1, DEC-992): writes one line of the workspace API server's
//! subject-link file ([workspace API spec](../../../docs/specs/workspace-api.md) §3.11 item 7,
//! DEC-978 item 7), which maps the digest of an identity provider's `{issuer, subject}` to the
//! user principal it signs in as.
//!
//! The subject is read from standard input and never written anywhere: not to the file, the
//! output, an error, or an argument list a process listing shows (identity spec §3.1, §3.3). Only
//! its digest, `sha256:` and the SHA-256 of the canonical `{"issuer","subject"}` object, reaches
//! the file. The file is replaced whole, by a rename, so the server never reads a partial one; it
//! is written owner-writable and readable by all (`0644`), never group- or world-writable, and a
//! file the server would refuse is never extended.

use std::io::{Read, Write};
use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::control::ControlError;

#[derive(Debug, Subcommand)]
pub enum IdentityCommand {
    /// Link an identity provider's subject, read from standard input, to a user principal in the
    /// API server's subject-link file. A link already there commits nothing and succeeds.
    Link(LinkArgs),
}

/// There is no `--subject`: the subject comes from standard input, so no process listing or shell
/// history shows it.
#[derive(Debug, Args)]
pub struct LinkArgs {
    /// The identity provider's issuer, exactly as its tokens carry it in `iss`.
    #[arg(long, value_name = "URL")]
    pub issuer: String,
    /// The user principal the subject signs in as: an opaque ULID (identity spec §3.1).
    #[arg(long, value_name = "ULID")]
    pub member: String,
    /// The subject-link file the API server reads (`MANDATE_API_SUBJECT_LINKS`).
    #[arg(long, value_name = "PATH")]
    pub links: PathBuf,
}

/// What a link did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    /// The subject's digest, as the file holds it.
    pub digest: String,
    /// Whether this run wrote it; `false` when the same link was already there.
    pub written: bool,
}

fn unimplemented() -> ControlError {
    ControlError::Unimplemented { story: "ID1" }
}

/// The digest the file names a subject by.
///
/// # Errors
/// Unimplemented.
pub fn subject_digest(issuer: &str, subject: &str) -> Result<String, ControlError> {
    let _ = (issuer, subject);
    Err(unimplemented())
}

/// Links the subject read from `subject` as `args` say.
///
/// # Errors
/// Unimplemented.
pub fn link(
    args: &LinkArgs,
    subject: &mut dyn Read,
    report: &mut dyn Write,
) -> Result<Linked, ControlError> {
    let _ = (args, subject, report);
    Err(unimplemented())
}
