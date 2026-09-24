# Mandate

A platform for deploying autonomous trading agents. Users connect their brokerage or exchange
accounts, describe an agent's goal and behavior, and deploy it. The agent trades on its own
within the mandate its owner approved, asks for approval when it is unsure, and records every
decision it makes.

It runs fully managed, hybrid (a thin hosted control plane with strategy, approvals, audit
data, and credentials on the customer's side), or fully on-prem, and supports multi-tenant
organizations with SSO.

This repository is at the design stage.

## Documents

- [Documentation index](docs/README.md)
- [High-Level Design](docs/HLD.md)
- Product: [vision](docs/product/01-vision-and-strategy.md), [PRD v1](docs/product/04-prd-v1.md),
  [roadmap](docs/product/05-roadmap.md)
- Project: [charter](docs/project/01-project-charter.md),
  [milestones](docs/project/02-milestones-and-wbs.md), [RAID log](docs/project/03-raid-log.md),
  [decision log](docs/project/04-decision-log.md)
