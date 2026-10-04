# Contributing to Mandate

Thank you for reading this before contributing — the rules here are short but binding, and they
exist because this repository builds software that can place orders on real brokerage accounts.

## Read these first

1. **[AGENTS.md](AGENTS.md)** — the non-negotiable rules. They bind every contributor, human or
   AI agent, and nothing in this guide overrides them.
2. **[COLLABORATION.md](COLLABORATION.md)** — how the repository actually runs: builders,
   independent cross-model reviews, the coordinator, and the merge flow.
3. **[docs/README.md](docs/README.md)** — the documentation map, including the specs that are the
   source of truth for trading behavior, the journal, and the mandate.

## How changes land

- **One PR per story**, targeted at `main`, using the
  [PR template](.github/pull_request_template.md) (story, spec clause → test, dependencies,
  journal events).
- **Safety-critical changes** (accounting, the risk gate, the executor, connectors, credentials,
  authentication, notifications — see AGENTS.md's list) ship as the DEC-77 sequence: a tests PR,
  an implementation PR, and a status PR, with an independent review by an agent on a *different
  model* before merge (DEC-79).
- **Run the local gate before proposing:** `cargo xtask check`. CI enforces the same jobs.
- **Documentation-only changes** take the short path (DEC-112) — cheap checks, still reviewed.

## Claiming work

Work is tracked one story per claim issue, labeled `claim`, titled `<story ID>: <title>`. See the
[claim issue template](.github/ISSUE_TEMPLATE/claim.md) and the coordination rules in
COLLABORATION.md. Unclaimed work is listed in
[docs/project/06-backlog-v1.md](docs/project/06-backlog-v1.md) in milestone order.

## Decisions, not debates

When a rule, spec sentence, or safety question is ambiguous, contributors record a decision
(`docs/project/decisions/`) rather than arguing in a thread: engineering and process decisions are
accepted by the contributor and proceed; decisions DEC-79 reserves for the founder (live money,
spending, legal and compliance text, weakening a safety rule) are marked Proposed and continue on
the most conservative option until the founder rules.

## Reporting problems

- A **defect** gets a reproduction first — a failing test or the exact command and output — per
  the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md).
- A **security vulnerability** is reported privately: see [SECURITY.md](SECURITY.md). Please do
  not open a public issue for it.
