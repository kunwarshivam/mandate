# Roles and RACI

| | |
|---|---|
| **Owner** | Project management |
| **Status** | Draft v0.2 |

## Team model

Mandate is built by the **founder working with AI coding agents**. No hires are planned for
v1 ([DEC-21](04-decision-log.md#decisions)). The founder owns every decision, reviews every
merge, and is the only one who holds production secrets. Agents implement, test, review, and
document within the rules in [`AGENTS.md`](../../AGENTS.md).

Some work cannot be delegated to agents and still needs outside humans: legal opinions, an
independent penetration test, and design-partner relationships.

## Roles

| Role | Held by | Responsibilities |
|---|---|---|
| Founder | Founder | Vision, priorities, design partners, architecture decisions, merge approval, releases, production secrets, final sign-off |
| Implementation agents | AI coding agents | Build stories from the [backlog](06-backlog-v1.md) on branches; write tests first; keep docs in sync |
| Review agents | AI coding agents (separate runs) | Independent review of every change for bugs, security, and adherence to [`AGENTS.md`](../../AGENTS.md) before founder review |
| Test and verification agents | AI coding agents | Run fuzzing, fault-injection, soak, and privacy suites; report results against release gates |
| Research agents | AI coding agents | Investigate APIs, libraries, exchange behavior, and competitors; summarize with sources |
| External counsel | Outside human, when needed | Regulatory posture, terms, disclosures |
| Penetration tester | Outside human, before real capital | Independent security assessment |
| Design partners | Outside humans | Usage, feedback, validation |

## Human-only responsibilities

These stay with the founder regardless of how capable agents become:

1. **Merging to main** and cutting releases.
2. **Reviewing safety-critical code:** risk gate, autonomy policy, executor and idempotency,
   reconciliation, connectors, credential handling, authentication, tenant isolation, and
   notification payloads.
3. **Holding secrets:** live exchange keys, production credentials, signing keys. Agents work
   only with paper and demo credentials and local test fixtures.
4. **Approving changes to accepted decisions** in the [decision log](04-decision-log.md).
5. **Signing off phase gates** ([milestones](02-milestones-and-wbs.md#phase-gates)).
6. **Anything with legal or financial consequence:** terms, disclosures, design-partner
   agreements, going live with real capital.

## RACI matrix

**R** responsible · **A** accountable · **C** consulted · **I** informed

| Activity | Founder | Implementation agents | Review agents | Test agents | Research agents | Counsel | Pen tester | Design partners |
|---|---|---|---|---|---|---|---|---|
| Product vision and PRD | A/R | I | I | I | C | I | I | C |
| Architecture and decisions | A/R | C | C | I | C | I | I | I |
| Core engine (Phase 0) | A | R | R | R | C | I | I | I |
| Agent runtime and risk gate | A/R (review) | R | R | R | I | I | I | I |
| Exchange connectors | A/R (review) | R | R | R | C | I | I | I |
| Advisors and calibration | A | R | R | R | C | I | I | C |
| Control plane and workspace services | A | R | R | R | I | I | C | I |
| Web app and approvals UX | A | R | R | R | I | I | I | C |
| Hybrid installer | A | R | R | R | I | I | I | C |
| Security review | A | C | R | R | I | I | R | I |
| Compliance posture, terms, disclosures | A/R | I | I | I | C | R | I | I |
| Release gates sign-off | A/R | C | C | R | I | C | C | I |
| Design-partner onboarding | A/R | C | I | I | I | I | I | C |
