# Design questions before the mandate spec rewrite

| | |
|---|---|
| **Owner** | Founder (decides); agents (options and recommendations) |
| **Status** | Open. Each answer becomes a decision-log entry before the spec-change PRs start |

The mandate spec rewrite for [DEC-97](04-decision-log.md#decisions) to
[DEC-103](04-decision-log.md#decisions) ([ADR-0002](../adr/0002-autonomous-ideation-and-retail.md))
depends on the design choices below. `AGENTS.md` ("Separate decisions from defects") asks for them
before drafting, so that review rounds find defects rather than open choices. The founder decides
them; agents give the options and a recommendation. Accepted decisions stand as written, and no
recommendation here departs from one; a departure needs its own decision-log entry first
(`AGENTS.md` rule 9). Until a question is answered, agents continue with its most conservative
option consistent with those decisions.

## 1. `universe.max_instruments` default and platform ceiling

`universe.max_instruments` bounds the working universe the research agent may build. Today
`universe.instruments` holds 1 to 50 instruments (mandate spec §3). More instruments mean more
admissions to approve, smaller positions at a given allocation, and more research cost.

- Default: 5, or 10.
- Platform ceiling: 50 (today's schema bound), or 20.

**Recommendation.** Default 5 and platform ceiling 20, the same for retail. At a 1,000 USD
allocation, five positions are 200 USD each, and five admissions to approve stay reviewable. Organizations may set a lower maximum through the policy hierarchy (DEC-51).

## 2. Thesis lifetime

A thesis has a horizon (`horizon_s`, mandate spec §8.2). When the research agent's output for an
instrument expires, it counts as zero for exits and as fully bearish for buys (§8.3). A position
whose thesis expires without invalidation is then held with no end, unless another model exits it.

- Expiry: the thesis expires at its horizon, or it stays until invalidated.
- Renewal: none; the research agent may renew before expiry with fresh evidence; or renewal is
  automatic while no invalidation condition holds.
- Expiry without invalidation: hold the position protected; treat the instrument as removed
  (exits-only, protection stays, discretionary exit under §2.2); or exit at once.

**Recommendation.** Every thesis expires at its horizon. The research agent may renew it before
expiry only through a new `ThesisProposed` with fresh evidence, which passes the admission checks and
the autonomy rules again. A thesis that expires without renewal makes the instrument removed:
exits-only, protection stays, and the discretionary exit follows §2.2. The horizon is also when
DEC-99 scores the thesis, so the position and its score end together.

## 3. Bounds on the research agent's weight

DEC-97 and ADR-0002 part 1 settle how the weight is set: the research agent is one signal model with
a user-confirmed weight, an envelope field the compiler may propose and the user confirms (DEC-47
stands). Its weight decides how far a thesis moves the combined conviction against the other
configured models, and it is the only model with outputs for newly admitted instruments. Open is
only whether anything bounds the user's value.

- No bound beyond the user's confirmation.
- Organizations may cap it through the policy hierarchy (DEC-51).
- A platform minimum, maximum, or both, in addition.

**Recommendation.** Organizations may cap it through the policy hierarchy. No platform bound until
the DEC-99 evaluation gives evidence for one.

## 4. Research-agent cost cap

Model cost grows with the number of agents, instruments, and sources (ADR-0002, "Consequences").

- No cap; cost is metered and billed.
- A cap per agent in tokens per day.
- A cap per agent in dollars per day, as an envelope field.

**Recommendation.** A cap per agent in dollars per day, as an envelope field the user confirms. When
the cap is reached, the research agent proposes no new theses until the next day. Existing positions
are managed normally: their theses stay valid until they expire or are invalidated, and exits,
protection, and kill switches are unaffected.

## 5. Switching into and out of bring-your-own-strategy mode

ADR-0002 part 1 settles the mode itself: pinning the universe disables the research agent and gives
today's behavior, so the pinned universe stays in the hashed, confirmed mandate version under the
existing change classification (DEC-43; E17-4). Open is how a switch between the modes is
classified.

- Both directions are mandate changes with the existing classification of the fields they touch.
- Pinning (the research agent off) is risk-reducing; unpinning (the research agent may admit
  instruments) is risk-increasing and needs step-up.

**Recommendation.** Both directions are mandate versions. Pinning is risk-reducing and applies at
once; the research agent's instruments not in the pinned list become removed instruments (exits-only,
protection stays, discretionary exit under §2.2). Unpinning is risk-increasing and needs step-up,
because it lets the platform admit instruments the owner did not choose.

## 6. The DEC-99 evaluation window, metric, and pass threshold

The Phase 1 exit requires research-agent theses to beat the baselines (buy-and-hold of the eligible
basket, and a broad index ETF) on forward paper trading, net of modeled costs. The window, the
metric, and the pass threshold are set before the evaluation starts.

- Window: a fixed calendar period; a minimum number of closed theses; or the later of both.
- Metric: mean excess return per closed thesis over the same holding window; hit rate; or the
  risk-adjusted return of the thesis portfolio against a baseline portfolio.
- Threshold: a positive point estimate; or a lower confidence bound above zero.

**Recommendation.** The window ends at the later of three months of forward paper trading and 100
closed theses. The metric is mean excess return per closed thesis, net of modeled costs, against each
baseline over the same holding window. The pass threshold is a one-sided 95% lower confidence bound
above zero against both baselines. The E17-0 spike report gives the spread of outcomes needed to
check that 100 theses can show a difference.

## 7. The DEC-100 monitoring thresholds and stagger window

Each workspace's gate stays the only binding control, with its own participation cap (trading domain
spec §9.6). The aggregate-flow monitor sums research-agent exposure per instrument over the
workspaces of its deployment and alerts the operator above a threshold; the operator's per-thesis
halt stops matching admissions and openings; each workspace staggers openings on a new thesis. The
values are set before live trading.

- Alert threshold: 0.5%, 1%, or 5% of average daily dollar volume, over 20 days or another period.
- Dollar threshold: a fixed amount per instrument, or none beyond the volume threshold.
- Stagger window: 5, 15, or 30 minutes inside the regular session.
- Hybrid and on-prem deployments run the same monitor and halt over their own workspaces, operated
  by the customer. Nothing crosses between deployments, and the global control plane carries none of
  it (DEC-10).

**Recommendation.** Alert at 1% of the 20-day average daily dollar volume or 1,000,000 USD per
instrument per deployment, whichever is lower. Stagger over 15 minutes, within the conduct controls.
The operator issues the halt; the monitor never halts by itself, so no workspace's state changes
another's decisions without an operator's action. Revisit the values before live trading.

## 8. The Robinhood paper stage

Robinhood has no paper environment, and E10-4 requires a paper run before going live
([OD-12](04-decision-log.md#open-decisions)).

- The simulated broker, configured with Robinhood's rules (order types, sessions, fractional shares,
  fees).
- Alpaca paper.
- Both.

**Recommendation.** The simulated broker with Robinhood's rules is the required paper stage for
Robinhood users; Alpaca paper is optional for users who also have an Alpaca account. Requiring Alpaca
would make Robinhood users open a second brokerage account. Whether any live Robinhood path is
possible still depends on the OD-12 answers on client order IDs.

## 9. `auto` for retail on paper before counsel answers question 33

DEC-98 stands: the retail profile has `auto_allowed: true`, and every retail user trades paper until
counsel signs off. Counsel question 33 asks what ceilings, disclosures, and checks `auto` needs for
retail live. Open is only what paper `auto` shows the user, so that a paper run does not set an
expectation for live.

- No extra wording.
- The retail profile screen and the go-live screen say that live `auto` depends on counsel's answer
  to question 33 and may come with lower ceilings or be unavailable.

**Recommendation.** Keep DEC-98 as written, with `auto` available to retail on paper now, and add the
wording on the retail profile and go-live screens. The wording is compliance text, so the founder
accepts it (DEC-79). Restricting retail to `ask` would need a decision-log entry superseding that
part of DEC-98.

## 10. How theses are shown to the user

Approval screens show the agent's proposal, the rule that triggered it, and model outputs with
authorship ([compliance](../product/08-compliance-and-regulatory.md), "Posture safeguards").
Notifications stay opaque (`AGENTS.md` rule 6); that part is fixed.

- The full thesis on the approval screen: instrument, direction, horizon, evidence, corroboration,
  invalidation, and confidence.
- A summary, with the full thesis behind "View model output".
- Either, plus the research agent's scorecard.

**Recommendation.** The approval screen shows the full thesis in the §8.2 shape, with evidence and
corroboration as links to the allowlisted sources (DEC-101) and confidence labeled self-reported and
uncalibrated. It shows no price targets or profit estimates. The scorecard stays off approval screens
until counsel answers question 35, because it may count as hypothetical performance. Notifications
carry opaque IDs and generic text only.
