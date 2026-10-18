//! Where an executor process learns its account (DEC-2007 item 3): from `run`'s flags, or from the
//! variables the deployment manager sets on the pod it starts (E21-33, design §3.5), ids only and
//! never a secret.
//!
//! The manager sets `MANDATE_PROCESS` (`executor` here), `MANDATE_WORKSPACE`, `MANDATE_SUBJECT`
//! (the account's `account_ref`) and `MANDATE_GENERATION`, which only the manager reads: the account
//! stream's writer epoch is what fences an earlier process (DEC-2003). A flag never overrides a
//! variable the manager set, and one that disagrees with it is refused rather than guessed between
//! (`AGENTS.md` rule 3).

/// The account a launch names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub workspace: String,
    pub account: String,
}

/// The account `flag` (the value after a flag) and `var` (an environment variable, `Some` for any
/// value it holds, unreadable ones included) name, or the refusal's code: `process_not_executor`
/// for a pod started for another process, `account_missing` for no workspace or account, and
/// `account_conflict` for a flag that disagrees with the manager's variable, or a manager variable
/// set without `MANDATE_PROCESS`. A value that is empty or has whitespace around it names nothing.
///
/// # Errors
/// The refusal codes above.
pub fn named(
    flag: &dyn Fn(&str) -> Option<String>,
    var: &dyn Fn(&str) -> Option<String>,
) -> Result<Named, &'static str> {
    let _ = (flag, var);
    Err("unimplemented")
}

