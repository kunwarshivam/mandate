# Product Experience Brief (web, v1)

| | |
|---|---|
| **Owner** | Product (web stream W1) |
| **Status** | Draft v0.1. The founder decided to start web design ahead of M9 ([DEC-134](../project/04-decision-log.md#decisions)); the product decisions in §6 were decided by the founder on 2026-09-27 ([DEC-135](../project/04-decision-log.md#decisions), [DEC-136](../project/04-decision-log.md#decisions)) |
| **Related** | [PRD](04-prd-v1.md) · [Personas](02-personas-and-journeys.md) · [Compliance](08-compliance-and-regulatory.md) · [HLD](../HLD.md) · [Mandate spec](../specs/mandate.md) · [Trading domain spec](../specs/trading-domain.md) · [ADR-0002](../adr/0002-autonomous-ideation-and-retail.md) · [ADR-0003](../adr/0003-earned-autonomy.md) |
| **Additions** | 2026-09-30, from [ADR-0003](../adr/0003-earned-autonomy.md): goal-first onboarding (A0, the contract card on A2), delegations on the approval screen (D6), the plan view (D12), the daily brief (D13), the chat thread (D14), autonomy settings (D15), and PX-15 to PX-18. The founder accepted [DEC-181](../project/04-decision-log.md#decisions) (delegations) and PX-15 to PX-18 ([DEC-198](../project/04-decision-log.md#decisions)) on 2026-09-30. 2026-10-08, from the founder's [DEC-528](../project/decisions/DEC-528.md): the journey from Home (signed-in, D1) with connected accounts (J-H), holdings (H1), instrument search, the instrument page and watchlists (I1 to I3), and the monitor agent (D16); all after the first paper trade |

This brief records the product-experience decisions that the designs and, later, the web code are
built from: the principles that settle trade-offs, the journeys, every screen with its states, the
interface rules that follow from the safety rules, the decisions only the founder can make, and the
order in which designs are needed. It is design and decisions, not code. `web/` stays at M9
([ADR-0001](../adr/0001-engineering-setup.md) ES-01). [HLD §11](../HLD.md#11-technology) names
Next.js and React Native for the web app and mobile approvals; that has not yet been ratified as a
decision, and this brief does not ratify it (§6, PX-14).

Where this brief and a spec disagree, the spec wins and the brief is wrong; §7 lists the places where
designing the screens found a gap in the specs.

## Contents

1. [Principles](#1-principles)
2. [Personas and journeys](#2-personas-and-journeys)
3. [Screen inventory](#3-screen-inventory)
4. [Shared patterns](#4-shared-patterns)
5. [UX rules derived from the safety rules](#5-ux-rules-derived-from-the-safety-rules)
6. [Open product decisions for the founder](#6-open-product-decisions-for-the-founder)
7. [Questions for the specs](#7-questions-for-the-specs)
8. [What the designs must cover first](#8-what-the-designs-must-cover-first)
9. [Convergence with the designs](#9-convergence-with-the-designs)

## 1. Principles

These principles decide trade-offs. When two of them conflict, the lower number wins.

| # | Principle | What it means on screen | Traced to |
|---|---|---|---|
| P1 | **Reducing risk is always within reach and never blocked by the interface.** | Pause, close, and the kill switch are on every screen where they apply, and loading, stale data, errors, and pending approvals never disable them. The spec requires step-up for an owner exit; nothing else stands in front of it | [AGENTS.md](../../AGENTS.md) rules 2 and 13; [trading §1](../specs/trading-domain.md#1-principles) principle 4; [trading §5.5](../specs/trading-domain.md#55-kill-switch); DEC-05, DEC-48 |
| P2 | **The owner sets the envelope, and can see that they did.** | Every envelope field shows where its value came from: the owner's words (with the quoted span), the owner's entry, a template, a platform proposal, or a platform default. A proposed value looks inactive until the owner confirms it. `auto` is never proposed, never preselected, and never offered as a chip. The one offer is a delegation scope on an approval screen (D6), which the owner picks or ignores, with none preselected (mandate §6.4, DEC-181) | Rule 11; [mandate §2.1](../specs/mandate.md#21-provenance-and-confirmation), [§7](../specs/mandate.md#7-compiler-and-platform-proposals-dec-97); V-020, V-022, MI-12; DEC-97 |
| P3 | **The platform explains and never persuades.** | The platform does originate ideas (DEC-97), so the interface does not pretend otherwise: a thesis is labeled platform-authored. But no screen says "recommended", estimates profit, sets a price target, ranks models, or shows a scorecard beside a decision. Approve and Skip carry equal weight | [mandate §6.4](../specs/mandate.md#64-approvals), [§8.1](../specs/mandate.md#81-signal-model-contract-dec-52-dec-97); DEC-52, DEC-126; compliance questions 26 and 35 |
| P4 | **Silence is safe, and the screen says so.** | Every request shows what happens if the owner does nothing ("If you do nothing, this action is skipped"). A timeout, a lost connection, or a failed step-up never turns into an approval | Rule 3; DEC-06; FR-6.6 |
| P5 | **Notifications carry nothing about trading.** | Push, email, SMS, and chat carry an opaque ID and generic text only: no instrument, size, price, thesis, or agent name. The same applies to anything a browser or operating system may copy elsewhere: URLs, page titles, and cached pages | Rule 6; DEC-11; FR-6.4 |
| P6 | **Show the state truthfully, including uncertainty.** | Stale data is labeled with its age; an `Unknown` order is shown as unknown; a degraded dependency is named. The interface never shows a calm "all good" it cannot prove. Unknown means stop, and the screen shows it stopped | [trading §1](../specs/trading-domain.md#1-principles) principle 3; [mandate §5.2](../specs/mandate.md#52-inputs-the-risk-clock-and-determinism) staleness |
| P7 | **Limits are visible where decisions are made, in dollars.** | Each limit appears as a dollar amount with its headroom, next to the action it limits. Policy ceilings appear as limits and are never pre-filled as values. `profit_stop` is a level where the agent stops, never a progress bar | [mandate §4.2](../specs/mandate.md#42-warnings-and-the-confirmation-screen), [§4.3](../specs/mandate.md#43-policy-hierarchy-dec-51-dec-98), [§3.1](../specs/mandate.md#31-goals-and-stop-conditions-dec-46-dec-59) |
| P8 | **What the owner saw is what gets recorded.** | The confirmation, go-live, approval, owner-exit, kill-switch, and release screens are *record screens*: rendered deterministically, stored as an artifact with the UI build, and frozen once shown (§4.1) | [mandate §10](../specs/mandate.md#10-records-dec-51-dec-97); `MandateConfirmed`, `AgentDeployed`, `OwnerExitRequested` |
| P9 | **Paper and live can never be confused.** | The environment is on every screen that shows an agent, a connection, or money. No live path is built into any flow until counsel signs off; in the designs, live exists only as a blocked state | Rule 8; DEC-98; V-031 |
| P10 | **Ask only when it matters, and make the answer fast.** | An approval is decidable in under a minute on a phone. Escalations are few, so each one gets the evidence it needs and nothing else | [Vision](01-vision-and-strategy.md#product-principles) principle 3; journey J2; PRD §8 (median response under 5 minutes) |
| P11 | **Every surface compiles to the mandate or is checked against it.** | The goal questions, the contract card, a delegation, the dial, away mode, and a chat message asking for a change each produce a mandate version the owner confirms. An approval, an owner request from chat or a connected agent, the plan view, and the brief act under the current version or read it. No screen holds state that steers the agent, and each shows which mandate field it reads or writes | Rule 11; [mandate §2](../specs/mandate.md), [§6.5](../specs/mandate.md#65-delegations-dec-181-adr-0003), [§7](../specs/mandate.md#7-compiler-and-platform-proposals-dec-97); ADR-0003 |

## 2. Personas and journeys

### 2.1 Who we design for first

| Persona | Role in the product | Why they shape v1 design |
|---|---|---|
| **Jordan**, retail investor (managed) | Operator and approver of their own agents | **Every workspace is retail unless shown otherwise** (DEC-68), so the retail profile is the default experience: paper only until counsel signs off, protection required, no leveraged ETPs, and no research agent until the DEC-99 evaluation passes ([mandate §4.3](../specs/mandate.md#43-policy-hierarchy-dec-51-dec-98)). Education and disclosures live here |
| **Alex**, professional systematic trader | Operator and approver | Journey J1: idea to agent with hard limits. Wants few pings, each one decidable in seconds |
| **Priya**, emerging manager | Workspace admin and approver | Journey J2: approving from a phone with evidence, two approvers, and separation of duties; a hybrid deployment |
| **Dana**, compliance | Auditor (read-only) | Journey J3: from a fill back to its causes in one query; export |
| **Sam**, IT | Org or workspace admin | Journey J4: hybrid install, SSO, health |

[PRD FR-1.3](04-prd-v1.md#61-identity-and-tenancy) lists the roles only; what each role sees and may
do is this brief's proposal (PX-11): roles decide who may act, not what is shown, so a viewer sees an
agent's mode and limits but has no Stop control, and an auditor sees the journal and exports only.

### 2.2 The research agent is gated, so design both modes

Users' agents get the research agent only after the DEC-99 forward-paper evaluation passes
([mandate §8.4](../specs/mandate.md#84-the-research-agent-dec-97-adr-0002), thin slice). Until then
every user agent is **bring-your-own-strategy** (a pinned universe). The designs cover both modes;
what the product shows about the research agent before it is available is PX-9.

### 2.3 Journeys

Each journey lists its steps with the screens of §3 in brackets, and the moment that matters.

**J-A. Onboarding and a paper account** (Jordan, Alex)

1. Sign up with a passkey or OIDC [O1]. Create an organization and workspace; the policy profile
   screen explains the retail profile and its ceilings, with the counsel-owned wording on live
   `auto` (DEC-125) as a placeholder [O2].
2. Connect Alpaca paper through OAuth [O3]. The result screen states the scopes granted (trading and
   account read, never transfers) and the 1× check. A rejected connection says why and what to do,
   without echoing any secret. Robinhood users get the simulated broker with Robinhood's rules as
   their paper stage (DEC-124), labeled as simulated.
3. Land on an empty dashboard whose one action is "Set up your first agent" [D1], which opens the
   three goal questions [A0] (ADR-0003, DEC-182).

*Moment that matters:* the owner sees, in words, that the platform cannot move their money.

**J-B. Authoring a mandate** (Alex, Jordan)

1. **Goal first** [A0] (DEC-182), as one conversation (DEC-476): three questions, how much money,
   what the goal is, and how much the owner can stand to lose, each answered in the owner's own
   words or numbers, one at a time or all at once [A1]. Templates stay available for owners who
   want to write it.
2. The compiler returns the mandate [A2], led by the **contract card**: the whole envelope in plain
   language on one card, with the three answers quoted, every value the compiler drafted marked
   *proposed*, and the dollar figures of step 5. Nothing proposed is active until confirmed. Below
   the card, each field carries a provenance badge: *stated by you* (the
   quoted span of the owner's words), *entered by you*, *from template* (`template_structure`, present
   because the template included the field or rule), *proposed by the platform — confirm or change*,
   or *platform default*. Constraints the mandate cannot express are flagged **not enforced**
   ([mandate §7](../specs/mandate.md#7-compiler-and-platform-proposals-dec-97)).
3. The owner edits in the form or YAML [A3]; both stay in sync (FR-3.2).
4. Validation [A4]: errors list every violated rule; a policy error names the nearest ancestor limit
   (mandate §4.3). Each warning (W-001, W-002, W-003, W-005, W-006) is acknowledged individually.
5. Confirmation [A5], a record screen: the plain-language summary; in dollars, one position's loss
   at its stop, the daily loss budget, the loss at which the agent flattens, and the lifetime-floor
   loss; the statement that gaps and exit pricing can exceed each of them; `scale_action` in plain
   words; with a research agent, that the platform chooses what to propose within the envelope and
   that the agent's confidence is self-reported and uncalibrated (DEC-126). Per-section confirmation,
   then step-up [G3]. The confirmation binds the version hash. For a new agent, A2 and A5 are one
   summary card at the end of the conversation, confirmed once, with every drafted value marked
   (DEC-477).

*Moment that matters:* the compiled mandate reads exactly like what the owner meant, and nothing the
platform proposed became active without the owner's hand on it. In the goal-first path, the owner
answered three questions and still confirmed every value the platform drafted.

**J-C. Backtest, paper, go-live** (Alex)

1. Backtest the version [B1] and read the report [B2]: return, volatility, Sharpe, maximum drawdown,
   turnover, fees, against buy-and-hold, under the hypothetical-performance legend. For a research
   agent the report states that a backtest tests mechanics, not thesis quality (DEC-99).
2. Deploy to paper [B3] (step-up) and watch the paper run [B4, D2]. The readiness report compares
   backtest, paper, and estimated live costs (trading §10).
3. Go live [B5], a record screen: the backtest and paper-run IDs, the disclosures to accept, the
   legend, step-up, and the independent approver where policy requires one. **Blocked for every user
   until counsel signs off** (DEC-98): the screen exists in the designs, in its blocked state. Because
   `environment` never changes across versions (V-031), going live creates a new live mandate; how
   that looks is PX-8.

*Moment that matters:* the owner knows what was proven and what was not.

**J-D. Living with an agent** (Alex, Priya, Jordan)

1. **Monitoring** [D1, D2, D3]: each agent's effective mode, every active restriction with how it
   lifts (§4.3), limits with headroom in dollars, rungs whose breach is still confirming (`pending`),
   positions with their protection, and the working universe or pinned list [D4].
2. **An approval under a deadline** [G4, D6]: a generic notification ("An agent needs your
   approval"); the approver signs in, the details load from the workspace deployment, and the screen
   shows the proposed action, the rule that asked, the combined score labeled "combined model score,
   not a probability of profit", the deadline, and "If you do nothing, this action is skipped". Live
   approvals need step-up within the 5 minutes before the response
   ([mandate §6.4](../specs/mandate.md#64-approvals)). The result screen shows what happened next: the
   gate re-ran and the order was submitted, or it was skipped with the reason.
3. **A rung is reached** (persona journey J5) [G5, D2]: sizes halve automatically and the owner is
   told; at `exits_only` the owner acknowledges with step-up, which resets the high-water mark [D7];
   the screen says sizes return in steps (DEC-57).
4. **A reconciliation pause** [G5, D7]: the agent is `paused` because the broker and the ledger
   disagree. The owner sees the difference, what still runs (resting protection) and what does not
   (new orders, the agent's own exits), and resumes only by acknowledging with step-up
   ([trading §11](../specs/trading-domain.md#11-reconciliation)). What exactly the owner sees is PX-5.
5. **External activity or an account restriction** [G5, D7, O4]: every agent on the account goes to
   `exits_only` (or `paused` if blocked) until the owner acknowledges
   ([trading §7.1, §7.3](../specs/trading-domain.md#73-account-restrictions)).
6. **The lifetime floor** [D8]: flattened and paused. There is no acknowledge button, because none
   exists in the spec; the only path is a version that loosens `max_loss_from_allocation`, with
   independent approval and, in a single-user workspace, a waiting period
   ([mandate §5.7](../specs/mandate.md#57-lifetime-loss-floor-dec-44-dec-55)).
7. **Stop asking me about this** [D6, D15] (DEC-181): beside Approve and Skip, the approval
   screen offers to approve this one and let the agent go ahead with the same kind of action for a
   while, within caps the screen states in dollars, orders, and days. "Approve just this" comes
   first; no scope is preselected. Later actions under the delegation run without asking and show
   the delegation on their record; the owner sees every delegation, its use so far, and its expiry
   on D15 and ends any of them there with one action, no step-up (ending one reduces risk).
8. **What is it doing?** [D12]: the plan view says, in words, what the agent is watching, what it
   would do next and under which rule, whether that would run automatically or ask, and what would
   stop it.
9. **The daily brief** [D13]: one summary a day of what ran, what was skipped, what the delegations
   let through, and what changed. Everything that does not need the owner waits for it (PX-16).
10. **Talking to the agent** [D14]: the owner writes in a chat thread. A message becomes either an
   owner request, shown as a card with the gate's verdict and the autonomy outcome before anything is
   sent, or a proposed envelope change, which opens A6 with step-up. The agent's replies are quoted
   model output, never the platform's voice (rule 4).
11. **From another agent** (E10-6, DEC-183): an owner who works in OpenAI Dots, Meta Muse, or Grok
   Bot connects Owlhead there. That agent can read status, the plan, and the brief, ask for an
   order, and propose an envelope change, exactly like step 10; it can never approve, confirm, or
   pick a delegation, so every such request lands in the owner's approvals inbox [D5] or on A6.

*Moment that matters:* the system protected capital before the owner read the message, and the
screen says exactly what the owner may now do. As the owner delegates more, they still see what ran
without them and why, and can take it back in one tap.

**J-E. Stopping** (any operator)

1. **Pause** [G2]: instant; the screen says that resting protection stays and that the agent will not
   exit by itself while paused ([trading §7.4](../specs/trading-domain.md#74-agent-modes)).
2. **Close a position** [D9]: an `owner_exit`. In the regular session it goes out paced by the
   participation caps. Outside it, the owner confirms the displayed bid, bid size, and a floor price;
   the ladder never prices below the floor, and any remainder rests at the floor and waits for the
   session ([mandate §6.1](../specs/mandate.md#61-purposes), DEC-66).
3. **Kill switch** [G2, D10]: the scope is chosen explicitly. An agent kill switch cancels only that
   agent's orders and sells exactly its shares, is terminal (`stopped`), and needs step-up. A
   connection or workspace kill switch uses the broker's cancel-all and close-position; the
   confirmation lists what that touches (PX-12).
4. **Goal complete / Holding** [D11]: release (the positions become the owner's and **unprotected**;
   step-up; the warning shown is recorded) or close.

5. **Stop** [G2, D2]: ends the agent, terminal, step-up (the brief's addition, because it cannot be
   undone), and offered only when the agent is flat (DEC-136, PX-13 (a)). An agent that holds
   positions offers instead the kill switch (close and stop) or "stop and release positions to me"
   (as `release`: the positions become the owner's and **unprotected**; step-up; the warning shown
   is recorded).

*Moment that matters:* in a panic, the owner reaches the right stop in two taps and knows its scope.

**J-F. Audit** (Dana)

1. Open a fill in the audit explorer [J2] and follow the causal trace: fill → order → approval →
   decision → signal-model outputs (and the thesis) → observations.
2. Open any gate decision [J6], allows included, with every check and the quotes and marks used.
3. Filter an agent's timeline [J1], export the period as JSON or CSV [J3], verify the chain [J4].
4. Review the daily surveillance report and its acknowledgments [J5].

*Moment that matters:* a complete answer without asking an engineer.

**J-G. Administration** (Sam, Priya)

Policies that only tighten [X1], members and roles [X2], notification channels, escalation chains,
and quiet hours [X3], deployment health [X4], billing [X5], and disclosures [X6].

**J-H. Home, finding an instrument, and asking an agent** (Jordan, Alex;
[DEC-528](../project/decisions/DEC-528.md), after the first paper trade). "Home" here is the
signed-in Home (D1); the landing page stays the public site (DEC-212, DEC-213).

1. **Arrive** [D1, H1]. The owner signs in with one or more connected accounts. Home (signed-in, D1)
   shows:
   - each account and its holdings, read-only, with their age;
   - "No agents deployed".

   A holding no agent bought is shown as the owner's, never as an agent's (CN-7). For Robinhood,
   only the agentic account appears (CN-8).
2. **Find an instrument** [I1, I3]. The owner searches by name or ticker, or opens one of their own
   watchlists. Results are ordered by how they match the query, then alphabetically (*proposed*,
   DEC-528). Nothing is ranked, scored, flagged or suggested.
3. **Open it** [I2]. The instrument page shows facts: name, ticker, asset class, the owner's
   holdings in it, and which of the owner's agents watch or hold it. *Proposed, left to E11-11's
   spec change:* whether an agent may trade it (the eligibility floor's outcome, in words).
4. **Ask an agent, or set one up** [D14, A0]. The page offers no "buy" (§5, Rule 12). Each choice
   leads to the existing journeys:
   - **Ask an agent** opens the chat thread of an agent that already has this instrument in its
     universe, as an owner request: the builder, the gate and the autonomy rules decide (DEC-141).
   - **Set up an agent** opens the ordinary goal questions (A0). The owner then picks a
     goal-seeking agent (J-B) or a monitor agent [D16], which watches and alerts and never opens.
     The instrument enters an agent only through a mode the owner already confirms in a mandate
     version (a pinned universe, bring-your-own-strategy), never as a universe hint.
5. **An agent over what the owner already holds** is not offered: adoption waits on cost basis and
   tax lots (DEC-46, DEC-528 item 3). The page says the holding stays the owner's.

*Moment that matters:* the owner finds what they came for and sees, in words, that every buy goes
through an agent they confirmed.

## 3. Screen inventory

### 3.1 States every screen handles

| State | Meaning | Default treatment (a screen lists only where it differs) |
|---|---|---|
| **Empty** | Nothing exists yet | One sentence and the single next action; never sample data that looks like the owner's |
| **Loading** | Data not yet received | Skeleton; never last session's values shown as current; Stop controls stay live |
| **Stale** | Data older than its freshness limit (market data per the data profile, broker state, ledger) | The value stays, marked with its age ("as of 14:02:11, 3 min ago"); no green "healthy" |
| **Paused** | The agent's effective mode is `paused` | A mode banner naming every restriction and how each lifts (§4.3) |
| **Restricted** | `exits_only`, an instrument restriction (`stale_mark`, `removed_instrument`), or an account restriction | The same banner; opening actions explained rather than hidden |
| **Error** | A request failed | What failed, whether anything changed ("nothing was sent" or "the result is unknown; we are checking"), and the next step. Never "try again" on an action whose result is unknown |
| **Degraded** | A dependency is down: the global control plane, the workspace deployment, the broker, market data, or the model gateway | A status-strip item naming the dependency and what still works. Trading continues without the global control plane (HLD); model outages never enlarge a buy (MI-10) |

### 3.2 Screens

⚠ marks a **safety-critical screen**: one where a wrong design could add risk, hide a limit, or read
as advice. A design for a ⚠ screen is checked against the spec clauses it cites before it is
accepted (§9).

#### Global

**G1 App shell** ⚠
- *Shows:* workspace switcher; the **environment badge** (paper or live) wherever an agent, connection,
  or money appears; the status strip (market data, broker, workspace deployment, global control
  plane); the approvals badge (a count only); the **Stop control**.
- *Primary action:* Stop (opens G2).
- *States:* Degraded: a strip item per dependency. If the workspace deployment is unreachable, the
  shell shows no agent data and says so; Stop stays visible and reports that it cannot reach the
  deployment rather than failing silently, with how to reach the broker directly.
- *Governs:* trading §5.5 (kill switch always available); HLD "Behavior when the global control plane
  is unavailable"; P1, P9.

**G2 Stop sheet** ⚠
- *Shows:* in the current context (agent, connection, workspace, organization; HLD §8), the three
  ways to stop, each with its scope in words: **Pause** (no new orders; protection stays; reversible;
  no step-up), **Stop** (agent scope only: ends the agent, terminal, sells nothing, and is offered
  only when the agent is flat, DEC-136; step-up, the brief's addition because it cannot be undone),
  and **Kill switch** (cancel and flatten, terminal; step-up, as the spec requires for an owner exit).
- *Primary action:* Pause, the least drastic, is first; Stop and Kill switch follow as distinct
  choices.
- *States:* Paused agent: offers Resume, Stop, and Kill switch. Stopped by a kill switch: shown as
  done. Agent holding positions: Stop is replaced by two choices, the kill switch (close and stop)
  and "Stop and release positions to me" (as `release`: the positions become the owner's and
  **unprotected**; step-up; the warning shown is recorded; DEC-136, PX-13 (a)). Loading and Stale:
  every action stays enabled.
- *Governs:* trading §5.5, §7.4; PRD FR-8.2; DEC-131 (owner Pause, Resume, Stop) as amended by
  DEC-136; PX-3, PX-4, PX-11, PX-13.

**G3 Step-up dialog** ⚠
- *Shows:* the action being authorized, in one line, and the passkey prompt.
- *Primary action:* Verify.
- *States:* Error (cancelled or failed): the action did not happen, and the screen says so; for an
  approval, the default still applies at the deadline.
- *Governs:* FR-1.4; mandate §6.4 (step-up within the 5 minutes before a live approval response);
  PX-7.

**G4 Notification landing** ⚠
- *Shows:* nothing but a sign-in, until the details load from the workspace deployment; then it
  routes to the target screen by opaque ID.
- *Primary action:* Sign in / Open.
- *States:* Expired: the request was skipped, with the time. Degraded (hybrid, deployment not
  reachable, for example off VPN): "Cannot reach your workspace", with no content cached.
- *Governs:* rule 6; DEC-11; FR-6.4; HLD flow C step 5; OD-05.

**G5 Alerts center**
- *Shows:* rung reached, reconciliation mismatch, stale feed, agent paused, external activity,
  account restriction, 1× check failed, model withdrawn, lineage retired, policy nonconforming, and
  the owner-exit remainder resting at its floor (DEC-66). On the safety side, the alerts that say a
  position was briefly unprotected or an exit stalled: an unprotected interval that reached
  `max_unprotected_s` and was canceled with protection re-placed (trading §5.4); the triggered-stop
  watchdog exiting a position whose stop did not fill (trading §5.4); any exit resting at the exit ladder's
  floor, whatever its purpose (trading §5.6); a cash reconciliation difference above threshold (trading §11). The
  crypto stop-limit gap disclosure (a stop-limit may not fill on a gap, trading §5.4) appears on D3 and
  on the confirmation screen when crypto is allowed.
- *Primary action:* open the screen that resolves the alert.
- *Governs:* FR-8.3; mandate §3.1 (Holding alert), §8.1 (model withdrawn), §8.6 (lineage retired);
  trading §5.4, §5.6, §7.2, §11.

#### Onboarding

**O1 Sign up and sign in**: passkey or OIDC (FR-1.1). Error states never reveal whether an email
exists.

**O2 Organization, workspace, and policy profile** ⚠
- *Shows:* the profile the workspace has (retail unless verified otherwise, DEC-68), its ceilings as
  limits, and the placeholder `[[RETAIL-AUTO-LIVE]]` for the DEC-125 wording.
- *Primary action:* Continue.
- *Governs:* mandate §4.3; DEC-98, DEC-125.

**O3 Connect account** ⚠
- *Shows:* the broker choice (Alpaca paper in v1; Robinhood's simulated paper stage when its connector
  exists, DEC-124), the OAuth handoff, and the result: scopes granted, the 1× check, the data profile.
- *Primary action:* Connect with Alpaca.
- *States:* Rejected (scopes beyond trading, or a key with transfer rights): the reason and how to
  create a trading-only key, never the key itself (FR-2.2). 1× check failed: agents cannot deploy;
  the owner is shown how to set 1×.
- *Governs:* FR-2.1, FR-2.2, FR-2.4, FR-2.6; trading §7.2; rule 7.

**O4 Connection detail** ⚠
- *Shows:* scopes, account state and restrictions (`blocked`, `closing_only`), the day-trading regime,
  the data profile, agents granted and their instrument groups, external activity awaiting
  acknowledgment, and the loss carry of recently retired agents (mandate §5.7).
- *Primary action:* acknowledge a restriction or external activity (step-up) [D7]; the connection
  kill switch.
- *States:* Restricted: every agent on the account shown with the mode the restriction imposed.
- *Governs:* trading §7.1, §7.3; FR-2.3, FR-2.6, FR-2.7.

#### Authoring

**A0 Goal questions** ⚠ (DEC-182, DEC-476, DEC-477)
- *Shows:* one conversation with one composer, as in any chat. The platform asks one question at a
  time: how much money, what the goal is, and how much the owner can stand to lose, then the
  symbols, the model, and each of its settings. The owner answers in their own words or numbers,
  one question at a time or all at once (A1), and is asked only what is still missing. The
  platform answers in plain sentences: what it understood ("Got it: …"), what it refused and why,
  then the next question. No example returns, no suggested amounts.
- *The model only points* (DEC-476): a model reads each message and returns readings that quote
  the owner's words; a reading is kept only when deterministic code reads the same value back from
  the quote. A value written in words, not figures, is read and said back once ("I read “about
  three grand” as $3,000.00.", DEC-477). The model's own reply is plain text in the platform's
  reply, with no buttons (PX-18), and one that reads as advice is withheld.
- *Writes:* `capital.allocation_usd`, `goal` (one of the goal types, with `on_complete`), and
  `capital.max_loss_from_allocation`, each `user_stated` with its quoted span; the compiler drafts
  the rest as `platform_proposed` (never `auto` or a delegation, V-022).
- *Primary action:* Send. Once nothing is missing, the conversation shows A2 and A5 as one card.
- *States:* A goal the types cannot express: kept as description text and flagged **not enforced**
  on A2. A loss answer above the policy ceiling: the ceiling shown as a limit, never substituted;
  the answer is refused with the reason. More money than the account has free (V-002), or a symbol
  another agent trades (V-006): refused when said, with the reason.
- *Governs:* mandate §2.1, §7; V-002, V-006, V-020, V-022, V-038; compliance question 36.

**A1 Describe**: the same conversation, answered all at once (DEC-476), plus an optional template
(FR-3.8). The opening message explains what a mandate is, without example returns.

**A2 Compiled review** ⚠
- *Shows:* every envelope field grouped by section (goal, capital, universe, behavior, sizing,
  protection, risk, autonomy, notifications), each with its provenance badge (the five of J-B step 2,
  *from template* included) and, for a stated value, the quoted span. Proposed values look inactive
  until confirmed. **Not enforced** constraints listed apart, with the note that they reach models
  only as description text. For a new agent (DEC-477), A2 and A5 are one summary card in the
  conversation: every value in compact form, each drafted one marked *proposed* or *default*.
- *Primary action:* Create agent, once, with a passkey (DEC-477). Saying a change shows a new card,
  and the earlier one can no longer be created.
- *States:* Loading (the compiler is a model call): "Reading your message…", and no second message
  until it answers; never partial fields shown as final. Error (compiler output fails the schema):
  the reply says none of it was used. Degraded (the model does not answer): the owner's words are
  kept, with Try again and "Continue without the model", which reads figures only. The account
  changed since a value was said (V-002, V-006): the card says why it cannot be created, with no
  button.
- *Governs:* mandate §2.1, §7; V-020, V-022, V-038; FR-3.1, FR-3.4; PX-1, PX-2.
- *Contract card* (DEC-182): the whole envelope in plain language on one card above the fields: the
  three answers quoted, every drafted value marked *proposed*, the dollar figures of A5, and the
  **unasked dollars** (DEC-189): what could trade without asking once confirmed. It is a rendering
  of the mandate, not a separate object.

**A3 Form and YAML editor**: both views of one document (FR-3.2). The model picker lists
methodology only, with no ranking, no "recommended", no scorecard (FR-8.4, DEC-52); parameters have
no defaults, so their fields start empty. Policy ceilings appear as hints ("limit: at most …"),
never as values (mandate §4.3).

**A4 Validation and warnings** ⚠
- *Shows:* every violated code in plain language; a policy error names the key, the value, and the
  nearest ancestor it breaks (mandate §4.3); each warning with its own acknowledgment. W-006 names the
  eligibility floor, `max_instruments`, and the position and daily-loss limits as what still bounds an
  automatic admission.
- *Governs:* mandate §4.1, §4.2, §4.3.

**A5 Confirmation** ⚠ (record screen)
- *Shows:* §2.3 J-B step 5; platform defaults marked "platform default", proposals marked "proposed
  by the platform — confirm or change"; `on_complete` in words; for `profit_stop`, the level at which
  the agent stops; when `universe.asset_classes` includes crypto, the disclosure that a stop-limit may
  not fill on a gap (trading §5.4). For a new agent, the summary card at the end of the
  conversation (DEC-477), with the marks *proposed* and *default*.
- *Primary action:* Confirm (step-up); Create agent for a new agent.
- *States:* Error after step-up: nothing was confirmed; the version does not exist yet.
- *Governs:* mandate §2.1, §4.2, §10 (`MandateConfirmed` stores the rendered screen and UI build);
  compliance questions 23 and 24; PX-1, PX-2.

**A6 Versions and diff** ⚠
- *Shows:* versions, the deployed one, and a diff whose every changed path carries its classification
  (risk-increasing, reducing, neutral); when it applies ("now" for reducing and neutral; "at the next
  safe point" for increasing); that pending approvals will be canceled and re-proposed.
- *Primary action:* Confirm the new version (step-up if risk-increasing; independent approval where
  policy requires it). The version comes from Mandate › Edit or from the owner's words in a thread
  or the copilot; both show the same review (DEC-483).
- *States:* Rejected at application (for example an allocation increase while a limit is latched, or
  `waiting_period`): the reason in words. Policy nonconforming: a conforming version is required
  before any risk-increasing change.
- *Governs:* mandate §2.2, §5.1, §9.2; FR-3.5.

#### Prove it

**B1 Backtest run**: the version and data snapshot, then progress. Error names the missing data.

**B2 Backtest report**
- *Shows:* FR-4.2 metrics against buy-and-hold, fees and slippage assumptions, the legend placeholder
  `[[LEGEND-HYPOTHETICAL]]`. For a research agent: that the backtest tests mechanics, not thesis
  quality (DEC-99), and no thesis-quality metric.
- *Governs:* FR-4.1, FR-4.2, FR-4.5; DEC-99; compliance questions 10 and 35.

**B3 Deploy to paper** ⚠: the version, the backtest shown, step-up. Rejection states: loss carry
(V-032), a claimed instrument group (V-006), allocation above account equity (V-002).

**B4 Paper run and readiness**: the paper agent's live view (as D2) plus the readiness report
(backtest, paper, and estimated live costs; simulated fees and dividends from the shadow ledger
labeled simulated; trading §10). Paper results are labeled "paper — simulated funds".

**B5 Go-live** ⚠ (record screen)
- *Shows:* the mandate version, the backtest and paper-run IDs shown, the legend and disclosure
  versions, `[[RETAIL-AUTO-LIVE]]` for retail, the consolidated (SIP) data requirement, step-up, and
  the independent approver.
- *Primary action:* Go live (step-up).
- *States:* **Blocked** (the only state reachable today): "Live trading is not available until
  counsel signs off", with no countdown and no waitlist framing that implies a date.
- *Governs:* FR-4.4, FR-2.7; mandate §10 (`AgentDeployed`); DEC-98, DEC-125; PX-8.

#### Operate

**D1 Dashboard**
- *Shows:* agents with environment, effective mode, positions, P&L (paper labeled simulated) with the
  disclosure placeholder `[[DISCLOSURE-PERFORMANCE]]` beside it (the user's own historical results with
  disclosures, compliance), open approvals with deadlines, recent decisions, and alerts.
- *Primary action:* open an agent or an approval.
- *States:* Empty: "Describe your first agent"; once accounts are connected, the empty state also
  shows them and their holdings (H1) and says "No agents deployed" (DEC-528). Stale: P&L marked
  with the mark's age. Degraded: named per dependency.
- *Governs:* FR-8.1; FR-8.5.

**D2 Agent detail** ⚠
- *Shows:* the effective mode and **every active restriction with what it blocks and how it lifts**
  (§4.3); P&L with the disclosure placeholder `[[DISCLOSURE-PERFORMANCE]]` beside it, as on D1;
  limits with headroom in dollars; ladder rungs, the active size factor, and any `pending`
  breach confirmation; the lifetime floor in dollars; positions with protection status; the working
  universe [D4] or the pinned list; open approvals; recent decisions linking to the trace [J2].
- *Primary action:* Stop control (G2: Pause, Stop, Kill switch); the acknowledgment the current
  restriction needs, if any [D7]. The owner Stop is offered only when the agent is flat; while it
  holds positions, G2 offers the kill switch or "stop and release positions to me" (DEC-136; PX-13).
- *States:* Paused, Restricted: banner per §4.3. Recovering (`awaiting_reconciliation` at startup):
  "Checking with the broker", no action needed. Holding: see D11. Stopped: read-only, with the reason;
  an owner Stop leaves no positions with the agent, since it needs the agent flat or its positions
  released to the owner (DEC-136).
- *Governs:* mandate §5.3 to §5.9, §3.1; trading §7.4; FR-8.1, FR-8.2.

**D3 Position detail** ⚠
- *Shows:* quantity from the ledger and the broker's last observation, cost basis, risk mark and its
  age, resting protection (bracket or OCO legs, or the crypto stop-limit), any unprotected interval
  running, the fractional part that cannot be protected, for crypto the disclosure that a stop-limit
  may not fill on a gap (trading §5.4), and working exits (deferred ones say
  "waiting for the regular session").
- *Primary action:* Close position [D9].
- *States:* `Unknown` order in the instrument: exits in it are held by the spec (trading §1
  principle 4), and the screen says so and offers the kill switch. `stale_mark`: no openings in it.
- *Governs:* trading §5.4, §5.6, §8.2; mandate §6.1.

**D4 Working universe and theses** ⚠ (research mode)
- *Shows:* instruments active, removed (exits only), and refused, each with its reason (the §8.5
  reason in words); each thesis with direction, horizon and expiry, evidence and corroboration as
  links to allowlisted sources, invalidation conditions, confidence labeled "self-reported by the
  model and uncalibrated", authorship "platform-authored", and for a revision the lineage's revision
  count and what changed. `max_instruments` usage (for example 3 of 5).
- *States:* Pinned mode: the pinned list only. Cost cap reached: new theses refused today; existing
  positions managed normally (DEC-120).
- *Governs:* mandate §2.3, §8.4 to §8.6; DEC-111, DEC-118, DEC-126; MI-18.

**D5 Approvals inbox**: open requests by deadline, then resolved ones with their outcome (approved
and submitted, approved and skipped by the gate, skipped at the deadline, canceled by a version or
mode change).

**D6 Approval request** ⚠ (record screen; mobile first)
- *Shows:* the proposed action (instrument, side, quantity, limit price, order value), its purpose,
  the mandate version, the rule that asked; the combined score labeled "combined model score, not a
  probability of profit"; the deadline as an absolute time and time remaining; "If you do nothing, this
  action is skipped"; model outputs behind "View model output", each labeled by author ("Output of
  software you selected" or platform-authored). **Admission variant:** the full thesis as in D4. Two
  approvers: who has approved so far.
- *Primary action:* Approve (step-up for live) or Skip, with equal weight.
- *States:* Expired: skipped, with the time; no buttons. Canceled: by a new version or a mode change,
  with the reason. Degraded: cannot reach the workspace; nothing cached. After approval: the gate
  re-ran; submitted, or skipped with the reason (binding: quantity, limit price, version).
- *Never:* a scorecard (DEC-126), a profit estimate, a price target, platform-authored alternatives,
  persuasive language.
- *Governs:* mandate §6.4; FR-6.2, FR-6.5, FR-6.6, FR-6.7; HLD flow C; PX-6, PX-7.

- *Delegation variant* (DEC-181; mandate §6.4, §6.5; laid out as PX-15 (b)): beside Approve and Skip, "Approve just
  this" first and the three fixed shapes (like this until the close, this instrument for a set time,
  this kind of order for a set time), none selected, each with the same weight as Skip. Choosing a
  shape shows every field it will write to `autonomy.delegations` (caps in dollars and orders,
  start, expiry) and the unasked dollars after it, then one step-up approves this action and confirms
  the new version, which applies at the next safe point. Counts only (asks approved, skipped, timed
  out; orders under each delegation), never profit or outcome (compliance question 37). Not offered
  for an admission, a two-approver ask, a live environment, or a client session (PX-15).
- *Client-requested variant* (DEC-185): "Requested by your connected agent" with the name the owner
  gave the client; never offers a delegation; answerable only here, never in the client's own app.

**D7 Acknowledgments** ⚠ (record screens, pending the spec change of §7 question 7). One pattern,
several variants; each shows what happened and what acknowledging changes. The spec requires step-up
for the drawdown acknowledgment (mandate §5.8), the daily-loss flatten acknowledgment (§5.4), and the
reconciliation resume (trading §11); **this brief adds step-up to the other variants** (external
activity, account restriction, surveillance), because each acknowledgment restores risk-taking:
- *Drawdown `exits_only` or flatten:* acknowledging resets the high-water mark to current equity and
  sizes return in steps (DEC-57). Disabled with the reason `flatten_in_progress` until the agent is
  flat (acknowledging re-enables risk-taking, so holding it is safe). Independent approval where
  policy requires it (mandate §5.8).
- *Daily-loss flatten:* acknowledging once flat changes `paused` to `exits_only`; the latch still
  lifts only at a new risk day plus the minimum time (mandate §5.4).
- *Reconciliation pause:* the difference between the ledger and the broker, the compensating events
  adopted, and what resuming does (PX-5; trading §11).
- *External activity:* the unattributed orders or fills ingested; agents on the account are
  `exits_only` until acknowledged (trading §7.1).
- *Account restriction:* the signal and the resulting account state (trading §7.3).
- *Surveillance threshold breach:* the report item; acknowledgment journaled (compliance, Market
  conduct).

**D8 Lifetime floor latched** ⚠
- *Shows:* that the agent was flattened and paused at the floor, the floor in dollars, the loss carry
  on the connection, and the only path forward: a version that loosens `max_loss_from_allocation`
  with independent approval, or, in a single-user workspace, after the waiting period. No acknowledge
  button.
- *Governs:* mandate §5.7; DEC-44, DEC-55.

**D9 Owner exit (close position)** ⚠ (record screen)
- *Shows:* the quantity (the agent's sub-ledger quantity), how it will be priced, and in the regular
  session that it is paced by participation caps. **Outside the regular session:** the displayed bid,
  bid size, and their time, frozen on screen; the floor price (default: the bid × (1 − the ladder's
  maximum offset)); and that any remainder rests at the floor and then waits for the
  session.
- *Primary action:* Close (step-up).
- *States:* Quote stale or missing: the owner cannot confirm a bid, so the screen offers "sell when the
  regular session opens" instead. Paused agent or `Unknown` order: see §7 question 2.
- *Governs:* mandate §6.1, §5.10 (`OwnerExitRequested`); trading §5.5, §5.6; DEC-66.

**D10 Kill switch confirmation** ⚠ (record screen)
- *Shows:* the scope and exactly what it does. **Agent:** cancels only this agent's orders, sells only
  its shares, leaves other agents and the owner's own holdings untouched, and ends the agent
  (`stopped`). **Connection or workspace:** the broker's cancel-all and close-position; the list of
  orders and positions affected, including any not managed by Mandate (PX-12). **Organization**
  ([HLD §8](../HLD.md#8-multi-tenancy-and-security) lists agent, connection, workspace, and
  organization): the workspace kill switch in every workspace of the organization, with the same list
  per workspace. Outside the regular session, the owner-exit confirmation of D9 for equities is
  optional: declining it, or having no fresh quote, still activates the cancel and `stopped`, and
  equity sells then wait for the session (trading §5.5; rule 13).
- *Primary action:* Activate (step-up).
- *States:* In progress: each step as journaled (mode applied, orders canceled and confirmed, sells
  submitted or deferred). Degraded (deployment unreachable): says the switch could not be delivered
  and how to reach the broker directly.
- *Governs:* trading §5.5; mandate §5.5; rule 13; PX-3, PX-4, PX-12.

**D11 Holding / goal complete** ⚠
- *Shows:* why the goal completed, the `on_complete` in effect, which limits stay armed, and the two
  exits: Release (the positions become the owner's and **unprotected**; the warning is recorded) and
  Close (as D9).
- *Governs:* mandate §3.1; `PositionReleased`.

**D12 Plan view** (DEC-184)
- *Shows:* what the agent is watching (the working universe or pinned list), what it would do next
  and under which mandate rule, whether that would run automatically, ask, or be denied (and why:
  the rule, a delegation, the admission ceiling, the client ceiling), and what would stop it (the
  limits with headroom, tripwires, the review date).
- *Reads:* the mandate and the runtime's state; writes nothing. Model text is quoted and attributed
  (rule 4).
- *Governs:* mandate §6.2; P6, P11.

**D13 Daily brief** (DEC-184; PX-16)
- *Shows:* what ran and under which source (rule, default, delegation), what was skipped and why
  (deadline, gate, ask budget), what each delegation let through and what remains, which guardrails
  fired (tripwires, suspensions, the review date), what changed (versions, theses, the universe), and
  anything waiting for the owner.
- *Delivery:* one notification a day, "Your daily brief is ready", with an opaque ID (rule 6).
- *Governs:* DEC-184, DEC-195; FR-8.3; P4, P5.

**D14 Chat thread** (DEC-184, DEC-192; PX-18)
- *Shows:* the owner's messages and the agent's replies. A reply is quoted model output and never
  renders a button. When a message asks for an action, deterministic code shows one of two cards: an
  **owner request** (the order the builder would propose, the gate's verdict, and the autonomy
  outcome, before anything is sent) or a **proposed version** (opens A6 with its classification and
  step-up; the review opens inline under the message, DEC-483). "Why" answers are assembled from the journal with links to each event, and a missing
  fact reads "not recorded" (E12-5).
- *Never:* an order placed from a message alone; a change to the envelope without A6.
- *Governs:* rules 4 and 11; DEC-141; compliance question 41.

**D15 Autonomy** (DEC-181, DEC-187 to DEC-189, DEC-194; PX-17)
- *Shows:* the dial as a read-out (Supervised, Co-pilot, Autopilot) derived from `autonomy.default`,
  the rules, and the live delegations; the unasked dollars; every delegation with its use, remaining
  caps, expiry, and state (live, suspended and why, spent, expired); tripwires and when each last
  fired; the review date; away mode; and connected clients with their scopes.
- *Primary actions:* End a delegation (one tap, no step-up: it reduces risk); Away mode (a reducing
  version, at once; at its end date the owner is asked to restore, and silence keeps it on); Change
  (opens the exact field on A3 with nothing pre-filled; widening goes through A6 with step-up); the
  "what would change" replay before confirming a widening (DEC-186, counts only, compliance question
  39); Revoke a client.
- *Governs:* mandate §6.5, §9.2; DEC-180; P2, P7, P11.

**D16 Monitor agent** ⚠ (DEC-528; E10-19, after the mandate spec change it names)
- *Shows:* what the agent watches and the conditions it alerts on, as the confirmed mandate states
  them; each alert it raised, with the facts or the quoted, attributed model output behind it; and
  the statement that this agent never opens a position.
- *Primary actions:* Stop control (G2); "Set up an agent", which opens A0 and never converts the
  monitor agent itself into one that trades (a new mandate, confirmed as usual).
- *Never:* a buy control, or an alert that reads as a suggestion in the platform's voice (P3, rule
  4). Notifications carry an opaque ID and generic text only (rule 6).
- *Governs:* rules 4, 6 and 11; DEC-528 item 4; FR-11.1.

#### Holdings and instruments (DEC-528, after the first paper trade)

**H1 Holdings** ⚠
- *Shows:* for each connection, its own account's holdings and cash, read-only, each with its age:
  quantity, the broker's mark, and whether an agent holds it.
  - A holding no agent bought is labeled the owner's (CN-7).
  - For Robinhood, only the agentic account (CN-8).
  - Paper and live are labeled (P9).
- *Primary action:* open an instrument (I2).
- *Never:* a sell or buy control. Closing an agent's position stays D9. A holding the owner holds
  outside any agent is not offered to an agent (DEC-528 item 3).
- *States:* Stale: per account, with its age. Degraded: the connection that cannot be read is named;
  the others still show.
- *Governs:* CN-7, CN-8; trading §7.1; FR-8.5; P6, P9.

**I1 Instrument search**
- *Shows:* results for a name or ticker query, with name, ticker and asset class, ordered by how
  they match the query text and then alphabetically (*proposed*, DEC-528).
- *Never:* a ranking, score, flag, "trending", "popular" or "interesting" list, or a suggestion the
  platform authors (data-plane §1.4; P3).
- *Governs:* DEC-528 item 2; FR-8.6.

**I2 Instrument page** ⚠
- *Shows:* facts only:
  - name, ticker and asset class;
  - the owner's holdings in it (H1);
  - the owner's agents that watch or hold it;
  - *proposed, left to E11-11's spec change:* whether an agent may trade it now (the eligibility
    floor's outcome and its reason, in words).
- *Primary actions:* "Ask an agent" (D14, as an owner request), "Set up an agent" (A0), and
  "Add to a watchlist" (I3).
- *Never:* "buy", a price target, a profit estimate, or a model's opinion outside quoted,
  attributed content (§5 Rule 12, rule 4).
- *Governs:* DEC-528 items 1 and 2; DEC-141; trading §3.2; FR-8.6.

**I3 Watchlists**
- *Shows:* the owner's own lists, curated by the owner (*proposed:* stored in the workspace; whether
  edits are journaled stays open in E11-11).
- *Primary actions:* create, rename, add or remove an instrument.
- *Rules:*
  - A watchlist is not an envelope field. A watchlist steers no agent's universe and is never read
    by an agent.
  - Lists are never read from or written to a broker's watchlist tools (connections §6.2 rule 2).
- *Governs:* DEC-528 item 2; FR-8.6.

#### Audit

**J1 Agent timeline**: events with filters by type, time, and outcome (FR-7.3). **J2 Causal trace**:
from any fill or order back to its causes (FR-7.2), each node linked to its journal event; LLM output
shown as quoted, attributed content. **J3 Export**: JSON and CSV for a time range (FR-7.4).
**J4 Chain verification**: result per segment, with the first break if any (FR-7.5). **J5 Surveillance
report**: daily items and acknowledgments (FR-7.7). **J6 Gate decision**: every check with its reason
code, the rule-set version, and the quotes and marks used, allows included (PRD §6.5 acceptance).
Auditors see these screens and nothing that acts.

#### Administration

**X1 Policies** ⚠: the organization and workspace policy editors. A value looser than its parent is
rejected with a message naming the parent limit (FR-1.5; the nearest ancestor per mandate §4.3); a change shows the agents it makes
nonconforming (mandate §4.3). **X2 Members and roles**, with separation of duties (FR-1.3, FR-1.6).
**X3 Notifications**: channels (every configured push channel is notified at once, DEC-438 item 6), quiet hours; the screen states that quiet hours
hold back push notifications of approval requests, which stay listed in the inbox and can still be
answered before their deadline, while risk-limit alerts ignore quiet hours (mandate §6.4, DEC-173). **X4 Deployment health** (hybrid and on-prem, where the customer is the deployment's
operator; journey J4): versions, heartbeats, reachability, and the DEC-100 operator surface, which
shows the aggregate-flow monitor's alerts (research-agent exposure per instrument over the
deployment's workspaces above the DEC-123 threshold; the monitor never halts by itself) and the active
per-thesis halts, and is where the operator issues or lifts a halt (step-up, the brief's addition; journaled in every
workspace of the deployment as a `PlatformOperatorAction`). A halt only removes permissions: exits
and protection continue (mandate §8.4, §8.5 check 9; HLD §8). In managed mode the platform is the
operator and this surface is internal. It carries only instrument identifiers and aggregate figures
of the deployment's own workspaces, never another deployment's.
**X5 Billing**: plan and agent count (FR-10.1). **X6 Disclosures** ⚠: accepted versions; leveraged-ETP
opt-in with step-up (V-005).

## 4. Shared patterns

### 4.1 Record screens

A record screen is one whose rendering the journal keeps as an artifact: confirmation (A5,
`MandateConfirmed`), go-live (B5, `AgentDeployed`), approval (D6, the approval events), owner exit
(D9) and kill switch (D10) (`OwnerExitRequested`), and release (D11, `PositionReleased`), per mandate
§5.10 and §10. **Acknowledgments (D7) are not record screens yet:** `HighWaterMarkReset` carries no
rendered content, and no event records what the owner saw when acknowledging a daily-loss flatten,
a reconciliation pause, external activity, an account restriction, or a surveillance breach. §7
question 7 asks for that event; D7 is designed as a record screen so it is ready when it exists.

- It renders deterministically from the data it records; the stored artifact and the UI build
  identify exactly what was on screen (mandate §10).
- Once shown, values that the owner confirms do not change underneath them. A quote that moves
  requires a fresh render and a fresh confirmation, never a silent update. On the kill switch (D10)
  the bid confirmation is optional and never holds the switch: without it, the cancel and `stopped`
  apply at once and equity sells wait for the session (trading §5.5).
- Collapsed content counts as not shown. Anything the rule requires the owner to see is expanded
  (PX-1).
- The action button is enabled only once the full screen has rendered.

### 4.2 Environment

Paper and live differ in more than a badge: a live screen shows the environment in its header, in
the Stop sheet, and in every record screen's title. Paper money is labeled simulated.

### 4.3 Restriction explainer

The mode banner (D2, D1) and alert texts use one table, so that every restriction says what it
blocks, how it ends, and who can end it:

| Restriction | Mode | Blocks | Ends when | Who acts |
|---|---|---|---|---|
| Drawdown rung `scale_sizes` | none (sizes scaled) | Full-size buys | Drawdown recovers past the hysteresis for `scale_lift_after_s` | Automatic |
| `drawdown_exits_only` | `exits_only` | Openings and increases | Owner acknowledgment, step-up (resets the high-water mark) | Owner |
| `drawdown_flatten` | `paused` | Every new order except protection | Owner acknowledgment once flat | Owner |
| `daily_loss` | `exits_only`, or `paused` after a flatten | As its mode | A new risk day plus `daily_breach_min_s` (and, after a flatten, acknowledgment once flat) | Automatic, then owner |
| `hard_breach` | `exits_only` | Openings and increases | A sane quote below the hard level, or the limit latches | Automatic |
| `lifetime_floor` | `paused` | Every new order except protection | Only a loosening version (independent approval; waiting period in a single-user workspace) | Owner and a second user |
| `goal_complete` | `exits_only` | Openings and increases | Release or close | Owner |
| External activity | `exits_only` (account) | Openings and increases on the account | Owner acknowledgment | Owner |
| Account `closing_only` / `blocked` | `exits_only` / `paused` | As its mode | Owner acknowledgment and an account refresh | Owner |
| Reconciliation mismatch | `paused` | Every new order except protection | Owner acknowledgment with step-up | Owner |
| Startup reconciliation | `paused` | As its mode | The reconciliation run confirms the replay | Automatic |
| 1× check failed | `paused` | As its mode | The account is at 1× | Owner (at the broker) |
| Owner pause | `paused` | As its mode | Resume (PX-4) | Operator |
| Stopped | `stopped` | Everything | Never; terminal | — |
| `stale_mark` (instrument) | none | Openings in that instrument | A sane mark arrives | Automatic |
| `removed_instrument` | none | Openings in that instrument | A fresh thesis is admitted, or a version re-adds it | Research agent or owner |

A rung whose breach is still confirming is shown as `pending` with its time in breach
(mandate §5.6), not as a restriction.

## 5. UX rules derived from the safety rules

| Source | Interface rule |
|---|---|
| Rule 1; mandate §4.3 | Policy ceilings are hints ("limit: at most $5,000"), never pre-filled values. A limit field starts empty unless the owner stated it or the platform proposed it |
| Rule 2; DEC-05 | Pause, close, and the kill switch never route through an approval. Every risk-increasing edit routes through classification and step-up (A6) |
| Rule 3; DEC-06; FR-6.6 | Every request shows its default. If the client cannot confirm that a response reached the server before the deadline, it shows "not recorded; the default applied", never "approved" |
| Rule 4; DEC-04; mandate §8.1 | Model and thesis text is shown as quoted, attributed content behind "View model output" (D6) or in the thesis view (D4); it never becomes a button label, a headline, or an imperative in the platform's voice |
| Rule 5 | No optimistic interface for anything that creates an order: "submitted" appears only after the server journals the intent. `Unknown` orders are shown as unknown |
| Rule 6; DEC-11; FR-6.4 | Notifications (push, email subject and body, SMS, chat) carry an opaque ID and generic text only; no agent name (PX-6). URLs use opaque IDs; page titles on agent, approval, and position screens are generic; no third-party analytics or session replay on workspace screens; no service-worker or local-storage cache of approval, position, or mandate content |
| Rule 7; FR-2.4 | Connections use OAuth where the broker offers it. Any key field is write-only and never shown again; a rejected key is explained without echoing it |
| Rule 8; DEC-98 | The environment badge is everywhere money appears. Live exists in the designs only as a blocked state (B5) until counsel signs off |
| Rule 9; DEC-79 | Compliance text (disclosures, the hypothetical-performance legend, retail `auto` wording, research-agent explanations) appears in designs as named placeholders, never drafted wording |
| Rule 10; trading §9 | A gate denial is shown as a rule in plain language ("opening orders only in the regular session"), never as an error to retry. Deferred exits say "waiting for the regular session" |
| Rule 11; mandate §2.1, §7 | Provenance badges on every envelope field; proposed values inactive until confirmed; no bulk accept of proposals (PX-2); `auto` only by the owner's explicit entry; W-006 shown when admissions are `auto` |
| Rule 12 | No order ticket and no direct broker action anywhere. The only owner orders are exits (D9, D10), which go through the ledger. No short-sale or market-order option for openings |
| Rule 13; trading §1 principle 4 | Exits are shown as "paced" or "deferred", never "denied". The agent kill switch says it touches only this agent's orders and shares. The Stop control does not depend on model state or on the dashboard having loaded |
| Mandate §4.2 | Worst-case losses in dollars on A5, with the gap-risk statement; `scale_action` in words |
| Mandate §3.1 | `profit_stop` shown as a stopping level, never as progress toward a goal |
| Mandate §5.6 | Confirming breaches shown as `pending` with time in breach |
| Mandate §5.7, §5.8 | The lifetime floor has no acknowledge action; the drawdown acknowledgment is disabled with `flatten_in_progress` until flat |
| Mandate §6.4; DEC-126 | Approval content exactly as listed (D6); combined score labeled; no scorecard, profit estimate, or price target on approval screens |
| Mandate §6.4 | Step-up for live approvals within the 5 minutes before the response; quiet-hours behavior stated on X3 |
| Mandate §6.1; DEC-66 | Outside the regular session, an owner exit confirms a frozen bid, bid size, and floor (D9) |
| Mandate §2.2 | A risk-increasing version shows "applies at the next safe point"; pending approvals are canceled when a version applies, and the inbox says why |
| Mandate §10 | Record screens as in §4.1 |
| Trading §5.5 | Kill-switch scopes named and their effects listed (D10) |
| Trading §7.1, §7.3, §11 | External activity, account restrictions, and reconciliation pauses resolve only through an acknowledgment screen (D7), with step-up for reconciliation |
| Trading §7.2 | The 1× check result is shown at connect and whenever it fails |
| FR-1.5; mandate §4.3 | A policy violation names the nearest ancestor limit it breaks |
| FR-8.4; DEC-52 | The model picker shows methodology only: no rank, "recommended", or scorecard; parameters start empty |
| DEC-99 | Backtest reports for research agents state that they test mechanics, not thesis quality |
| DEC-111; MI-18 | A revision shows the lineage's revision count and never a predecessor's score |
| DEC-124 | Robinhood's paper stage is labeled as the simulated broker |
| HLD, control-plane outage | Workspace screens keep working; only relay push is lost, and the status strip says so |
| DEC-171 | Every screen, notification, email, listing, and consent screen names the product **Owlhead** and links owlhead.ai; "Mandate", the internal name, never appears in interface copy. The lowercase noun "mandate" for the agent's contract is a separate term |
| DEC-181; mandate §6.4 | Delegation shapes: "Approve just this" first, none pre-selected, equal weight with Skip, every field shown before confirming, counts only |
| DEC-185 | A client-requested order always asks, names the client, offers no delegation, and is answered only in Owlhead |
| DEC-189 | The unasked-dollars figure appears wherever autonomy is set or shown: A2, D6's delegation variant, D13, D15 |
| DEC-195 | Asks past the daily budget never notify; the brief lists them once |
| DEC-197 | A guardrail that fires narrows only its scope and says what lifts it; ending a delegation or turning on away mode needs no step-up |
| DEC-528 | Rule 12 stands: no screen offers "buy". The flow from Home (signed-in, D1) is search, then the instrument page, then "Ask an agent" or "Set up an agent". Search results are ordered by match to the query text, then alphabetically, never by a market measure. A watchlist is the owner's, steers no agent's universe, and is never read by an agent. A holding no agent bought is shown as the owner's and is never offered for adoption. A monitor agent's alerts follow rule 6 |

## 6. Open product decisions for the founder

Decided by the founder: PX-1 to PX-14 on 2026-09-27 (DEC-135) and PX-15 to PX-18 on 2026-09-30 (DEC-198), every recommendation accepted as written.
PX-13 (a) amends DEC-131 (DEC-136).

Each question has options, a recommendation, and the reason. Before the founder answered, designs
followed the recommendation, because each is the more conservative option (rule 9), except PX-13, where
they followed DEC-131's rule (b); they now follow every recommendation, PX-13 (a) included (DEC-136).
Answers are recorded as decisions: a row in the decision log up to DEC-302, a file under
`docs/project/decisions/` after that (DEC-344). The labels PX-1 to PX-18 are local to this brief.

**PX-1. How much of the compiled mandate the confirmation screen shows by default.**
(a) Every field expanded. (b) The plain-language summary and dollar figures on top; below, every
section with every value visible in compact form, and every proposed value, warning, and `auto`
expanded with its explanation. (c) The summary only, with "view details".
*Recommendation: (b).* A confirmation carries the decision only if the owner saw the values
(compliance questions 23 and 24), and collapsed content counts as not shown (§4.1), which rules out
(c). Fully expanded (a) buries the few values that need attention among fifty that do not.

*Decision: (b), accepted.*

**PX-2. Can the owner accept all platform proposals at once?**
(a) One "accept all proposed" button. (b) Per-section confirmation, where each proposed value in the
section must be ticked or edited. (c) Per-field confirmation for everything.
*Recommendation: (b).* MI-12 requires every field confirmed, and a proposal accepted in bulk is the
platform choosing limits in all but name (compliance question 23). Per-field for all fields (c) is
tedious for values the owner stated in their own words.

*Decision: (b), accepted.* For setting up a new agent, superseded by
[DEC-477](../project/decisions/DEC-477.md) (founder, 2026-10-06): one confirmation of a summary that
shows every value, each drafted one marked.

**PX-3. What the one Stop control offers.**
(a) Only the kill switch. (b) Pause first (instant, no flatten, reversible), then the owner Stop
(terminal, no flatten, positions stay, DEC-131), then the kill switch (terminal, flatten). (c) Separate
Pause, Stop, and Kill buttons in the shell.
*Recommendation: (b).* Most "stop" moments want the agent to stop acting, not to sell at any price;
pause preserves positions and protection and can be undone. Stop is for ending the agent without
selling, and says it leaves positions in place (PX-13). One entry keeps the panic path to two taps, and
a sheet makes each scope explicit.

*Decision: (b), accepted; the owner Stop in it is as amended by DEC-136 (offered only when flat).*

**PX-4. Step-up on pause, resume, and stop.** The spec requires step-up for an owner exit (so for the
kill switch) and for resuming after a reconciliation pause; it says nothing for an owner pause, its
resume, or an owner Stop.
(a) None needs step-up. (b) Pause needs none; resume and Stop need step-up. (c) All three need it.
*Recommendation: (b).* Pause only removes permissions and can be undone, so friction there is pure
cost; resume restores risk-taking, which is the case step-up exists for (FR-1.4); Stop cannot be
undone and leaves positions without further management (DEC-131), so it gets the same check as the
kill switch. Needs a line in the runtime brief or the mandate spec.

*Decision: (b), accepted; the owner Stop in it is as amended by DEC-136 (offered only when flat).*
*Spec line:* mandate spec §6.1, "Owner controls and step-up" (DEC-173). The kill switch is as
DEC-158 option (c) leaves it: without valid step-up it still stops the agent and flattens as an
automated flatten does, and only its privileges beyond the stop (selling equities outside the regular
session at a confirmed bid) need step-up. The web kill switch inherits the same rule.

**PX-5. What the owner sees while reconciliation holds an agent.**
(a) "Paused: reconciliation mismatch" and a resume button. (b) The difference (ledger against broker,
per instrument and order), the compensating events adopted, what still runs (resting protection) and
what does not (new orders, the agent's own exits), the kill switch, and the resume action enabled
only after the difference has been shown, with step-up, recording the difference shown (which needs
the acknowledgment event of §7 question 7). (c) Automatic
resume once the difference is adopted.
*Recommendation: (b).* (c) contradicts trading §11. With (a), the owner acknowledges what they have
not seen, which makes the step-up theater; the acknowledgment should carry the evidence the way a
mandate confirmation does.

*Decision: (b), accepted.*

**PX-6. What a notification may name.** The HLD and persona examples put the agent's name in the
push ("Agent btc-accumulator needs approval"), but agent names are chosen by users and often contain
a ticker.
(a) Allow the agent name. (b) Generic text only: "An agent in your workspace needs your approval".
(c) A platform-assigned opaque agent label (for example "Agent 7").
*Recommendation: (b), with (c) as an option the owner can turn on.* Rule 6 says generic text; the
name leaks the instrument to the relay, the push provider, and the lock screen. Updating the HLD and
persona examples follows.

*Decision: (b), with (c) as an option the owner can turn on, accepted.*

**PX-7. How the approval deadline and step-up work on a phone.**
(a) The deadline goes in the notification. (b) The notification is generic; the detail screen shows
the absolute deadline and the time remaining, one reminder goes to every configured channel as the
deadline nears (DEC-438 item 6), and step-up is asked at the moment of approving, per approval, for live.
(c) As (b), but one step-up covers every approval in its 5-minute window.
*Recommendation: (b).* A deadline in the payload is not generic text. Asking at the moment of approval
binds each passkey gesture to one decision, and a passkey is one gesture, so the cost is small;
(c) is allowed by the spec but lets one gesture approve several orders. The deadline is enforced by
the server: an approval arriving after it fails closed and shows "skipped". Skip needs no step-up.

*Decision: (b), accepted.*

**PX-8. Paper to live, given that `environment` never changes (V-031).** Going live cannot be a new
version of the paper mandate.
(a) "Promote to live" creates a new live mandate prefilled from the paper one; the live confirmation
shows every field, highlights the two that differ (environment and connection), and needs step-up;
`environment` and `connection_id` are always `user_entered` on the live mandate, and every prefilled
field is confirmed again with its provenance recorded (V-020, V-038; mandate §7, "live is always
user-entered");
E10-4's paper-run requirement is defined as a run of an identical envelope. (b) The owner authors the
live mandate from scratch. (c) Change V-031 so a version may switch paper to live.
*Recommendation: (a).* (b) invites typing errors between paper and live; (c) weakens an approved spec
rule for convenience. This needs a spec and PRD clarification of "a paper run of this mandate"
(§7 question 3), and it matters only once counsel allows live.

*Decision: (a), accepted.*

**PX-9. What the product says about the research agent before users may have it.**
(a) Hide it; users see bring-your-own-strategy only. (b) Show it as "not available yet". (c) Show it
with an explanation of what it will do.
*Recommendation: (a).* Anything that describes a future capability to bring ideas invites expectations
of performance before the DEC-99 evaluation exists (compliance question 35). Designs still cover the
research mode fully so it can switch on by policy.

*Decision: (a), accepted.*

**PX-10. How approve and skip are weighted on the approval screen.**
(a) Approve is the primary button. (b) Approve and Skip have equal weight and neither is preselected;
the deadline is shown as a time, without animation or alarm colors. (c) Skip is primary, since it is
the default.
*Recommendation: (b).* (a) is a nudge toward adding risk, and compliance question 26 asks whether
engagement design amounts to a recommendation; (c) is a nudge the other way that makes approvers
distrust the tool. Neutral design is the defensible position.

*Decision: (b), accepted.*

**PX-11. Which roles may stop an agent.**
(a) Operators and admins only. (b) Operators and admins may pause, stop, and flatten; approvers may
pause. (c) Anyone with access, viewers included.
*Recommendation: (b).* An approver who sees something wrong at 3 a.m. should be able to stop new
orders; pausing never adds risk and can be undone. Stop ends the agent for good and flattening
realizes losses, so both are the operator's call. Viewers and auditors act on nothing.

*Decision: (b), accepted.*

**PX-12. The account-wide kill switch and the owner's own holdings.** At connection or workspace scope,
the spec uses the broker's cancel-all and close-position, which can close positions no agent manages.
(a) Offer only the account-wide switch. (b) Offer two: "Stop all agents on this account" (each agent's
own kill switch; the owner's holdings untouched) as the default, and "Close everything on this account"
(cancel-all and close-position) as a second, listed choice with its effects shown. (c) Offer only the
per-agent form.
*Recommendation: (b).* The common emergency is "stop the agents", and closing the owner's long-term
holdings by surprise is a harm of its own. The account-wide switch stays for a compromised or runaway
account, where closing everything is the point.

*Decision: (b), accepted.*

**PX-13. Owner stop while holding positions.** **Before DEC-136 the rule was (b), under DEC-131:**
an owner Stop sets `stopped`, cancels approvals, and hands no flatten; "stopping an agent leaves its
positions where they are, and flattening them is what a kill switch is for"
([runtime brief](../project/tasks/E6-1-agent-runtime-and-kill-switches.md), agent modes). The
designs followed (b) until the founder decided (a).
(a) Stop is offered only when flat; otherwise the choice is the kill switch (close and stop) or
"stop and release positions to me" (as `release`: unprotected, with the warning, step-up). (b) Stop
leaves positions where they are, under whatever protection is resting, with no further management
(DEC-131, the rule before DEC-136). (c) Stop always flattens.
*Proposed change to DEC-131: (a).* Every ending then has a defined owner of the positions; under (b)
nobody re-places protection when the GTC orders expire, and the open questions of §7 question 1
remain. Choosing (a) changes an accepted decision, so under rule 9 it needs a new decision,
which the founder makes. What it costs: the runtime's Stop gains a flat-or-release precondition and
one more owner choice in G2. Before the decision, D2 offered Stop with the plain statement that it
left positions and resting protection in place, and G2 offered the kill switch for ending with a
flatten; G2 and D2 now follow (a).

*Decision: (a), accepted (DEC-136, amending DEC-131).*

**PX-14. When to ratify the web stack.** DEC-134 starts design, not code.
[HLD §11](../HLD.md#11-technology) names Next.js for the web app and React Native for mobile
approvals, but no decision-log entry or ADR-0001 row ratifies it, so it is a sketch, not a decision.
(a) Ratify or revisit it at M9 (ADR-0001 ES-01). (b) Ratify it now so prototypes can become code.
(c) Ratify or revisit it at the start of M8, from the designs and one constraint recorded now: record
screens render deterministically into journaled artifacts (§4.1), which favors server-side rendering
for those screens. The HLD's Next.js choice is consistent with that constraint.
*Recommendation: (c).* Ratifying now gains little, since no web code is planned before M9, and the
designs will say more about what the stack needs; waiting until M9 itself would put the choice on the
critical path.

*Decision: (c), accepted.*

**PX-15 to PX-18** come from [ADR-0003](../adr/0003-earned-autonomy.md). The founder accepted each
recommendation on 2026-09-30 (DEC-198).

**PX-15. How delegation shapes appear on the approval screen.**
(a) All four choices as equal buttons in one row. (b) Approve just this and Skip as the two equal
buttons, then one equal-weight row, "Approve and stop asking about this for a while", that opens the
three shapes with every field shown. (c) No shapes on the card; delegations only from D15.
*Recommendation: (b).* It keeps the one-minute decision of P10 for the common case, gives the offer
the same weight as Skip without making it the first thing seen, and shows every field before the
step-up. (a) crowds a phone screen and makes a standing yes as easy to hit as a single one; (c) loses
the moment the owner has the most context.

*Decision: (b), accepted (DEC-198).*

**PX-16. What may interrupt the owner, and what waits for the brief.**
(a) Everything notifies. (b) Only asks with a deadline, risk-limit alerts, restrictions and
reconciliation holds that need an acknowledgment, a fired tripwire, and a kill switch interrupt;
everything else (delegation use, a delegation ending, admissions, fills, skipped asks) waits for D13.
(c) Only risk-limit alerts interrupt; asks wait for the brief.
*Recommendation: (b).* An ask that waits for the brief times out and is skipped, so (c) quietly turns
every ask into a skip; (a) is the fatigue DEC-195 guards against.

*Decision: (b), accepted (DEC-198).*

**PX-17. Is the autonomy dial a control or a read-out?**
(a) A slider that sets the mode. (b) A read-out of what the owner's own choices add up to, with
"Change" opening the exact field, empty, and widening through A6 with step-up. (c) No dial.
*Recommendation: (b).* A slider would write `auto` from one gesture, which V-022 forbids the platform
to pre-fill and rule 11 requires the owner to enter; (c) hides how much runs without the owner.

*Decision: (b), accepted (DEC-198).*

**PX-18. What a chat message may turn into.**
(a) Anything, including an order placed from the message. (b) An owner request card (builder, gate,
and autonomy rules, shown before sending) or a proposed version (A6 with step-up); replies never
render buttons. (c) Chat is read-only.
*Recommendation: (b).* It keeps rule 4 (the model gives opinions, deterministic code builds orders)
and rule 11 (the envelope changes only by a confirmed version) while making the agent easy to talk
to; (c) forces the owner back into forms for everything.

*Decision: (b), accepted (DEC-198).*

## 7. Questions for the specs

Designing the screens found places where the specs are silent, disagree, or need a change. They are for the spec
owners, with a recommended reading; none is decided here.

1. **A stopped agent that still holds positions.** DEC-136 (PX-13 (a)) removes the case once the
   runtime's Stop gains its flat-or-release precondition; until that change, DEC-131's runtime rule
   leaves positions where they are (PX-13 (b)), and for that interval two parts stay open: what
   happens to the resting protection once it reaches GTC expiry, since a stopped agent re-places
   nothing (trading §5.4); and
   whether a stopped agent with positions retires (mandate §2, `AgentStopped`), and if so what net
   dollar loss joins the connection's loss carry while positions are still held (mandate §5.7).
   A third follows from them: whether the owner can still close those positions through Mandate (D9 or
   the agent kill switch), since a stopped runtime proposes nothing, ever. The one path the specs do
   give is selling at the broker, which is external activity (trading §7.1) and switches every agent on
   the account to `exits_only` until acknowledged.
   Recommended: answer both in the runtime brief for the interval; DEC-136 removes the case after it.
2. **Owner exit while paused, or with an `Unknown` order.** Trading §5.5 exempts kill-switch orders
   from the agent's mode, while MI-1 lets `paused` hold exits. Is closing one position (`owner_exit`,
   not a kill switch) held by `paused`? And during a reconciliation mismatch, which quantity does a
   kill switch sell when the ledger and the broker disagree? Recommended: the kill switch sells the
   lesser of the two and alerts; a single close is allowed when the mode is an owner pause and held
   during a reconciliation pause, with the kill switch offered.
3. **"A paper run of this mandate" (E10-4, FR-4.4) under V-031.** The live mandate has a different
   environment and connection, so a different hash. Recommended: define the requirement as a paper
   run of a mandate whose envelope is identical except for those two fields (PX-8).
4. **The owner pause as a restriction.** Should §5.9 list an owner pause, and does resuming from it
   need step-up? (Runtime DEC-131 raises the first part in its Decisions needed 4; PX-4 bears on the
   second, which mandate §6.1 now answers: resume needs step-up, DEC-173.)
5. **Account-wide close-position scope.** Trading §5.5 says "close-position endpoint per instrument"
   without saying which instruments. Recommended: the confirmation lists every position it will close,
   including those no agent manages (PX-12).
6. **Notification examples.** HLD flow C and persona journey J2 show agent names in notification text;
   rule 6 says generic text. Recommended: update the examples once PX-6 is decided.
7. **An acknowledgment record.** Mandate §10 keeps the rendered screen for confirmations, go-live,
   approvals, owner exits, and releases, but `HighWaterMarkReset` (§5.10) carries only the old and new
   high-water mark, the user, and step-up evidence, and the daily-loss flatten (§5.4), reconciliation
   (trading §11), external-activity (§7.1), account-restriction (§7.3), and surveillance
   acknowledgments define no event with the content shown. Recommended: one owner-acknowledgment
   event (or a field on `OwnerAcknowledged` and `HighWaterMarkReset`) carrying the rendered screen as
   an artifact and the UI build, like `MandateConfirmed`. PX-5 (b) depends on it.

## 8. What the designs must cover first

In priority order, so W2 and the later web work start from the same list. The order puts first the
screens where a wrong design adds risk and those on journeys J1 and J2.

1. **G1 app shell and G2 Stop sheet**: every other screen sits inside them, and the kill switch must
   be reachable from all of them.
2. **D6 approval request, mobile first**, including the admission variant, expired, canceled, step-up,
   and the result after the gate re-runs.
3. **A5 confirmation** with provenance, proposed values, dollar figures, warnings, and step-up; with
   **A2 compiled review**, which feeds it.
4. **D2 agent detail** with the restriction explainer (§4.3), and **D7 acknowledgments** (drawdown and
   reconciliation first).
5. **D9 owner exit and D10 kill switch**, including outside-session bid and floor confirmation and
   every scope.
6. **D1 dashboard** and **G5 alerts**.
7. **O3 connect account** and **O2 profile**.
8. **B2 backtest report, B4 paper readiness, B5 go-live** (blocked state).
9. **D4 working universe and theses** (research mode) and **D8 lifetime floor**.
10. **J2 causal trace** and **J6 gate decision**, then J1, J3 to J5.
11. **A6 versions and diff**, **D11 holding**, **A3 editor**.
12. **Administration** X1 to X6.
13. **A0 goal questions and A2's contract card** (E10-7), then **D6's delegation and client
    variants** and **D15 autonomy**, then **D12 plan view**, **D13 daily brief**,
    and **D14 chat** (ADR-0003).

## 9. Convergence with the designs

The W2 design stream produces screens in a design canvas at the same time as this brief. The screen
IDs above are the shared vocabulary: each design frame names the screen ID it implements. When a
design and this brief differ, the difference is reconciled here, in the brief's next revision, and
listed below. A difference on a ⚠ screen is resolved in favor of the spec clause the screen cites.

| Design link | Screens | Differences found | Resolution |
|---|---|---|---|
| (none received yet) | | | |
