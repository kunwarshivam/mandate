# AGENTS.md

Instructions for AI coding agents working in this repository. Humans: see
[docs/README.md](docs/README.md).

## What this project is

Mandate is a platform for deploying autonomous trading agents that trade on users' own
exchange and brokerage accounts, bound by an enforceable mandate, escalating to a human when
unsure, and recording every decision. It is built by the founder working with AI coding
agents. Agents land their own changes once they pass CI and an independent agent review; the
founder reviews after the fact and can revert (DEC-79).

## Sources of truth

Read the relevant document before changing anything it covers. If code and docs disagree,
stop and ask; do not silently pick one. The one exception is DEC-176: when one reading only
tightens the rule and adds no risk, an agent may record that reading in a DEC and take it; a
reading that loosens the spec toward the code still goes to the founder.

| Question | Document |
|---|---|
| How is the system structured? | [docs/HLD.md](docs/HLD.md) |
| What must v1 do? | [docs/product/04-prd-v1.md](docs/product/04-prd-v1.md) |
| What should I work on? | [docs/project/06-backlog-v1.md](docs/project/06-backlog-v1.md), in milestone order from [docs/project/02-milestones-and-wbs.md](docs/project/02-milestones-and-wbs.md) |
| Where does the work stand, and what is next? | [docs/project/08-work-tracker.md](docs/project/08-work-tracker.md), one page, updated at the end of every session; how the work ran, with every PR, is [docs/project/11-work-log.md](docs/project/11-work-log.md) |
| What has already been decided? | [docs/project/04-decision-log.md](docs/project/04-decision-log.md) for DEC-01 to DEC-302, and one file per decision under [docs/project/decisions/](docs/project/decisions/README.md) after that (DEC-344) |
| What are the exact trading rules (accounting, orders, fees, settlement, account rules)? | [docs/specs/trading-domain.md](docs/specs/trading-domain.md) and its [reference cases](docs/specs/reference-cases/trading-domain.yaml), which tests must reproduce exactly |
| What is a mandate, which invariants must hold, and how are limits, autonomy, and the order builder defined? | [docs/specs/mandate.md](docs/specs/mandate.md), the [mandate](schemas/mandate.schema.json) and [policy](schemas/policy.schema.json) schemas, and its [reference cases](docs/specs/reference-cases/mandate.yaml) |
| How are events journaled, hashed, stored, and replayed? | [docs/specs/journal.md](docs/specs/journal.md) and its [test vectors](docs/specs/reference-cases/journal.yaml) |
| How is the code organized, and which tools and versions do we use? | [docs/adr/0001-engineering-setup.md](docs/adr/0001-engineering-setup.md) (ADR-0001) |
| Why does the platform originate ideas, and what are the retail and connector plans? | [docs/adr/0002-autonomous-ideation-and-retail.md](docs/adr/0002-autonomous-ideation-and-retail.md) (ADR-0002, DEC-97, DEC-98) |
| What do terms mean? | [docs/product/glossary.md](docs/product/glossary.md) |
| How is work reviewed and released? | [docs/project/07-quality-and-release.md](docs/project/07-quality-and-release.md) |

## Non-negotiable rules

1. **The mandate is the contract.** No code path may let an agent act outside its mandate.
   Limits are enforced by the risk gate, independent of agent logic.
2. **Reducing risk never needs approval; increasing risk beyond limits always does.**
3. **Timeouts and ambiguity resolve to a safe default that never adds risk.**
4. **LLMs produce opinions, never orders.** Deterministic code sizes positions and places orders.
5. **Journal before acting.** Every order intent is written to the journal, with an idempotency
   key, before it is sent.
6. **No sensitive content in notifications.** Notification payloads carry only opaque IDs and
   generic text.
7. **Credentials never leave the vault**, never appear in logs, and are never committed.
8. **Never place real orders.** Use broker paper environments (Alpaca paper), venue demo
   environments, and local fixtures only. Never ask for, read, or use live credentials or
   production secrets.
9. **Accepted decisions are binding.** To deviate, write a new decision. Agents accept
   reversible engineering and process decisions themselves and proceed; decisions reserved for the
   founder (DEC-79: live money, spending, legal and compliance text, weakening a safety rule) stay
   Proposed while agents continue with the most conservative option.
10. **Enforce US account rules.** Day-trading regime, settlement, market-hours, eligibility,
    market-conduct, and account-restriction rules are part of the risk gate, not optional checks
    ([trading domain spec §9](docs/specs/trading-domain.md#9-risk-gate)).
11. **The owner sets the envelope; the platform brings the ideas** (DEC-97,
    [ADR-0002](docs/adr/0002-autonomous-ideation-and-retail.md)). Envelope fields (capital, goal, risk
    limits, autonomy rules, allowed asset classes, `max_instruments`, connection) are entered or
    proposed and always confirmed by the user, and no code path changes them without a confirmed
    mandate version. Signal models, their fixed weights, thresholds, protection, and cadence are
    envelope fields too. Only the working universe and its theses come from the research agent at
    runtime, within the envelope, journaled, and admitted through the eligibility floor and the
    autonomy rules. Requests from an owner-connected agent (DEC-141) are owner input: they pass
    the same builder, gate, and autonomy rules, and never change the envelope.
    Bring-your-own-strategy pins the universe. No calibration in v1.
12. **All account-level actions go through the account ledger.** Agents never call the broker
    directly; opening orders are limit orders in the regular session; no short sales in v1.
13. **Risk reduction is never denied by conduct controls, eligibility, day-trade budgets, buying
    power, or opening-session rules.** Risk exits, protective orders, and automated kill switches
    are exempt from all of them. Owner exits are paced only by participation caps. Discretionary
    exits (signal or goal driven) are paced by conduct controls and, for equities, wait for the
    regular session, but are never denied, except when the exits already allowed sell the whole
    position, so nothing is left for it to sell; then it is refused `sell_exceeds_available` as
    DEC-410 item 3 says. Exits and protective orders
    may be held only by agent mode `paused` or `stopped`, by an `Unknown` order in the same
    instrument, or by the broker. Owner exits may sell equities outside the regular session once the
    owner confirms the displayed bid. The kill switch is always available, touches only its scope
    (an agent-scoped kill switch never uses cancel-all or close-position), and does not depend on
    model state.

## Safety-critical paths

Changes here require tests written or verified against approved reference cases before
implementation, and before merge an independent review by an agent on a different model, zero
missed mutants, and green CI (DEC-79):

- Accounting (positions, cash, fees, corporate actions, settlement, funding, P&L)
- Risk gate, US account rules, eligibility, market-conduct controls, drawdown ladder, kill switches
- Account ledger, reservations, and protective-exit sequencing
- Autonomy policy (AUTO / ASK / DENY), mandate validation, and mandate change classification
- Executor, idempotency, reconciliation, crash recovery
- Broker and exchange connectors, OAuth scopes, and key-permission checks
- Credential handling and the vault
- Authentication, step-up authentication, roles, tenant isolation
- Notification payloads and the approval flow

## How to work

1. Pick one story from the backlog that no open claim issue holds, and claim it
   (`.cursor/skills/mandate-mode/playbooks/coordination.md`). One change implements one story.
2. In the change description, cite the story ID, the PRD requirement, and the HLD section.
3. Write tests first for safety-critical paths; include property-based tests for invariants.
4. Keep changes small enough to review in one sitting.
5. Update docs in the same change when behavior, interfaces, or decisions change.
6. Emit journal events for every new state change.
7. Run the full local check before proposing the change (see below).

## Working with agents

Start non-trivial work with the `mandate-mode` skill (`.cursor/skills/mandate-mode/SKILL.md`): it
picks the playbook, lists the few decisions reserved for the founder, and routes to the skills
vendored from pstack and cursor-team-kit, and for `web/` the design skills
(`.cursor/third_party/README.md`). Prove work with the
`verify-mandate` skill; its feature map says which code, tests, and commands cover each feature.
Where a vendored skill conflicts with this file, this file wins.

### The trust ladder

Agents copy what the repository already contains, so a mistake that gets in once spreads. When an
agent needs the same correction twice, put the rule on the highest rung that can hold it, using the
`correction` playbook:

1. **Unrepresentable:** types, private fields, and crate boundaries (`xtask/layers.toml`).
2. **Checked:** clippy lints, xtask checks, and CI. Today these include debt markers
   and plain comments (`cargo xtask markers`), `#[allow]` without a reason, `#[ignore]` without a
   pending story, a pending test that passes on the PR's code or fails without its stub's own
   report (`cargo xtask ci pending`), a pending test a macro generates rather than a plain
   function, a saved proptest failure seed, tracked or not (DEC-381), feature-map drift, and
   mutants on the diff of safety-critical crates, an `Unimplemented` stub body in a crate whose
   tests are still pending aside.
3. **Guided:** this file, skills under `.cursor/skills/`, and `.cursor/BUGBOT.md`.
4. **Reviewed:** the PR template and the independent review agent, the last resort, not the plan.

Leave the code in a state you would want the next agent to copy: one paved path per task, no
workarounds, and debt either fixed or recorded in the backlog.

## Getting it right the first time (specs and safety-critical design)

The mandate spec needed a rewrite after every review round (v0.1 to v0.3). The findings had eight
causes; each rule below closes one of them. Follow these before sending any spec, schema, or
safety-critical design for review.

| Cause, with an example from the mandate spec | Rule |
|---|---|
| **Local fixes, no global re-check.** v0.2 scaled the high-water mark on allocation changes but left contributed capital additive, so a withdrawal could trip the lifetime floor, contradicting the spec's own sentence two lines earlier | **Invariants first.** Write the properties the design must always hold (for example, "an allocation change never triggers or lifts a limit") before writing rules. After every change, re-check every invariant, not only the finding being fixed |
| **Claims stated, never tested.** "Can neither lift nor trigger a limit" was asserted, not checked | **Every claim is a test.** Each invariant and each "never" or "always" in the text has a property-based test in the reference implementation. Fuzz random sequences of events (marks, fills, allocation changes, acknowledgments, version changes, clock ticks) and assert the invariants |
| **Examples that break their own rules.** A base mandate's `target_qty` cost more than its allocation; a case titled "fraction cap binds" did not bind; v0.1's V-021 rejected the spec's own example | **Validate fixtures against the rules.** Every example and base fixture passes every rule it is not meant to fail; every case asserts the condition its title claims, and is recomputed from the rules, not typed |
| **Cross-spec contracts not traced.** The agent flatten reused the account-wide kill switch (cancel-all, close-position); units differed (basis points vs fraction); two specs gave different outcomes for the same limit | **Trace every reference.** For each rule that relies on another spec, read that section and confirm scope, units, and outcome match; update both sides in the same change |
| **Lifecycles not finished.** Nothing said what happens after a flatten, after a goal completes, after an acknowledgment, or at a time boundary | **Walk every state to its exit.** For each state and limit: how it is entered, what it blocks, how it ends, who can end it, and what happens at session close, midnight, restart, and version change |
| **No adversary.** Order splitting, redeploying to reset limits, deposits to lift rungs, one bad tick, and a model outage enlarging orders were all found by reviewers | **Attack it yourself.** Before review, list how a careless user, a bad model, a malicious insider, and a bad market tick could exceed intended risk or imply platform advice; each must be blocked or disclosed |
| **Decisions hidden inside drafts, and scope creep.** About a third of each round were design choices only the founder can make, and each round added features that became new surface | **Separate decisions from defects.** List open design choices before drafting and record each in the decision log (agents decide all but those DEC-79 reserves for the founder). While fixing findings, add no features; propose them separately |
| **Tests that pass while checking nothing.** Fuzz checks that reused the implementation's own predicate, or reset their own timer on every bounce, could not catch the regressions they named | **Independent oracles.** A property test computes the expected result its own way (a separate accumulator, state derived from journaled events), and each oracle is shown to fail on a seeded bug before it is trusted |

**Before external review:**

1. Run the invariant fuzz, schema and fixture validation, and a link check. For the mandate spec:
   `python3 reference/mandate/generate.py`, `python3 reference/mandate/check_cases.py`, and
   `python3 reference/mandate/fuzz.py <seed>` for several seeds, and
   `python3 reference/mandate/mutants.py` (every seeded bug must be caught); requirements in
   `reference/mandate/requirements.txt`.
2. Self-review against the three role checklists: engineer (determinism, ordering, time base,
   replay, every input listed), risk (loopholes, gap risk, lifecycle), and compliance (who chooses
   what, records, wording).

**Freeze rule:** after an external review, fix only blockers and majors. Minors go to the backlog.

## Conventions

As decided in [ADR-0001](docs/adr/0001-engineering-setup.md).

- **Setup:** `bash .cursor/install.sh` installs everything at the pinned versions and is idempotent;
  Cloud Agents run it automatically through `.cursor/environment.json`. Rust comes from
  `rust-toolchain.toml` (1.98.1) via rustup, which must already be installed; Python 3.14 and uv
  0.12; cargo-deny, cargo-nextest, cargo-mutants, typos, gitleaks, shellcheck, and actionlint at
  the versions in `.github/workflows/ci.yml` (keep the two in sync). The binaries it downloads are
  x86_64 Linux builds. It downloads only from `github.com` (release assets redirect to
  `release-assets.githubusercontent.com`), `static.rust-lang.org`, `index.crates.io`,
  `static.crates.io`, `pypi.org`, and `files.pythonhosted.org`, never from `astral.sh`, so it runs
  behind restrictive egress proxies. The one exception is PostgreSQL 18 for the Postgres journal
  tests: it tries the system package manager and `apt.postgresql.org`, and if that fails those
  tests skip. It also sets `python-install-mirror` in uv's user config
  (`~/.config/uv/uv.toml`) so later `uv` calls fetch Python from GitHub too. Work under `web/`
  (DEC-200) also needs `nodejs.org`, for the Node.js release pinned in `web/.nvmrc`,
  `registry.npmjs.org`, for `npm ci`, and `cdn.playwright.dev`, for the Chromium download that
  local end-to-end runs need; `install.sh` installs no Node.js and fetches from none of them.
- **Before proposing any change, run `cargo xtask check`.** It runs every per-PR job: lint
  (shellcheck over `.github/scripts/`, actionlint over `.github/workflows/`, fmt, clippy
  `-D warnings`, crate layering, the `live` feature (only the crate `xtask/layers.toml` marks
  `live_feature` may declare one, and CI only compiles it; DEC-529), markers, saved proptest
  seeds, the feature map, typos, ruff), test (nextest, doctests, pytest), pending tests, reference-case fixture drift, the reference
  implementation checks, the workspace API schema checks (`schemas`), supply chain (cargo-deny,
  the dependency registry, gitleaks), the spec guard, the Postgres journal tests (skipped unless
  `MANDATE_PG_URL` is set), mutants on the diff of safety-critical crates, and the workspace API
  schemas' mutation sweep (`schema-mutants`). CI runs them as two required checks:
  `cargo xtask ci fast` (lint, test, pending tests, spec guard) and `full`, an aggregate over
  `cargo xtask ci full` (fixtures, reference, schemas, supply chain, Postgres), every
  deterministic `cargo xtask ci mutants` shard, and every `cargo xtask ci schema-mutants` shard (DEC-688). Every
  required check and aggregated job must finish in under ten minutes; add parallel shards rather
  than removing tests, baselines, mutants, or safety gates (DEC-464).
- **New crates** get an entry in `xtask/layers.toml` in the same change; safety-critical crates also
  get a CODEOWNERS line and start `src/lib.rs` with the lint header `cargo xtask layers` checks.
- **New dependencies** need a row in `docs/dependencies.md` in the same change (none by default).
- **Reference-case fixtures** (`fixtures/refcases/`) are generated: after a YAML change the founder
  approved, or one DEC-176 lets an agent accept (it tightens a rule, or resolves a gap by the
  reading that adds no risk, and weakens no safety invariant or non-negotiable), run
  `cargo xtask refcases --write`; never edit them by hand.
- **Tasks** use `docs/project/templates/task.md`; PRs use `.github/pull_request_template.md`;
  decisions are one file each, `docs/project/decisions/DEC-<n>.md` (DEC-344; the format and how to
  reserve a number are in that directory's README), and an architecture decision also gets an ADR
  from `docs/adr/template.md`. The decision log takes no new rows.
- **Rust** for the core: runtime, risk, execution, connectors, market data, journal.
- **Python** for research, model tooling, and the SDK.
- No `unwrap()` or `expect()` in non-test Rust code on trading paths; return typed errors.
- Money and quantities use fixed-point decimal types, never floating point, in accounting and
  order paths.
- Timestamps are UTC with nanosecond precision.
- Imports at the top of files; no inline imports.
- No plain comments (`//`, `/* */`) in Rust code (DEC-80). Put the reason in a name, a type, a
  test, an assertion message, or the item's doc comment (`///`, `//!`); `cargo xtask markers`
  enforces it.
- Commits: imperative, descriptive subject lines; one logical change per commit.

## Do not

- Add dependencies without stating why in the change description.
- Disable, skip, or weaken tests to make a change pass.
- Introduce floating-point arithmetic into money or quantity calculations.
- Log order details, positions, or mandate content in anything that leaves the workspace
  deployment.
- Add platform-generated trade recommendations, per-trade pricing, or custody of funds (see
  [compliance](docs/product/08-compliance-and-regulatory.md)).
