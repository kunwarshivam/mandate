- **CI-pending-gate-target-dir (Proposed; debt; lane RN)** As an agent, I want the pending-test
  gate's fixture runs (`pending_nextest` with `BuildDir::OwnTree`, in `xtask/src/main.rs`) to pin
  `CARGO_TARGET_DIR` to the fixture's own absolute `target/` and drop `CARGO_BUILD_TARGET_DIR`, as
  the mutants job's builds do since #2583, so that a caller's `CARGO_BUILD_TARGET_DIR` or
  `build.target-dir` cannot make concurrent fixtures share one directory. *Accepted when:* a test
  runs the gate with `CARGO_BUILD_TARGET_DIR` exported and finds nothing built there.
