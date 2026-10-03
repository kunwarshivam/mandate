# Collaborating on Mandate

This guide is for people with write access to this repository. Most of the code here is written by
AI agents under a merge coordinator, and the repository's rules, checks, and merge flow are built
around that. A person working alongside them follows the same rules, plus a few that exist because
a person can do things the agents cannot.

Read [AGENTS.md](AGENTS.md) first. Its non-negotiable rules bind everyone, people included, and
where this guide and AGENTS.md disagree, AGENTS.md wins.

## How the repository runs

- **Builders.** AI agents, one per stream, each owning a set of crates and documents. A builder
  claims a backlog story on a GitHub issue labeled `claim`, opens one PR per story, and posts
  `@coordinator ready, head <sha>` when CI is green.
- **Reviewers.** Each PR gets an independent review by an agent on a different model from the one
  that wrote it (DEC-79). Safety-critical PRs always get one. The verdict is a PR comment starting
  `**Independent review, round N`.
- **The coordinator.** An agent session acting for the founder. It dispatches reviews, rules on
  their findings, and approves a merge by writing `Coordinator-approved-head: <sha>` into the PR
  description and adding the `coordinator-approved` label (DEC-175).
- **The merge workflow.** [`.github/workflows/merge.yml`](.github/workflows/merge.yml) squash-merges
  a labelled PR once its head matches the approved sha and `fast` and `full` are green. A push after
  approval moves the head and withdraws the approval.
- **The founder** owns the product, the specs, and every decision DEC-79 reserves: live money,
  spending, legal and compliance text, and anything that weakens a safety rule. The founder also
  merges the web UI (`web/`, DEC-200) directly.

GitHub is the only shared state. Claims, decisions, rulings, and handoffs live in issues, PRs, and
the files under `docs/project/`, never in a chat.

## Rules for people

These come on top of AGENTS.md.

1. **Never apply `coordinator-approved`, and never write a `Coordinator-approved-head:` line.**
   Write access lets you do both, and the merge workflow cannot tell who did. Approval belongs to
   the coordinator, after the independent review, and to the founder.
2. **Never click Merge on a PR, and never push to `main`.** Every change reaches `main` through a
   PR, a review, and the label. This includes your own PRs and documentation-only changes.
3. **Do not push to another stream's branch.** Agents' branches start with `agent/` and belong to
   the session that opened them. A push there moves the head under a running review and can
   collide with the builder's next push. Comment on the PR instead.
4. **Claim before you start.** Check `gh issue list --label claim --state open`. A story with an
   open claim is taken. Open your own claim issue, titled `<story ID>: <title>`, before your first
   commit, and say in it that a person holds it.
5. **No real orders, no live credentials.** Use Alpaca paper, venue demo environments, and local
   fixtures only. Never ask for, read, or use live keys or production secrets, and never commit
   any secret. Paper keys stay in your own environment and out of the repository.
6. **The repository is public.** Keep commits, PRs, issues, and comments free of personal data,
   infrastructure details, account identifiers, and secrets.
7. **Never rewrite shared history.** No force-push to `main` or to a branch someone else is
   working on. On your own branch, merge `origin/main` with a merge commit rather than rebasing
   once a review has started.
8. **No `Co-authored-by` trailers.** CI rejects them (`spec-guard`'s `commit-trailers` check). If
   your editor or an AI tool adds one, remove it before you push.

## Setting up

- **Toolchain.** Rust comes from `rust-toolchain.toml` through rustup. Python 3.14 and uv 0.12 are
  needed, plus cargo-deny, cargo-nextest, typos, and gitleaks at the versions pinned in
  [`.github/workflows/ci.yml`](.github/workflows/ci.yml).
- **Install script.** `bash .cursor/install.sh` installs all of them at the pinned versions, and
  running it twice is safe. It downloads x86_64 Linux binaries, so on macOS or ARM install the same
  versions by hand.
- **Web UI.** Work under `web/` also needs the Node.js release in `web/.nvmrc`.
- **Before every push,** run `cargo xtask check`. It runs every job the two required CI checks run:
  - lint (fmt, clippy with warnings as errors, crate layering, typos, ruff);
  - tests (nextest, doctests, pytest);
  - reference-case fixture drift and the reference implementation checks;
  - supply chain (cargo-deny, the dependency registry, gitleaks);
  - the spec guard.
- **One test fails when run as root.** `mandate-marketdata`'s `concurrent_writes` test needs a
  non-root user. Run the checks as an ordinary user.

## Making a change

1. **Pick a story** from [the backlog](docs/project/06-backlog-v1.md) in milestone order, or a
   follow-up row a review left there. [The work tracker](docs/project/08-work-tracker.md) says what
   is next.
2. **Claim it** (rule 4).
3. **Branch** from `origin/main` with a name that is not under `agent/`. `<your-handle>/<story>`
   works.
4. **Safety-critical paths go tests first (DEC-77).** The list is in AGENTS.md. Work in two stages:
   - A tests PR. Stubs return an `Unimplemented` error, and the new tests are marked
     `#[ignore = "pending <story>"]`. No pending test may pass on a do-nothing implementation.
   - Then an implementation PR that deletes only those `#[ignore]` lines.
   - Spec, reference, and fixture changes never share a PR with crate code (ES-22).
5. **Decisions** go in their own file, `docs/project/decisions/DEC-<n>.md`
   ([how](docs/project/decisions/README.md)). Take the next free number on `main`, add the file in
   your first commit, and say on your claim issue that you took it. A reading that only tightens a
   rule you may accept yourself (DEC-176). Anything that loosens a spec toward the code, or that
   DEC-79 reserves, stays `Proposed` for the founder.
6. **Open the PR** with [the template](.github/pull_request_template.md). Cite the story ID, the PRD
   requirement, and the HLD section. List the spec clauses and the tests that cover them. Include
   your evidence:
   - the bugs you planted and the test that caught each;
   - the mutants run, `MANDATE_BASE_REF=origin/main cargo xtask ci mutants` with
     `CARGO_TARGET_DIR` unset. A safety-critical crate must show zero missed.
7. **Post `@coordinator ready, head <40-hex sha>`** on the PR when `fast` and `full` are green. If
   you are blocked on a choice, post a comment that begins `Decision needed:`. Both are what the
   coordinator watches for.
8. **After the review,** fix what the coordinator's ruling accepts and only that. Minor findings go
   to the backlog under the freeze rule. Merge `origin/main`, push, and post ready again with the
   new head. Do not push while a review you asked for is running unless CI is red.

## Conventions you will be held to

The full list is in AGENTS.md. The ones that catch people most often:

- **Money and quantities** use the fixed-point decimal types, never floating point. **Timestamps**
  are UTC with nanosecond precision.
- **Errors.** No `unwrap()` or `expect()` outside tests on trading paths; return typed errors.
- **No plain comments in Rust (DEC-80).** Put the reason in a name, a type, a test, an assertion
  message, or a doc comment. `cargo xtask markers` enforces this.
- **New crates and dependencies.** A new crate gets a row in `xtask/layers.toml`. A new dependency
  gets a row in [`docs/dependencies.md`](docs/dependencies.md) and a reason in the PR.
- **Reference-case fixtures** under `fixtures/refcases/` are generated. Never edit them by hand.
- **Commits.** One logical change per commit, with an imperative subject line.

## Where to look

| Question | Document |
|---|---|
| What is the product, and what must v1 do? | [README](README.md), [PRD](docs/product/04-prd-v1.md) |
| How is the system built? | [HLD](docs/HLD.md), [ADR-0001](docs/adr/0001-engineering-setup.md) |
| What are the exact rules? | [Trading domain spec](docs/specs/trading-domain.md), [mandate spec](docs/specs/mandate.md), [journal spec](docs/specs/journal.md) |
| What has been decided? | [Decision log](docs/project/04-decision-log.md) and [decision files](docs/project/decisions/) |
| What should I work on? | [Backlog](docs/project/06-backlog-v1.md), [work tracker](docs/project/08-work-tracker.md), open `claim` issues |
| What do the terms mean? | [Glossary](docs/product/glossary.md) |
| How is work reviewed and released? | [Quality and release](docs/project/07-quality-and-release.md) |
| Everything else | [Documentation index](docs/README.md) |

## Getting help

Ask on the relevant claim issue or PR. For anything about direction, scope, or a decision DEC-79
reserves, ask the founder.
