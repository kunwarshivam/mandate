# ADR-0002: Autonomous ideation and retail from the start

| | |
|---|---|
| **Status** | Accepted ([DEC-97](../project/04-decision-log.md#decisions), [DEC-98](../project/04-decision-log.md#decisions)) |
| **Date** | 2026-09-26 |
| **Deciders** | Founder |

## Context

Mandate v1 as specified is a guardrail platform: the user supplies the strategy (instruments,
signal models, weights, thresholds) and the platform enforces the mandate around it. Every
platform choice was forbidden ([DEC-33](../project/04-decision-log.md#decisions),
[DEC-38](../project/04-decision-log.md#decisions), [DEC-45](../project/04-decision-log.md#decisions),
[DEC-62](../project/04-decision-log.md#decisions), `AGENTS.md` rule 11) so that Mandate could
claim to be software only and never an adviser. Retail was gated behind counsel and a retail
profile that disabled `auto` and LLM models ([DEC-61](../project/04-decision-log.md#decisions)).

On 2026-09-26 the founder set a different direction: the product is a fully autonomous agent.
The platform brings in ideas, uses LLMs to work them into the inputs an agent needs, and the agent
trades on them within the owner's envelope. Retail is in scope from the start, through consumer
brokers, so adoption does not depend on funds.

Findings that shape the decision:

1. The safety architecture already supports this. The risk gate, drawdown ladder, lifetime floor,
   journal, kill switches, eligibility floor, and conduct controls do not depend on who chose the
   instrument. "LLMs produce opinions, never orders" ([DEC-04](../project/04-decision-log.md#decisions))
   is exactly the boundary a research agent needs: it produces theses; deterministic code sizes and
   places orders. The mandate spec's signal-model contract (§8.1, §8.2) already defines the output
   shape a thesis must take.
2. What breaks is the compliance posture, not the engineering. Platform-originated, personalized
   ideas executed in a user's account is the shape of investment advice under the Advisers Act.
   The software-only defense is gone and counsel must say what replaces it.
3. **Robinhood** launched **Agentic Trading** in beta on 2026-05-27: a customer opens a dedicated
   agentic trading account, funds it with what they are willing to risk, and connects a third-party
   agent through Robinhood's Model Context Protocol (MCP) servers. Equities only at launch; options,
   crypto, event contracts, and futures announced as coming. Robinhood states that it does not
   control, supervise, monitor, or audit connected agents, that customers are responsible for
   reviewing activity, and that it does not guarantee any agent output. Robinhood also has a REST
   crypto trading API for its crypto customers.
4. **Fidelity** has no public retail trading API. Aggregators such as SnapTrade can read Fidelity
   and Robinhood accounts but cannot place trades on either. Fidelity's FIX access is institutional.

## Decision

1. **The mandate is a risk envelope and a goal, not a strategy.** Mandate fields split into two
   kinds:
   - **Envelope fields** are owned and confirmed by the user and keep every existing rule
     (validation, policy hierarchy, change classification, step-up for risk-increasing changes):
     `environment`, `connection_id`, `capital`, `goal`, `risk.*`, `protection.enabled`,
     `autonomy`, `notifications`, the allowed asset classes and the leveraged-ETP opt-in, and a new
     `universe.max_instruments`. The compiler and templates may now **propose** values for them
     (provenance `platform_proposed`); the user still confirms every one, and every proposed value
     is shown as proposed.
   - **Strategy fields** are produced by the platform at runtime, journaled, and bounded by the
     envelope: the working universe, theses, the signal-model set and weights, entry and exit
     thresholds, the rebalance band, protection distances, and cadence. They leave the hashed
     mandate document; changing them is not a mandate version (amends
     [DEC-43](../project/04-decision-log.md#decisions)).
   - **Bring-your-own-strategy remains a mode.** An owner may pin the strategy fields, which
     disables the research agent and gives today's behavior. Funds that want their own strategies
     keep it.
2. **A research agent joins the runtime** (HLD §5), beside the signal models: LLM-driven,
   asynchronous, never blocking trading, metered through the model gateway. Its loop:
   1. **Ingest:** market data, news and filings, screens over the eligible universe, and the
      agent's own memory (positions, past theses and their outcomes, track records; supersedes
      [DEC-62](../project/04-decision-log.md#decisions) for the agent's own memory).
   2. **Propose:** a thesis names one instrument, direction (long only in v1), horizon, evidence,
      and invalidation conditions, with conviction and confidence in the signal-model output shape
      (mandate spec §8.2). Journaled as `ThesisProposed` (agent stream) with the prompt and
      response as artifacts.
   3. **Admit:** an instrument enters the working universe only if it passes the eligibility floor
      (trading spec §3.2), the policy's asset classes, `universe.max_instruments`, the instrument
      group claim (trading spec §7.1), and the autonomy rules (part 3). Admission and removal are
      journaled as `UniverseChanged` (account stream, a risk input with `risk_clock`).
   4. **Feed:** the thesis is the research agent's signal-model output for that instrument. The
      order builder combines it with any other configured models (mandate spec §8.3), sizes
      deterministically, clips to the envelope, and proposes; the gate decides. Nothing else on the
      order path changes.
   5. **Retire:** an invalidated thesis marks the instrument removed: exits only, protection stays,
      discretionary exit through the existing removed-instrument rules (mandate spec §2.2).
3. **Autonomy over admissions.** The condition language gains `new_instrument` (true when the
   order would be the first in a newly admitted instrument) and `thesis_confidence`. Admitting an
   instrument is an `open` action for the autonomy rules; the platform default for admissions is
   `ask`, and the owner may set `auto` for them like any other rule.
4. **Signal-model contract.** The research agent's outputs may be directional and may name
   instruments outside the current universe; the "no imperatives" restriction of
   [DEC-52](../project/04-decision-log.md#decisions) is lifted for it. Methodology-only
   documentation, no performance claims, content-hash pinning and no substitution
   ([DEC-67](../project/04-decision-log.md#decisions)) stay.
5. **Retail from the start.** Retail workspaces are the default (DEC-68 stays). The retail profile
   of [DEC-61](../project/04-decision-log.md#decisions) is replaced: `auto_allowed: true`,
   `signal_model_types` includes `llm`, `protection_required: true`, leveraged ETPs off, the
   lifetime loss ceiling and approval timeout minimum set with counsel. Launch gates stay:
   counsel review, disclosures, and risk-disclosure acceptance before any live trading; until then
   every retail user trades paper (`AGENTS.md` rule 8 is unchanged).
6. **Connectors** (amends [DEC-23](../project/04-decision-log.md#decisions)): Alpaca stays first
   (paper environment, OAuth, stocks, ETFs, and crypto; in progress). **Robinhood Agentic Trading**
   is second: the retail equities path, connected over MCP to the customer's dedicated agentic
   account, which is a natural allocation boundary (the agent can only reach what the customer
   deposited there). Robinhood's crypto API follows for crypto. Kraken Derivatives US moves to
   third. Fidelity is deferred until it offers an official trading API; aggregator trading is not
   used.
7. **Compliance posture.** The software-only, never-advises position is withdrawn. The working
   assumption is that Mandate may be an investment adviser under the Advisers Act and must plan
   for registration or a counsel-approved structure. Counsel engagement moves from M13 to before
   the Phase 1 exit. The compliance document records the new assumption and the questions this
   raises; no live trading for any user until counsel signs off.
8. **Unchanged:** DEC-03 to DEC-07; the gate, ladder, floor, and breach confirmation; journal
   before acting; kill switches; eligibility floor and conduct controls; no shorts, 1×, limit-only
   openings; no custody, no per-trade pricing, no promises of returns.
9. **Sequencing.** Phase 0 (market data, accounting, backtest, journal) is unchanged: E3-3 stays
   next. The mandate spec, its schemas, the reference implementation, and the 215 reference cases
   are rewritten in spec-change PRs before M5 (the field split, universe as runtime state, the
   research agent contract, `ThesisProposed` and `UniverseChanged`, the retail profile, and
   V-020, V-022, and MI-12 restated for envelope fields). Backlog: E15 (LLM signal models) becomes
   Must in Phase 1; a new epic E17 (research agent and dynamic universe) is Must in Phase 1; a
   Robinhood connector story joins E7; the retail profile joins E9. The Phase 1 exit criterion
   becomes an agent trading paper unattended on theses it generated.

## Consequences

- **Easier:** the product matches what users want from an autonomous agent. Robinhood's dedicated
  account maps onto the allocation. Funds keep the bring-your-own-strategy mode.
- **Harder:** prompt injection through news and filings (RAID R-05) becomes the top technical risk,
  because the research agent chooses instruments from public text. The envelope, the eligibility
  floor, `max_instruments`, the admission rules, and the input-drift detector (V-018, now needed
  in Phase 1) are the defenses. Model cost and provider dependence rise. The spec rewrite is large.
- **Hard to reverse:** once retail users trade on platform ideas, the adviser posture is set;
  registration or its structural substitute is a business commitment.
- **Monitor:** counsel's answers; Robinhood's beta terms (rate limits, whether one platform may act
  for many customers, MCP contract stability); thesis quality against outcomes (scorecards move
  earlier); escalation precision when admissions are `ask`.

## Alternatives rejected

| Alternative | Why |
|---|---|
| LLM as a user-selected signal model only (user still picks the universe) | Not autonomous: the user still has to have the idea |
| LLM drafts the whole mandate, user confirms, no runtime changes | Ideas expire; a universe frozen at confirmation cannot follow them |
| Keep the software-only posture and stay out of retail | The founder's direction; the posture removed the product's value |
| Fidelity first | No official trading API; aggregators cannot trade there |
| Trade through an aggregator (SnapTrade or similar) | Read-only for Robinhood and Fidelity; a third party in the order path |
