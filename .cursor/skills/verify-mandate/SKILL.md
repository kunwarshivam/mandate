---
name: verify-mandate
description: Prove a change to the Mandate repository works, with commands whose output is the evidence. Use before claiming any task is done, when asked "does it work", "prove it", or "verify", and when you need to know which tests and reference cases cover a feature.
---

# Verify Mandate

A claim that something works needs a command that shows it, run in this session, with its result
in your reply. "It compiles" and "the tests I wrote pass" are not evidence on their own.

## The CLI

`cargo xtask` is the only verification CLI. Do not write one-off scripts for anything it covers;
if it lacks something you need twice, add a subcommand (the `correction` playbook of
`mandate-mode`).

| Command | Proves |
|---|---|
| `cargo xtask check` | Every per-PR job, as CI runs them. Required before proposing any change |
| `cargo xtask ci lint` | fmt, clippy `-D warnings`, crate layering, debt markers, the feature map, typos, ruff |
| `cargo xtask ci test` | nextest, doctests, pytest; reference cases marked `passing` in `status.toml` |
| `cargo xtask ci mutants` | cargo-mutants on the changed source of safety-critical crates: every mutant caught |
| `cargo xtask ci spec-guard` | Protected paths cite a DEC and ship without code (set `MANDATE_BASE_REF` to check one commit range) |
| `cargo xtask refcases` | `fixtures/refcases/` matches the reference-case YAML |
| `cargo xtask layers`, `cargo xtask deps` | Dependency directions and the dependency registry |
| `cargo test -p mandate-refcases -- --include-ignored` | Pending reference cases, before a status change marks them passing |

The toolchain comes from `.cursor/install.sh` (idempotent, pinned, checksum-verified).

## The feature map

[`feature-map.md`](feature-map.md) lists each feature with its spec anchor, code, tests, reference
cases, and the command that exercises it. Read it to find what covers the code you are about to
change. When you add a crate, a reference-case suite, or a feature, add its entry in the same
change: `cargo xtask ci lint` fails when a crate or suite is missing or a listed path does not
exist.

## Evidence rules

1. Run the narrowest command that exercises the change, then `cargo xtask check`.
2. Report each claim with the command and the result line that proves it (test counts, mutants
   caught and missed, the failing case). Label anything you did not run as unverified.
3. A test only counts if it fails without the change. For a new check or oracle, plant the bug it
   should catch, show it failing, and revert.
4. Safety-critical code: `cargo xtask ci mutants` with zero missed mutants. An equivalent mutant is
   excluded only in `.cargo/mutants.toml`, with its reason, which the review agent checks.
5. Changes to the reference-case harness (`mandate-refcases`) are proven by seeding bugs in the
   code it tests and showing the expected case fails; mutating the harness itself proves nothing.
