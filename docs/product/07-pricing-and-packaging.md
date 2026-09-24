# Pricing and Packaging

| | |
|---|---|
| **Owner** | Product |
| **Status** | Draft v0.1: hypotheses to validate with design partners |

## Principles

1. **Bill the organization**, on plan plus usage.
2. **Never charge per trade, or as a percentage of assets or profits.** This keeps Mandate a
   software business, avoids broker-dealer and investment-adviser-style compensation, and
   keeps incentives neutral.
3. **Pass through variable costs transparently** (hosted model usage, premium data) with a
   stated margin.
4. **Deployment mode is a packaging choice,** not a different product.

## Packaging

| | Individual | Team | Enterprise |
|---|---|---|---|
| For | Alex | Priya | Marcus |
| Deployment | Managed | Managed or hybrid | Managed, hybrid, or fully on-prem / air-gapped |
| Workspaces | 1 | Multiple | Unlimited |
| Live agents | Small allowance | Larger allowance, add-ons | Custom |
| Identity | Email + passkey, OIDC | OIDC SSO, roles | SAML, SCIM, separation of duties |
| Approvals | Web push, email, chat | + SMS, two-approver rule | + phone escalation, custom channels |
| Audit | Explorer, export | + retention settings | + SIEM export, custom retention, chain anchoring |
| Support | Community / email | Shared channel | Dedicated, SLA |

## Price levers (hypotheses)

| Lever | Unit | Rationale |
|---|---|---|
| Platform fee | Per organization per month, by plan | Covers control plane, audit, support |
| Live agents | Per live agent per month above the allowance | Scales with value delivered; paper agents free or cheap to encourage testing |
| Usage | Agent-hours beyond allowance; hosted model tokens at cost plus margin | Tracks our variable cost |
| Hybrid / on-prem license | Annual, per organization, by agent tier | Customer bears infrastructure cost; we bear support and releases |

Specific price points will be set after design-partner interviews on willingness to pay.

## Questions to validate

1. Do users anchor on "per agent" or "per workspace"?
2. Is a free paper-trading tier the right acquisition path for Individuals?
3. How large a premium does hybrid command over managed?
4. Which Enterprise features (SAML, SCIM, on-prem) actually drive upgrade decisions?
