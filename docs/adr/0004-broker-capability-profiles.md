# ADR-0004: Broker capability profiles

| | |
|---|---|
| **Status** | Accepted ([DEC-530](../project/decisions/DEC-530.md)) |
| **Date** | 2026-10-08 |
| **Deciders** | An agent (Claude Code), under DEC-79, at the founder's direction of 2026-10-08 |

Every ADR has a decision behind it; this one's is [DEC-530](../project/decisions/DEC-530.md).

## Context

The platform connects to more than one broker (Alpaca now, Robinhood for the first live order,
Kraken later). Each broker accepts different order types, quantity forms, times in force and
protective orders, and offers different idempotency. Today those facts live in two places that
do not scale: trading spec §5.2 is titled "Alpaca capability matrix", and the executor infers
Alpaca's "crypto has no OCO" from the asset class. Robinhood's published contract allows
fractional quantities only for market orders, has no OCO or bracket placement, and has a client
order id (`ref_id`) that orders cannot be queried by ([contract
confirmation](../project/tasks/robinhood-contract.md)). The founder ruled on 2026-10-08 that
these must not become broker-specific code.

## Decision

1. Each connector declares a typed, versioned **capability profile** of its broker's rules,
   from the published contract only (DEC-530 items 1 and 4).
2. Shared code (builder, executor protection, reconciliation, deployment validation) reads the
   profile and never branches on the broker, or on the asset class to learn a broker rule.
3. Platform policy (`AGENTS.md` rules 12 and 13, the gate) stays separate; the builder
   intersects policy with the profile.
4. The profile's hash is registered configuration, so replay and audit see which rules applied.

## Consequences

- A new broker is a new connector and a new profile; no change to the builder, executor or gate
  unless the broker offers something the profile cannot express.
- Trading spec §5.2 becomes "Broker capability profiles", with Alpaca's table as one profile and
  Robinhood's as another (the brief's SP1).
- The executor's protection choice and the builder's quantity rounding gain a profile argument;
  their existing tests pin Alpaca's behavior through the change.
- Must be monitored: a broker changing its contract. For MCP brokers the contract hash already
  halts openings on drift (connections spec §6.2 rule 3); the profile version moves with it.

## Alternatives rejected

| Alternative | Why |
|---|---|
| Broker branches in the connector and executor | Rework for every broker; the founder rejected it |
| Learn capabilities from the broker's tool schemas at runtime | Broker metadata never changes what we may do (CN-9) |
| Rewrite all of trading spec §5 first | Right end state, too slow; rows are added as stories need them |
