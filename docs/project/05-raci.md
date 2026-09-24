# Roles and RACI

| | |
|---|---|
| **Owner** | Project management |
| **Status** | Draft v0.1 |

## Roles

| Role | Status | Responsibilities |
|---|---|---|
| Founder / CEO (acts as product owner) | Filled | Vision, priorities, design partners, fundraising, final sign-off |
| Tech lead, trading systems (Rust) | To hire | Core engine, runtime, risk, connectors, reliability |
| Platform engineer | To hire | Control plane, workspace services, deployment, security |
| Frontend engineer | To hire | Web app, approvals experience, audit explorer |
| Quant / ML engineer | To hire | Advisors, backtesting methodology, calibration, model gateway |
| Security advisor | Fractional | Threat model, reviews, penetration test coordination |
| Compliance / trading-risk advisor | Fractional | Risk controls review, compliance posture, industry credibility |
| External counsel | Engaged when needed | Regulatory posture, terms, disclosures |
| Design partners | To recruit | Usage, feedback, validation |

Until roles are filled, the founder holds them, and AI coding agents assist with
implementation under founder review.

## RACI matrix

**R** responsible · **A** accountable · **C** consulted · **I** informed

| Activity | Founder | Tech lead | Platform | Frontend | Quant/ML | Security | Compliance advisor | Counsel | Design partners |
|---|---|---|---|---|---|---|---|---|---|
| Product vision and PRD | A/R | C | C | C | C | I | C | I | C |
| Core engine (Phase 0) | A | R | I | I | C | I | I | I | I |
| Agent runtime and risk gate | A | R | C | I | C | C | C | I | I |
| Exchange connectors | A | R | C | I | I | C | I | I | I |
| Advisors and calibration | A | C | I | I | R | I | I | I | C |
| Control plane and workspace services | A | C | R | C | I | C | I | I | I |
| Web app and approvals UX | A | I | C | R | I | C | I | I | C |
| Hybrid installer | A | C | R | I | I | C | I | I | C |
| Security review and pen test | A | C | C | C | I | R | I | I | I |
| Compliance posture, terms, disclosures | A | I | I | I | I | C | R | R | I |
| Release gates sign-off | A | R | R | R | R | C | C | C | I |
| Design-partner onboarding | A/R | C | C | C | C | I | I | I | C |
