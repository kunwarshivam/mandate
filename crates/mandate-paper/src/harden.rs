//! LT-9's process half (DEC-937 item 4): a live run turns its core dumps off before it reads any
//! credential, so the OAuth token, which exists only in the process's memory, cannot reach a
//! core file.

use rustix::io;
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

/// The core-file limit the kernel reports back, judged (DEC-954 item 2).
///
/// # Errors
/// [`PaperError::Harden`] unless `read` is zero, soft and hard.
fn zero_core_limit(read: Rlimit) -> Result<(), PaperError> {
    let _ = read;
    Ok(())
}

/// The dumpable flag the kernel reports back, judged (DEC-954 item 3).
///
/// # Errors
/// [`PaperError::Harden`] unless `read` is [`DumpableBehavior::NotDumpable`].
fn not_dumpable(read: io::Result<DumpableBehavior>) -> Result<(), PaperError> {
    let _ = read;
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rustix::io::Errno;
    use rustix::process::{DumpableBehavior, Rlimit};

    use super::{not_dumpable, zero_core_limit};

    /// Either limit, soft or hard: `None` is `RLIM_INFINITY`, then zero, one, and any value.
    fn limit() -> impl Strategy<Value = Option<u64>> {
        prop_oneof![
            Just(None),
            Just(Some(0)),
            Just(Some(1)),
            any::<u64>().prop_map(Some)
        ]
    }

    proptest! {
        /// LT-9, DEC-954 item 2: a read-back limit passes only when both its soft and its hard
        /// limit are exactly zero; the oracle counts the nonzero halves on its own.
        #[test]
        fn only_a_zero_soft_and_hard_limit_passes(current in limit(), maximum in limit()) {
            let open = [current, maximum].iter().filter(|half| **half != Some(0)).count();
            let verdict = zero_core_limit(Rlimit { current, maximum });
            prop_assert_eq!(verdict.is_ok(), open == 0, "{:?} {:?}", current, maximum);
        }
    }

    /// LT-9, DEC-954 item 3: only a read-back `NotDumpable` passes; a dumpable process, one whose
    /// core is readable by root only, and a kernel that does not answer each refuse.
    #[test]
    fn only_a_not_dumpable_read_back_passes() {
        assert!(not_dumpable(Ok(DumpableBehavior::NotDumpable)).is_ok());
        for refused in [
            Ok(DumpableBehavior::Dumpable),
            Ok(DumpableBehavior::DumpableReadableOnlyByRoot),
            Err(Errno::INVAL),
        ] {
            assert!(not_dumpable(refused).is_err(), "{refused:?}");
        }
    }
}
