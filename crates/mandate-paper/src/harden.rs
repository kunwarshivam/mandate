//! LT-9's process half (DEC-937 item 4, DEC-954): a live run turns its core dumps off before it
//! reads any credential, so the OAuth token, which exists only in the process's memory, cannot
//! reach a core file.
//!
//! The core-file limit is set wherever `rustix` gives `setrlimit` and `getrlimit`: every unix
//! target but the few it leaves them out on (ESP-IDF, Fuchsia, Horizon, Redox, Vita), so macOS
//! and Linux both. The dumpable flag is Linux's alone. Any other target refuses, so the runner
//! builds everywhere and a live run there reads no credential.

#[cfg(any(target_os = "linux", target_os = "android"))]
use rustix::io;
#[cfg(any(target_os = "linux", target_os = "android"))]
use rustix::process::{DumpableBehavior, dumpable_behavior, set_dumpable_behavior};
#[cfg(all(
    unix,
    not(any(
        target_os = "espidf",
        target_os = "fuchsia",
        target_os = "horizon",
        target_os = "redox",
        target_os = "vita"
    ))
))]
use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

use crate::PaperError;

/// Sets the process's core-file limit to zero, soft and hard, and reads it back; on Linux it
/// also marks the process not dumpable and reads that back, which stops a core-dump pipe
/// handler and a same-user `ptrace` attach from reading its memory as well (DEC-954). macOS has
/// no dumpable flag, and the zero limit is what stops its core files.
///
/// # Errors
/// [`PaperError::Harden`] if a call fails, if the kernel reports back anything but what was
/// set, or on a target with no core-file limit; a live run then reads no credential (LT-9).
pub fn disable_core_dumps() -> Result<(), PaperError> {
    #[cfg(not(all(
        unix,
        not(any(
            target_os = "espidf",
            target_os = "fuchsia",
            target_os = "horizon",
            target_os = "redox",
            target_os = "vita"
        ))
    )))]
    return Err(PaperError::Harden);
    #[cfg(all(
        unix,
        not(any(
            target_os = "espidf",
            target_os = "fuchsia",
            target_os = "horizon",
            target_os = "redox",
            target_os = "vita"
        ))
    ))]
    {
        let none = Rlimit {
            current: Some(0),
            maximum: Some(0),
        };
        setrlimit(Resource::Core, none).map_err(|_| PaperError::Harden)?;
        zero_core_limit(getrlimit(Resource::Core))?;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            set_dumpable_behavior(DumpableBehavior::NotDumpable).map_err(|_| PaperError::Harden)?;
            not_dumpable(dumpable_behavior())?;
        }
        Ok(())
    }
}

/// The core-file limit the kernel reports back, judged (DEC-954 item 2).
///
/// # Errors
/// [`PaperError::Harden`] unless `read` is zero, soft and hard.
#[cfg(all(
    unix,
    not(any(
        target_os = "espidf",
        target_os = "fuchsia",
        target_os = "horizon",
        target_os = "redox",
        target_os = "vita"
    ))
))]
fn zero_core_limit(read: Rlimit) -> Result<(), PaperError> {
    if read.current == Some(0) && read.maximum == Some(0) {
        Ok(())
    } else {
        Err(PaperError::Harden)
    }
}

/// The dumpable flag the kernel reports back, judged (DEC-954 item 3).
///
/// # Errors
/// [`PaperError::Harden`] unless `read` is [`DumpableBehavior::NotDumpable`].
#[cfg(any(target_os = "linux", target_os = "android"))]
fn not_dumpable(read: io::Result<DumpableBehavior>) -> Result<(), PaperError> {
    match read {
        Ok(DumpableBehavior::NotDumpable) => Ok(()),
        _ => Err(PaperError::Harden),
    }
}

#[cfg(all(
    test,
    unix,
    not(any(
        target_os = "espidf",
        target_os = "fuchsia",
        target_os = "horizon",
        target_os = "redox",
        target_os = "vita"
    ))
))]
mod tests {
    use proptest::prelude::*;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    use rustix::io::Errno;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    use rustix::process::DumpableBehavior;
    use rustix::process::Rlimit;

    #[cfg(any(target_os = "linux", target_os = "android"))]
    use super::not_dumpable;
    use super::zero_core_limit;

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
    #[cfg(any(target_os = "linux", target_os = "android"))]
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
