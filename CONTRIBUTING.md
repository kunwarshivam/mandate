# Contributing to Mandate

Thank you for reading this before contributing — the rules here are short but binding, and they
exist because this repository builds software that can place orders on real brokerage accounts.

**Start with [AGENTS.md](AGENTS.md).** Its non-negotiable rules bind every contributor, human or
AI agent. For how the repository actually runs day to day — builders, independent cross-model
reviews, the coordinator, the merge workflow, claiming work — read
[COLLABORATION.md](COLLABORATION.md), the operational manual. This file is only the front door.

## What you need to know in one minute

- **Who can contribute.** The repository is public for transparency under a source-available,
  all-rights-reserved license ([LICENSE](LICENSE)); contributions are made by collaborators with
  write access, and external contributions are not accepted by default. Most of the code is
  written by AI agents under a merge coordinator — a person working alongside them follows the
  same rules.
- **One PR per story**, targeted at `main`, using the
  [PR template](.github/pull_request_template.md) (story, spec clause → test, dependencies,
  journal events).
- **Safety-critical changes** (accounting, the risk gate, the executor, connectors, credentials,
  authentication, notifications — AGENTS.md's list) ship as the DEC-77 sequence: a tests PR, an
  implementation PR, and a status PR, with an independent review by an agent on a *different*
  model before merge (DEC-79).
- **Run the local gate before proposing:** `cargo xtask check`. CI enforces the same jobs.
  Documentation-only changes take the short path (DEC-112) — cheap checks, still reviewed.
- **Decisions, not debates.** When a rule or spec sentence is ambiguous, contributors record a
  decision in `docs/project/decisions/` rather than arguing in a thread; what is reserved for the
  founder stays Proposed while work continues on the most conservative option (DEC-79).

## Finding and claiming work

Work is tracked one story per claim issue, labeled `claim`, titled `<story ID>: <title>` — see the
[claim issue template](.github/ISSUE_TEMPLATE/claim.md) and the claiming rules in
[COLLABORATION.md](COLLABORATION.md). Unclaimed work is listed in
[docs/project/06-backlog-v1.md](docs/project/06-backlog-v1.md) in milestone order, and where
things stand is in the [work tracker](docs/project/08-work-tracker.md).

## Reporting problems

- A **defect** gets a reproduction first — a failing test or the exact command and output — per
  the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md).
- A **security vulnerability** is reported privately: see [SECURITY.md](SECURITY.md). Please do
  not open a public issue for it.
- **Conduct** issues follow the [code of conduct](CODE_OF_CONDUCT.md).
