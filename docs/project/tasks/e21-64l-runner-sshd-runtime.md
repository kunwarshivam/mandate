# Task: E21-64l create the SSH runtime before runner boot validation

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15).

## Story

- **Story:** E21-64l ([backlog row](../backlog/E21-64l-runner-sshd-runtime.md))
- **Acceptance criteria (verbatim):** The boot script creates root-owned `/run/sshd` at mode 0755 before `sshd -t`; an executable plan test proves the ordering and fails without it; the founder closes the debug window and repeats the one-runner canary before setting `CI_RUNNER`.
- **PRD / HLD / spec anchors:** PRD FR-9.1; HLD §3 workspace deployment; ADR-0006 item 5.
- **Decisions that apply:** DEC-2065, DEC-2077, DEC-2078.

## Scope

- **Reference cases that must move from pending to passing:** none.
- **Invariants touched:** runner bootstrap fails closed before registration; the debug window remains one `/32` and is removed after diagnosis.
- **Crates in scope:** none; `deploy/tofu/modules/ci-runners`, its plan tests, and CI-runner documentation.
- **Crates out of scope:** trading, accounting, risk, execution, connectors, hosted workspace infrastructure and every application crate.
- **New dependencies allowed:** none.
- **Safety-critical:** yes, because the CI host carries a root-only GitHub App key and executes repository jobs; this defect fix changes no credential, admission or isolation rule and follows tests-first in one small PR.
- **Size budget:** 15 non-generated lines excluding task, backlog, decision and tracker records.

## Commands

```bash
tofu -chdir=deploy/tofu/modules/ci-runners test
tofu -chdir=deploy/tofu/environments/ci-runners test
python3 -m unittest discover -s deploy/tofu/modules/ci-runners/tests -p manager_test.py
cargo xtask ci tofu
cargo xtask check
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if the fix changes SSH reachability, weakens the one-address debug fence, exposes a credential, needs a dependency, weakens or skips a test, or deviates from DEC-2065, DEC-2077 or DEC-2078.

## Definition of done

- [x] A plan test reproduced the live cloud-init failure before implementation.
- [x] The boot script creates the volatile SSH runtime before validating sshd.
- [x] Focused OpenTofu and manager tests pass.
- [x] Documentation records the live finding and keeps the debug window open only through diagnosis.
- [ ] `cargo xtask check` is green.
- [ ] Independent review passes at the exact head and CI is green.
- [ ] The PR merges before the founder removes debug access and repeats the canary.
