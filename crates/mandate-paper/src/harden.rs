//! LT-9's process half (DEC-932 item 4): a live run turns its core dumps off before it reads any
//! credential, so the OAuth token, which exists only in the process's memory, cannot reach a
//! core file.

use rustix::process::{DumpableBehavior, Resource, Rlimit, set_dumpable_behavior, setrlimit};

use crate::PaperError;

/// Sets the process's core-file limit to zero, soft and hard, and marks it not dumpable, which
/// also stops a core-dump pipe handler and a same-user `ptrace` attach from reading its memory.
///
/// # Errors
/// [`PaperError::Harden`] if either call fails; a live run then reads no credential.
pub fn disable_core_dumps() -> Result<(), PaperError> {
    let none = Rlimit {
        current: Some(0),
        maximum: Some(0),
    };
    setrlimit(Resource::Core, none).map_err(|_| PaperError::Harden)?;
    set_dumpable_behavior(DumpableBehavior::NotDumpable).map_err(|_| PaperError::Harden)
}
