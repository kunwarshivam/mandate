# ADR-0003: Earned autonomy, the goal-first desk, and the money layer for general agents

| | |
|---|---|
| **Status** | Accepted: the founder accepted [DEC-180](../project/04-decision-log.md#decisions), [DEC-181](../project/04-decision-log.md#decisions), [DEC-183](../project/04-decision-log.md#decisions), [DEC-191](../project/04-decision-log.md#decisions), [DEC-193](../project/04-decision-log.md#decisions), and PX-15 to PX-18 ([DEC-198](../project/04-decision-log.md#decisions)) on 2026-09-30, each as recommended; the other decisions were accepted by an agent |
| **Date** | 2026-09-30 |
| **Deciders** | Founder (DEC-180, DEC-181, DEC-183, DEC-191, DEC-193, DEC-198); drafted by a Cursor agent at the founder's request of 2026-09-30 to research Muse, Grok Bot, and OpenAI Dots and recommend a direction |

## Context

[ADR-0002](0002-autonomous-ideation-and-retail.md) made the platform bring the ideas while the
owner sets the envelope. Two things still stand between an owner and an autonomous agent:

- **The front door is a form.** Onboarding asks the owner to write a mandate: capital, goal, risk
  limits, autonomy rules, protection, and cadence, field by field. The compiler may propose values
  (mandate spec §7), but the product still reads as "write the contract, then the bot runs".
- **Trust cannot grow.** An owner's choices are `ask` for everything, or `auto` typed into the rules
  by hand. Nothing lets an owner say "yes, and stop asking me about this kind of order for a
  while", so an agent either interrupts constantly or is trusted wholesale.

In September 2026 three general-purpose agents shipped with the pattern the second point lacks, and
the brokers opened their accounts to agents without supervising them:

1. **Meta Muse** ([launch, 8 September](https://about.fb.com/news/2026/09/introducing-muse-personal-ai-agent/);
   [safety design](https://research.meta.ai/blog/security-and-safety-for-ai-agents-our-approach-with-muse)).
   The owner gives it a goal and it keeps working in the background. A separate **Sentinel** is
   "the sole permission authority": Muse proposes, only Sentinel grants. Approvals are "strict
   capabilities, not conversational suggestions", scoped as one-time, session, task, time-bounded,
   or perpetual, and later actions must match the granted scope exactly
   ([help centre](https://www.meta.com/help/artificial-intelligence/1385290430137537/): "Allow once",
   "Allow for this task", "Always allow", "Deny").
2. **SpaceXAI Grok Bot** ([beta since 11 August](https://x.ai/news/introducing-grok-bot)). A team
   of always-on bots under a chief-of-staff bot that pull the owner in "only for judgment calls".
   An **Auto Review** step allows, asks about, or blocks each action; "Require Approval" wins over
   "Always Allow", and its own guides call Auto Review model-based, a complement to least privilege
   rather than a replacement ([summary](https://www.dplooy.com/blog/grok-bot-and-grok-46-cloud-agents-api-and-pricing)).
3. **OpenAI Dots** ([DevDay, 29 September](https://thenextweb.com/news/openai-dots-always-on-ai-agents-cloud-computers-devday)).
   Always-on agents on GPT-6 Astra that "work towards their goals around the clock", reach more than
   4,000 apps through plugins, and act under built-in rules plus owner-set **Custom Rules** that
   allow, require approval for, or block an action. One Dot per user today; more are planned
   ([The Verge](https://www.theverge.com/ai-artificial-intelligence/1002033/openai-dots-launch-muse-competitor)).
4. **Brokers ring-fence money but do not supervise.** Robinhood's agentic account
   ([product page](https://robinhood.com/us/en/agentic-trading)) is funded separately and reached
   over MCP; Robinhood "does not control, supervise, monitor, recommend, or audit these AI agents",
   and "you are ultimately responsible for the trades your AI agent places"
   ([support](https://robinhood.com/us/en/support/articles/trading-with-your-agent/)). eToro's
   [Agent Portfolios](https://www.etoro.com/news-and-analysis/etoro-updates/agent-portfolios-let-your-ai-agent-trade-for-you/)
   give each agent a sub-portfolio with a budget and a scoped key. Public's
   [Agents](https://www.prnewswire.com/news-releases/public-becomes-the-first-brokerage-to-introduce-ai-agents-for-your-portfolio-302729050.html)
   run plans the owner writes, inside Public.
5. **Unsupervised model trading loses money.** In Alpha Arena season 1 (18 October to 3 November
   2025) six models each traded 10,000 USD of real money in leveraged crypto perpetuals with no human
   and no gate; four ended 42 to 59 percent down
   ([final standings](https://www.traderank.ai/alpha-arena-leaderboard);
   [nof1](https://nof1.ai/blog/TechPost1)). The format differs from ours (leverage, perpetuals, no
   envelope), which is the point: sizing and limits were left to the model.

The general agents ask before every money action or hand it to a broker that does not supervise.
Mandate already holds what they lack for money: the risk gate is a Sentinel for trading, the
journal is the audit trail, the AUTO/ASK/DENY rules are the permission policy, and the research
agent brings the ideas. What it lacks is a goal-first front door and a bounded way for trust to
grow.

## Decision

1. **Goal-first drafting** ([DEC-182](../project/04-decision-log.md#decisions); within today's rules).
   Onboarding starts with three questions: how much money, what the goal is, and how much the owner
   can stand to lose. The compiler drafts every other envelope field as `platform_proposed` (mandate
   spec §7) and shows one plain-language contract card; the owner confirms each proposed value, as
   V-020 and MI-12 already require. Nothing proposed takes effect until confirmed. V-022 and V-038
   stand unchanged for drafting: no `auto` anywhere, no pinned instruments, no `live` environment,
   no connection is ever proposed. The loss answer maps only to limits (`max_loss_from_allocation`,
   `max_drawdown`, and `max_daily_loss`), never to position sizing beyond them.
2. **Delegations: bounded, expiring, owner-picked autonomy** ([DEC-181](../project/04-decision-log.md#decisions);
   a mandate spec change, accepted by the founder). A new envelope field, `autonomy.delegations`, holds
   owner-created permissions that turn an `ask` into `auto` inside the envelope. The mandate spec
   will define it; the invariants come first and bind that text:
   1. **It only lifts an ask.** A delegation changes an `ask` from step 4 of §6.2 (a named rule or
      the default) into `auto`. It never lifts a `deny`, never changes a limit, never skips the gate
      dry run or the gate at submission, and never changes step 5: the admission ceiling applies
      after it, so no delegation covers a newly admitted instrument in v1.
   2. **It is bounded.** Each delegation names the rule it lifts (or the default), a condition in the
      §6.3 language, a maximum order value, a maximum order count, a maximum total order value, and
      a start and expiry at most 30 days apart. Usage is counted from journaled decisions, and a
      delegation that is spent or expired stops matching; the decision falls back to `ask`.
   3. **It is suspended by any sign of trouble.** A delegation lifts nothing while the agent's mode is
      stricter than `normal`, while any drawdown-ladder rung is in effect, while a latched limit
      (daily loss, drawdown, or the lifetime floor, §5.8) has not lifted, or while a kill switch in
      its scope is engaged. A new mandate version with any other risk-increasing change carries no
      delegation over.
   4. **The owner picks it, never the platform.** Every delegation is `user_entered`, created by the
      owner with step-up as a confirmed mandate version (§9.2 classifies adding or widening one as
      risk-increasing, removing or narrowing as risk-reducing). The platform may *offer* scopes on an
      approval card; none is pre-selected, "Approve just this" is the first option, and every option
      carries the same visual weight as Skip (P3, PX-10). V-022 is restated so that offering a scope
      on an approval card is not proposing `auto`; the compiler and templates still never propose
      `auto`.
   5. **An owner-connected agent cannot touch it.** DEC-141 already says a client cannot change the
      envelope or approve; a delegation is both, so an MCP client can neither create, widen, nor pick
      one.
   6. **Timeouts still skip.** `on_timeout` stays `skip`; an unanswered card creates nothing.
   7. **The record shows it.** Each automatic decision under a delegation journals the delegation's
      ID, so the owner can see what ran without asking and why.
3. **The autonomy dial** ([DEC-181](../project/04-decision-log.md#decisions)). Supervised (`default:
   ask`, no delegations), Co-pilot (active delegations), and Autopilot (`default: auto` inside the
   limits, asks beyond them) are names for what the owner's own choices add up to, not settings the
   platform chooses. The dial shows where an agent stands. Moving towards more autonomy opens the
   exact field to edit, with nothing pre-filled, confirmed with step-up. Autopilot stays what V-022
   already allows: `auto` the owner typed.
4. **The desk** ([DEC-184](../project/04-decision-log.md#decisions); product surfaces, no rule
   change). A chat thread with the agent, in which a message becomes either an owner request (through
   the builder and the gate, like any owner input) or a proposed envelope change (confirmed with
   step-up); a **plan view** of what the agent is watching, what it would do next, and what would
   stop it; a **daily brief** with a high bar for interrupting; and the desk shown as roles: the
   research analyst (the research agent), the risk officer (the gate), the trader (the executor),
   the reviewer (post-trade journal review), and the chief of staff (the brief). The concept of part
   2 is named **delegation**, because "grant" already means an approver's response in the approval
   escalation work ([DEC-173](../project/04-decision-log.md#decisions)). Every one of these
   surfaces is public-facing, so its copy says Owlhead, never Mandate (DEC-171).
5. **The money layer for general agents** ([DEC-183](../project/04-decision-log.md#decisions);
   accepted by the founder). E10-6's MCP server (DEC-141, DEC-148) is packaged as a plugin for Dots, Muse,
   and Grok Bot and becomes a primary distribution path beside Mandate's own app. It is listed
   under the public name **Owlhead** and links owlhead.ai, like every other public surface
   ([DEC-171](../project/04-decision-log.md#decisions)); "Mandate" stays internal. The general agent
   stays owner input: it can read status, the plan, and the brief, ask for an order, and propose an
   envelope change; the gate decides, and the human confirms. It never holds broker credentials and
   never approves. Mandate stays the complete product and its research agent stays the default
   source of ideas (DEC-97, DEC-141); this changes where E10-6 sits and how it is presented, not what
   it may do.
6. **Track record without performance.** The approval card may show counts: asks approved,
   skipped, timed out, and orders run under each delegation. It shows no profit, loss, or outcome
   until counsel answers [question 35](../product/08-compliance-and-regulatory.md) and the new
   questions this ADR adds, because an outcome figure may count as hypothetical performance.
7. **Held back.** Autopilot by default (the platform proposing `auto` anywhere), delegations over
   admissions, perpetual delegations, the platform promoting trust on its own, live money for anyone
   (`AGENTS.md` rule 8 and DEC-102), and user-supplied research agents (behind counsel and the DEC-99
   forward-paper gate).
8. **Unchanged:** DEC-03 to DEC-07, DEC-47, DEC-97's split of envelope and strategy, every §9.2 row
   other than the new one, the gate, the ladder, the lifetime floor, breach confirmation, journal
   before acting, kill switches, the eligibility floor, conduct controls, and `on_timeout: skip`.
9. **Sequencing.** Documents first: this ADR and its decisions; the mandate spec change for
   delegations (spec text, schema, and reference model, following
   [#321](https://github.com/kunwarshivam/mandate/pull/321)'s shape, with the MC-U reference cases
   in their own tests-first change); the product-experience brief; the competitive landscape and
   counsel questions; and backlog stories E8-8, E10-7, E10-8, and E11-4 to E11-6. The code stories
   are safety-critical and follow the DEC-77 sequence, claimed only after the approval-escalation
   (M7) and autonomy (E6-2) pull requests now open have merged. A web prototype on fixture data may
   start at once under DEC-200.
10. **Guardrails that keep autonomy running** ([DEC-185](../project/04-decision-log.md#decisions)
    to [DEC-197](../project/04-decision-log.md#decisions); added on 2026-09-30 at the founder's
    request for "all kinds of protections and guardrails while making sure we can still work
    autonomously"). More autonomy needs more guardrails, but a guardrail that asks the owner about
    everything turns an autonomous agent back into a nagging one. So every guardrail here follows
    one rule (DEC-197): it is deterministic, recorded in the journal, only ever reduces risk, engages
    without anyone's approval, narrows only the scope where the trouble is (one delegation, one
    client, one agent), and lifts only through the envelope's own rules. Silence and ambiguity end
    autonomy; they never extend it. The guardrails:

    | Guardrail | What it stops | Decision |
    |---|---|---|
    | **Client ceiling:** an order a connected agent asked for is never `auto` | A prompt-injected Dots, Muse, or Grok Bot trading through a delegation or an `auto` rule | DEC-185 |
    | **What would change:** replay the journal under a proposed version before confirming it | Widening autonomy blind | DEC-186 |
    | **Tripwires:** owner-set conditions that end delegations or hold new openings | Trust outliving the conditions it was given under | DEC-187 |
    | **Review date:** unconfirmed past it, every `auto` and delegation reads as `ask` | A mandate nobody has looked at in months still acting alone | DEC-188 |
    | **Unasked dollars:** one figure for what can trade without asking right now | Autonomy the owner cannot size | DEC-189 |
    | **"Can I?" dry run** for connected agents | Connected agents flooding the owner with asks the gate would deny | DEC-190 |
    | **Hold new openings** from a connected agent (`exits_only`, never pause) | An owner away from Owlhead unable to stop new risk from where they are | DEC-191 |
    | **Explanations from the record:** "why" answered from journaled events | A model inventing its reasons after the fact | DEC-192 |
    | **A statement you can verify:** the hash chain proves every action was within the mandate | Trust resting on the platform's word | DEC-193 |
    | **Away mode:** a reducing version with an end date; restoring asks the owner | Autonomy running while the owner cannot answer | DEC-194 |
    | **Ask budget:** a daily cap on asks per agent and per client; beyond it, asks are skipped | Approval fatigue, and an agent or client wearing the owner down | DEC-195 |
    | **Delegation total:** all of a version's delegations together stay within the allocation | Twenty small delegations adding up to one large one | DEC-196 |

## Consequences

- **Easier:** an owner starts from a goal instead of a form; trust grows in small, visible, expiring
  steps instead of all at once; general agents reach trading through a gate instead of a raw broker
  key; the gate, journal, and autonomy rules already hold the weight.
- **Harder:** the approval card carries a second decision (how far the yes reaches), which must stay
  neutral and quick; delegations add a runtime counter to the autonomy path and a new suspension
  check; the spec, schema, reference model, and cases grow; E8 and `mandate-builder` gain a
  safety-critical story each.
- **Hard to reverse:** once owners rely on delegations, removing them turns quiet agents back into
  noisy ones. Distribution through other companies' agents depends on their plugin terms.
- **Risks:** a platform-drafted envelope and offered scopes look more like advice; counsel is already
  engaged (DEC-102) and gets new questions. An owner could delegate broadly to escape an exposure
  guard they wrote; a delegation lifts only the rule it names, is capped in value, count, and time,
  and the gate's limits still bind. A general agent could be prompt-injected into asking for orders;
  those orders are owner input and meet the same builder, rules, and gate, and it cannot approve
  them.
- **Monitor:** how often owners pick each scope; asks per active agent before and after; whether
  delegations are spent or expire unused; suspensions by cause; counsel's answers; Dots, Muse, and
  Grok Bot plugin terms.

## Alternatives rejected

| Alternative | Why |
|---|---|
| Autopilot by default (the platform proposes `auto`) | Breaks V-022 and rule 11's "the owner sets the envelope"; the most advice-like choice |
| Let the model decide orders end to end, as in Alpha Arena | Breaks rule 4; the evidence is that unconstrained model trading loses |
| The platform promotes trust by itself as the track record grows | Autonomy would change without an owner decision; an outcome-driven promotion is a performance claim |
| Perpetual delegations ("always allow") | Nothing forces a second look; a 30-day cap makes the owner renew with fresh evidence |
| Delegations that also lift `deny` or the admission ceiling | A `deny` is the owner's hard no (DEC-05); admissions are where prompt injection bites hardest (ADR-0002) |
| Hand general agents broker credentials or a broker MCP | Breaks rule 12 and journal before acting; the brokers do not supervise |
| Name the concept "grant", as Muse does | "Grant" already means an approver's response in the approval escalation work (DEC-173) |
| Do nothing | The general agents set the expectation that trust grows; without it Mandate reads as either nagging or reckless |
| Make it safe by asking more (every connected-agent call, every delegation use, every tripwire lift) | Approval fatigue makes the yes automatic, which is less safe than a bounded `auto`; DEC-197 narrows only where the trouble is |
| Let a connected agent pause an agent | `paused` holds the agent's own exits (`AGENTS.md` rule 13), so an injected pause during a fall adds risk; `exits_only` stops new risk and keeps exits running (DEC-191) |
| Away mode that restores itself at its end date | Restoring autonomy is risk-increasing and needs the owner's confirmation (§9.2); silence must not re-arm it |
