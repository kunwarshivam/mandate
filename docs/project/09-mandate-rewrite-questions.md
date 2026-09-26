# Design questions before the mandate spec rewrite

| | |
|---|---|
| **Owner** | Founder (decides); agents (options and recommendations) |
| **Status** | Open. Each answer becomes a decision-log entry before the spec-change PRs start |

The mandate spec rewrite for [DEC-97](04-decision-log.md#decisions) to
[DEC-103](04-decision-log.md#decisions) ([ADR-0002](../adr/0002-autonomous-ideation-and-retail.md))
depends on the design choices below. `AGENTS.md` ("Separate decisions from defects") asks for them
before drafting, so that review rounds find defects rather than open choices. The founder decides
them; agents give the options and a recommendation. Until a question is answered, agents continue
with its most conservative option (`AGENTS.md` rule 9).

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

## 3. The research agent's weight

The research agent is one signal model with a user-confirmed weight (DEC-47, DEC-97). Its weight
decides how far a thesis moves the combined conviction against the other configured models. It is
the only model with outputs for newly admitted instruments.

- User-set only, like every other weight.
- User-set within platform bounds (a minimum, a maximum, or both).
- User-set, with organizations able to cap it through the policy hierarchy.

**Recommendation.** User-set and confirmed like every envelope field; the compiler may propose a
value, shown as proposed. Organizations may cap it through the policy hierarchy (DEC-51). No
platform bound until the DEC-99 evaluation gives evidence for one.

## 4. Research-agent cost cap

Model cost grows with the number of agents, instruments, and sources (ADR-0002, "Consequences").

- No cap; cost is metered and billed.
- A cap per agent in tokens per day.
- A cap per agent in dollars per day, as an envelope field.

**Recommendation.** A cap per agent in dollars per day, as an envelope field the user confirms. When
the cap is reached, the research agent proposes no new theses until the next day. Existing positions
are managed normally: their theses stay valid until they expire or are invalidated, and exits,
protection, and kill switches are unaffected.

## 5. The pinned universe in bring-your-own-strategy mode

DEC-97 moves the working universe out of the hashed mandate document. In bring-your-own-strategy
mode the owner pins the universe and the research agent is off (E17-4).

- The pinned universe stays inside the hashed, confirmed mandate version, with the existing change
  classification (adding an instrument is risk-increasing and needs step-up).
- The pinned universe is runtime state, as in ideation mode.

**Recommendation.** Keep it inside the hashed, confirmed mandate version with the existing change
classification. The owner chose those instruments, so a change to them is a mandate change, as it is
today (DEC-43).

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

## 7. The DEC-100 cap values and stagger interval

The risk gate caps aggregate research-agent exposure per instrument across all workspaces, in
dollars and as a share of the instrument's average daily dollar volume, and staggers one thesis's
orders across accounts. The values are set before live trading.

- Volume cap: 0.5%, 1%, or 5% of average daily dollar volume, over 20 days or another period.
- Dollar cap: a fixed amount per instrument, or none beyond the volume cap.
- Stagger: a fixed interval between accounts; or a random delay per account, spread over a window
  inside the regular session.
- Hybrid workspaces run on the customer's site, and the global control plane holds no positions
  (DEC-10). The aggregate cap needs their exposure: they report it, or they are outside the research
  agent until a design keeps trading intent on site.

**Recommendation.** Start at 1% of the 20-day average daily dollar volume or 1,000,000 USD per
instrument, whichever is lower. Stagger with a random delay per account, spread over 15 minutes and
within the conduct controls. In v1 the research agent runs for managed workspaces only; hybrid use
waits for a design that sends no trading intent to the control plane. Revisit the values before live
trading.

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

## 9. `auto` for retail before counsel answers question 33

DEC-98 sets `auto_allowed: true` in the retail profile. Counsel question 33 asks what ceilings,
disclosures, and checks `auto` needs for retail. Until counsel signs off, every retail user trades
paper.

- Allow `auto` for retail on paper now.
- `ask` only for retail until counsel answers question 33.

**Recommendation.** `ask` only for retail until counsel answers question 33; DEC-98's
`auto_allowed: true` applies afterwards if counsel agrees. Users who run `auto` on paper would expect
it live, and waiting costs little while every retail user trades paper.

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
