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
| `cargo xtask ci lint` | shellcheck over `.github/scripts/`, actionlint over `.github/workflows/`, fmt, clippy `-D warnings`, crate layering, the `live` feature (`cargo xtask live-feature`: only the crate `xtask/layers.toml` marks `live_feature` may declare one, and CI compiles it at most once, in `cargo check -p <runner>`; DEC-529), debt markers and plain comments, the feature map, typos, ruff |
| `cargo xtask ci test` | nextest, doctests, pytest; reference cases marked `passing` in `status.toml` |
| `cargo xtask ci pending` | Every test in the workspace marked `#[ignore = "pending <story>"]` fails on the change's code |
| `cargo xtask ci mutants` | cargo-mutants on the changed source of every crate `xtask/layers.toml` marks `safety_critical = true`, the `tool`-layer `mandate-refcases` harness included (DEC-253): every mutant caught. A mutant is judged only by the tests the run executes. Those are the mutated packages' own plus, whenever a mutated package is one `mandate-refcases` is built on, the whole reference harness, so an oracle that lives outside the crate it covers still judges it (DEC-497). The reach comes from cargo's dependency graph, so a new suite is an oracle the moment it exists |
| `cargo xtask ci spec-guard` | Protected paths cite a DEC and ship without code, diffed from the merge base with `origin/main` (on a CI `pull_request` run, the merge commit's first parent); set `MANDATE_BASE_REF` to check one commit range |
| `cargo xtask refcases` | `fixtures/refcases/` matches the reference-case YAML |
| `cargo xtask layers`, `cargo xtask deps` | Dependency directions and the dependency registry |
| `cargo test -p mandate-refcases -- --include-ignored` | Pending reference cases, before a status change marks them passing |

CI sets `MANDATE_MUTANT_SHARD=INDEX/TOTAL` on each deterministic mutation shard. Do not set it for
local proof: `cargo xtask check` and `cargo xtask ci mutants` must cover the complete diff locally.
The required `full` check aggregates every shard with `cargo xtask ci full`, which covers fixture
drift, reference, supply-chain, and PostgreSQL checks (DEC-464).

The toolchain comes from `.cursor/install.sh` (idempotent, pinned, checksum-verified).

## The feature map

[`features/`](features/README.md) holds one file a feature, each with its spec anchor, code,
tests, reference cases, and the command that exercises it. `cargo xtask feature-map --index` lists
them by title; read the ones that cover the code you are about to change. When you add a crate, a
reference-case suite, or a feature, add or edit its feature's file in the same change, and only
that file: a new feature is a new file `features/<slug>.md` opening with its `# ` title, and there
is no list to update. `cargo xtask ci lint` fails when a crate or suite is named in no feature, a
listed path does not exist, or a feature file has no title.

## Evidence rules

1. Run the narrowest command that exercises the change, then `cargo xtask check`.
2. Report each claim with the command and the result line that proves it (test counts, mutants
   caught and missed, the failing case). Label anything you did not run as unverified.
3. A test only counts if it fails without the change. For a new check or oracle, plant the bug it
   should catch, show it failing, and revert.
4. Safety-critical code: `cargo xtask ci mutants` with zero missed mutants. An equivalent mutant is
   excluded only in `.cargo/mutants.toml`, with its reason, which the review agent checks.
5. Changes to the reference-case harness (`mandate-refcases`) are proven by seeding bugs in the
   code it tests and showing the expected case fails. The harness is safety-critical, so the
   mutation gate also runs on its changed source (DEC-253): a harness line no test can fail is a
   case that passes while checking nothing.
