# Task: the first live trade (DEC-529, DEC-531): one order on the founder's own Robinhood account, on the product path

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story; this brief is a **path brief** over the stories below, and each slice it lists is one
story's tests PR or implementation PR under DEC-77. Docs only: it writes no code.

The founder decided on 2026-10-08 that paper trading stays on Alpaca paper
([DEC-502](../decisions/DEC-502.md), [DEC-509](../decisions/DEC-509.md)), and that before
**2026-11-02** they want one real-money order on their own Robinhood agentic account, through the
same production path. They also ruled that it be built **the product way, fast**: no
broker-specific rules in code, nothing built only for this order, every safety gate kept.

So this path builds only things the product keeps:

- **Broker rules are data** ([DEC-531](../decisions/DEC-531.md),
  [ADR-0004](../../adr/0004-broker-capability-profiles.md)): Robinhood's connector declares a
  capability profile; the builder, protection and reconciliation read it. Alpaca moves onto the
  same profile first.
- **Live is an environment, not a binary.** The paper path's deployment runner takes the
  environment and the broker from the confirmed mandate and its connection record.
- **The cap is the mandate's envelope; the confirmation is the approval flow.** The founder's
  mandate says `ask` for openings, and the order waits for the founder's grant in the CLI inbox,
  bound to the order's content hash (mandate spec §6.1, `mandate-approval`).

[DEC-529](../decisions/DEC-529.md) (Proposed, founder-reserved) holds only what is the founder's:
the live order itself and the few narrowings it needs. **Until the founder accepts DEC-529,
every slice except the live run is still built, because each is product work; nothing live is
run.**

This path needs the [first paper trade](first-paper-trade.md)'s runner (E1a, E1b, built
generic) and its manual run (E2).

## Story

- **Stories on the path:** E7-15 (the contract, recorded in
  [robinhood-contract.md](robinhood-contract.md)); E7-16 (the MCP client); E7-6 (the Robinhood
  connector, equities); E7-4 (protection read from the profile); E7-11 (its first slice: the
  connection record from the CLI); M7's remainder (the CLI inbox and grant commands); and new rows
  the coordinator adds with the claim: **E7-23** (capability profiles, DEC-531), **E7-24** (the
  founder's OAuth login, session-only), **E7-25** (the simulated Robinhood server, DEC-124), and
  **E7-26** (the deployment runner for any environment and broker).
- **Acceptance criteria (DEC-529 item 1):** one limit buy in the regular session on the founder's
  own Robinhood agentic account, long only, on the confirmed mandate's one instrument, within the
  mandate's `max_order_usd`, sent only after the founder's grant in the approval flow; it reaches
  Robinhood only through `ProductionCycle::run` and the Robinhood connector; after it fills, the
  strongest protection Robinhood's profile offers (one GTC stop-limit) covers the whole position;
  the journal verifies afterwards; and a restart sends no second order.
- **PRD / HLD / spec anchors:** PRD FR-2.2, FR-2.4, FR-2.6, FR-3.5, FR-4.3; HLD §6.A, §6.B;
  connections spec §2, §4, §6, §8, §9; trading-domain spec §4.2, §5.1 to §5.4, §7.1 to §7.3, §9,
  §11; mandate spec §6.1, V-001; journal spec §6.3, §6.4, §9.2.
- **Decisions that apply:** DEC-36, DEC-77, DEC-79, DEC-124, DEC-155, DEC-176, DEC-441, DEC-470,
  DEC-475, DEC-502, DEC-509, DEC-529, DEC-531.

## Invariants first

Every slice keeps all of these. Each is a test (named in the slice that adds it), and each
oracle is shown to fail on a seeded bug before it is trusted.

| # | Invariant |
|---|---|
| LT-1 | **No agent reaches Robinhood.** No test, CI job or agent run opens a connection to any Robinhood host. The default build contains no Robinhood host; only the runner's `live` feature adds the published MCP host. Every test runs against the simulated server on loopback |
| LT-2 | **No broker branch in shared code** (DEC-531). The builder, executor, gate and reconciliation never name a broker, and never read the asset class to learn a broker rule; a property test runs them against generated profiles and checks every order sent is one the profile allows |
| LT-3 | **Policy and profile intersect, never override.** An order is sent only if both the platform policy (limit openings in the regular session, no short sale) and the profile allow it; an empty intersection is a refusal before the intent, never a fallback to another order type |
| LT-4 | **One door, one grant.** An order reaches Robinhood only through `ProductionCycle::run`; with `ask` it is sent only after a grant whose content hash equals the hash of the request sent; any difference is a refusal with nothing sent |
| LT-5 | **Journal before acting.** `OrderSubmitted`, carrying the deterministic `ref_id`, commits before `place_equity_order`; `review_equity_order` runs first and any pre-trade alert refuses |
| LT-6 | **No re-send.** After a lost answer the order is `Unknown`; no second `place_equity_order` for that intent; recovery follows the profile (DEC-529 item 4); zero or several matches stay `Unknown` and block the instrument |
| LT-7 | **Only the agentic account** (CN-8). Every request names the recorded account; data about any other account is dropped in the connector before redaction, hashing or storage |
| LT-8 | **Allowlist, pinned contract, pinned profile** (CN-2, CN-9). The connector calls only the nine allowlisted tools; a contract or profile hash that differs halts openings; a fund-movement tool refuses the connection |
| LT-9 | **Credentials stay in the process** (CN-1, rule 7). The OAuth token exists only in the connector process's memory, never on disk, in a log, the journal, an artifact or an error; a canary-token test scans every output |
| LT-10 | **Protected or flat.** When the run exits, the entry is terminal and any filled quantity is covered by the profile's strongest protection, or the unprotected interval is journaled and its alert key is in the run's report |
| LT-11 | **No rule relaxed.** Every US account rule, the eligibility floor, the collar, both participation caps, 1× buying power and DEC-470 item 1's flat account apply |
| LT-12 | **Numbers never through a float** (ES-23). Every Robinhood price and quantity is parsed from its decimal string by `mandate-num` |
| LT-13 | **Replay never calls Robinhood.** Replay of the run's journal folds to the same state with the connector absent |
| LT-14 | **Alpaca unchanged.** Moving Alpaca onto its profile changes no Alpaca outcome; every existing executor, builder and paper test passes unchanged |

## Scope

- **Reference cases:** none move; every passing case stays passing.
- **Crates in scope:** `mandate-executor` (the profile type, protection and reconciliation read
  it), `mandate-builder` (quantity form from the profile), `mandate-alpaca` (declares its
  profile), `mandate-paper` (the runner, generic from the paper path's E1a; renamed after the
  live run), `mandate-cli` (connection record, inbox wiring, `live` acceptance), and three new
  crates: `mandate-mcp` (E7-16), `mandate-robinhood` (E7-6) and `mandate-rh-sim` (E7-25).
- **Out of scope:** `mandate-risk` and `mandate-runtime` (used, not changed), the vault, E9-4,
  the full connection manager, crypto, options, extended hours, `web/`.
- **New dependencies:** none external except `getrandom` 0.2 for the OAuth PKCE verifier and
  `state` (already built through `ring` under `rustls`; needs a `docs/dependencies.md` row), and
  `tokio`'s `net` feature for the loopback redirect. `mandate-mcp` and `mandate-robinhood` use what
  `mandate-alpaca` uses (`reqwest`, `serde_json` with `raw_value`, `tokio`, `secrecy`,
  `thiserror`); JSON-RPC and server-sent events are written by hand; base64url is written by hand
  against RFC 7636 appendix B; URLs use `reqwest::Url`.
- **Safety-critical:** yes, every code slice. DEC-77 tests PR then implementation PR.
- **Size budget:** under 400 non-generated changed lines in safety-critical crates per PR.

## Where each piece lives

| Piece | Crate | Layer | Why there |
|---|---|---|---|
| `CapabilityProfile` type and its canonical hash | `mandate-executor` (`types.rs`) | 6 | Beside `BrokerConnector`, which gains `fn profile()`; the builder reads it through the executor's existing ports |
| Alpaca's profile | `mandate-alpaca` | 7 | Trading spec §5.2's table as data |
| Robinhood's profile, order mapping, `ref_id`, CN-8 filter, state mapping | **new** `mandate-robinhood` | 7 | Beside `mandate-alpaca` |
| MCP client, allowlist, contract hash, OAuth (`auth` module) | **new** `mandate-mcp` | 6 | Knows nothing of brokers or orders (connections spec §6.2) |
| Simulated Robinhood: the published contract over loopback MCP, with Robinhood's rules | **new** `mandate-rh-sim` | tool | DEC-124's paper stage and E7-16's fixture server |
| Connection record, inbox and grant commands | `mandate-cli` | 7 | The founder's CLI plays the connection and deployment manager in Phase 1 |
| The runner for any environment and broker | `mandate-paper` (renamed after the live run) | 9 | One runner; the `live` feature holds the live hosts |

**Founder review.** New crates add `xtask/layers.toml`, `CODEOWNERS` and `Cargo.toml` entries;
the OAuth slice adds a `docs/dependencies.md` row; each PR names it under "Decisions needed".

## Robinhood's profile (from the contract)

| Row | Value | Source |
|---|---|---|
| Equities, regular session, order types | `market`, `limit`, `stop_market`, `stop_limit` | `place_equity_order.type` |
| Quantity forms | `market`: whole, fractional, notional; every other type: whole | `quantity`, `dollar_amount` |
| Times in force | `gfd`, `gtc` | `time_in_force` |
| Protection forms | one resting stop-limit (`gtc`); no OCO, no bracket | Tool list |
| Idempotency | client id `ref_id`, deduplicated; query by it: no | `ref_id`; `get_equity_orders` filters |
| Pre-trade check | `review_equity_order` | Tool list |

With the platform policy (limit openings only), the builder's intersection leaves `limit`, whole
shares, `gfd`, regular session; protection takes the one stop-limit. None of that is written as
Robinhood code.

## Lifecycle walk

| State | Entered | Blocks | Ends | Who | Session close, midnight, restart, version change |
|---|---|---|---|---|---|
| DEC-529 Proposed | Now | The live run only | The founder accepts | Founder | — |
| Logged out | Run start | Every Robinhood call | OAuth login in the founder's browser | Founder | Restart logs out (no token on disk) |
| Logged in | Token in memory | Nothing | Expiry, revocation, exit | Robinhood, founder | Expiry mid-run: `closing_only`; the resting stop is unaffected |
| Contract or profile drift | Hash differs at login | Every opening | A released connector version | Agents, founder | Checked at every login |
| Rehearsal | Live build against `mandate-rh-sim` | Nothing real | Its report | Founder | Separate journal, never the live one |
| Approval pending | Gate passed, `ask` | The submit | Grant (sent), deny or timeout (not sent) | Founder | Timeout is a deny (rule 3); restart re-asks nothing past the intent's age |
| Intent journaled, not sent | `IntentProposed` | A second cycle | `OrderSubmitted` or the intent's age | Executor | Restart re-hands nothing past its age |
| Submitted, answer lost | `OrderSubmitted` without an answer | Every order in the instrument | Exactly one match, else the founder reconciles | Executor, founder | Next run refused (DEC-470 item 1) |
| Working entry (`gfd`) | Accepted | Another opening | Filled, or cancelled at the run's bound | Executor | `gfd` expires at the close |
| Filled, stop not yet placed | Complete fill | Adds | Stop accepted | Executor | Unprotected interval journaled and alerted beyond `max_unprotected_s` |
| Protected | Stop accepted (`gtc`) | Adds | Stop fills, or the founder exits | Robinhood, founder | Rests with no process running; GTC expiry unknown (open question) |
| Done | Run report | Every further run (account not flat) | — | — | `journal export`, `journal verify` clean |

## Adversary review

| Actor | Attempt | Blocked or disclosed by |
|---|---|---|
| Careless founder | Sets a large `max_order_usd` | It is an envelope value, confirmed with step-up and shown in the grant |
| Careless founder | Grants without reading | The grant shows the order and is bound to its hash (LT-4) |
| Careless founder | Runs twice, or on a shared account | DEC-470 item 1 refuses a non-flat account; no re-send (LT-6) |
| Agent | Calls Robinhood in its own session | Rule 8, DEC-441 item 11, LT-1's build test; no agent has the founder's login |
| Bad model | `Long` on a thin or expensive instrument | The gate's floors, the collar, the envelope |
| Next broker | Has rules nobody coded | It declares a profile; LT-2's property test covers any profile |
| Malicious server | Adds a transfer tool, changes a schema, injects text | LT-8, CN-9 |
| Bad tick | Robinhood's quote jumps | Cross-check against Alpaca's IEX quote within the collar (DEC-529 item 12) |
| Lost answer | Place times out | `Unknown`, profile-driven recovery, never a re-send (LT-6) |
| Gap through the stop | Price gaps below the stop's limit | Disclosed: a stop-limit may not fill (DEC-529 item 7) |
| Credential leak | Token in a log, error or core dump | LT-9; the runner disables core dumps at start |

## Cross-spec trace

| Reference | Scope | Outcome | Matches? |
|---|---|---|---|
| Trading §5.2 "Alpaca capability matrix" ↔ DEC-531 | Every broker | Profiles as data | **No** until SP1 renames it and adds Robinhood's profile |
| Executor `protection.rs` (`asset_class == Crypto` picks one stop-limit) ↔ DEC-531 item 2 | Protection | Strongest form the profile offers | **No**: B2 replaces the branch |
| DEC-441 item 10 ↔ U-R1, U-R2 ↔ the contract | Idempotency | Submit with id, query by id | **No**: no query by `ref_id`; DEC-529 item 4 (founder) |
| Trading §5.4 (OCO or bracket for equities) ↔ Robinhood's profile | Protection | One GTC stop-limit | **No**: SP1, DEC-529 item 7 (founder) |
| Trading §4.2 (`sip` for live equities) | Live quote | Collar and risk mark | **No**: DEC-529 item 12, SP1 |
| Trading §7.2 (account type and regime per broker) | Account | Robinhood rows | **No**: SP1 |
| Mandate §6.1 `cli_confirm` paper only ↔ DEC-155 item 4 | Live grant | Method field | **No**: DEC-529 item 3, until E9-4 |
| ES-23 ↔ the runner's `live` feature | Build | — | **No**: DEC-529 item 3 |
| Connections §6.2 rule 5 ↔ ES-23 numbers | Prices, quantities | Decimal strings via `mandate-num` | Yes |
| CN-8 ↔ `get_accounts` | Reads | Other accounts dropped | Yes, by C2 |
| DEC-124 ↔ the rehearsal | Founder's run | Simulated stage first | Yes, R0 |

## Slices, in parallel lanes

T is a tests PR and I its implementation PR (DEC-77); D docs; SP a protected spec PR the founder
approves. **Now** means it can start today, beside the paper path. Lanes touch different crates,
so six builders can work at once; reviews run in parallel across lanes and merges one at a time.

| # | Lane | Story | Kind | Crate | What | After | Now? | Estimate |
|---|---|---|---|---|---|---|---|---|
| L0 | — | E7-15 | D | docs | This brief, DEC-529, DEC-531, ADR-0004, the contract | — | this PR | — |
| SP1 | — | E7-23 | SP | `docs/specs/` | Trading §5.2 as broker profiles with Robinhood's; §5.4, §4.2, §7.2 Robinhood rows; connections §4, §6.2, §6.6 from the contract; mandate §6.1 and V-001 per DEC-529 | — (DEC-529 parts wait for the founder) | **now** | 200–300 |
| B1 | Profile | E7-23 | T, I | `mandate-executor`, `mandate-alpaca` | `CapabilityProfile` and its hash; `BrokerConnector::profile`; Alpaca declares §5.2; registered as `broker_profile` configuration | — | **now** (coordinate with the paper path's A1 in `mandate-alpaca`) | T 250–350, I 200–300 |
| B2 | Profile | E7-23 | T, I | `mandate-executor` | Protection and reconciliation read the profile; the `asset_class == Crypto` branch goes; LT-2's property over generated profiles; LT-14 | B1 | **now** | T 250–350, I 200–300 |
| B3 | Profile | E7-23 | T, I | `mandate-builder` | Quantity form from policy ∩ profile (LT-3); deployment refuses a policy the profile cannot meet | B1 | **now** | T 200–300, I 150–250 |
| M1 | MCP | E7-16 | T, I | **new** `mandate-mcp` | Transport: pinned host, HTTPS only (loopback in test builds), no redirects, JSON-RPC, `Mcp-Session-Id`, JSON and SSE answers, timeouts, the token bucket with a reserved exit budget | — | **now** | T 300–400, I 300–400 |
| M2 | MCP | E7-16 | T, I | `mandate-mcp` | Allowlist, contract hash, drift halts, fund-movement tool refuses, metadata never surfaces | M1 | **now** | T 200–300, I 150–250 |
| O1 | MCP | E7-24 | T, I | `mandate-mcp` `auth` | OAuth 2.1 with PKCE and single-use `state` on a loopback redirect; token in `SecretString` only (LT-9); `getrandom` row | M1 | **now** | T 250–350, I 250–350 |
| S1 | Robinhood | E7-25 | T, I | **new** `mandate-rh-sim` | The contract's rules as a pure core: types, quantity forms, `gfd`/`gtc`, regular hours, `agentic_allowed`, `ref_id` dedup, the ten states, cancel refused when terminal, `review` alerts, scripted fills and faults | — | **now** | T 250–350, I 250–350 |
| S2 | Robinhood | E7-25 | T, I | `mandate-rh-sim` | Loopback MCP server over S1, injection and extra-tool variants, a lost-answer fault, a binary for the rehearsal | S1, M1 | **now** | T 200–300, I 200–300 |
| C1 | Robinhood | E7-6 | T, I | **new** `mandate-robinhood` | Its profile; `Submit` → `review_equity_order` then `place_equity_order` with the derived `ref_id`; `Cancel`; the state mapping | M2, S1, B1 | **now** | T 300–400, I 250–350 |
| C2 | Robinhood | E7-6 | T, I | `mandate-robinhood` | Reads filtered to the agentic account (CN-8). The account fingerprint (CN-5) follows after the live run: one connection needs no duplicate check | C1 | **now** | T 200–300, I 150–250 |
| K1a | Live | M7 | T, I | `mandate-cli` | The `inbox` and `grant` commands (M7's clap remainder) over the existing approval path | — | **now** | T 200–300, I 200–300 |
| K1b | Live | E7-11 | T, I | `mandate-cli` | `connection record`: a Robinhood `ConnectionEstablished` (scopes the allowlist, the account number by reference, the 1× attestation), and `live` accepted by `version confirm` and `agent deploy` for that connection only (DEC-529 item 3) | the paper path's P0 | as soon as P0 merges | T 200–300, I 150–250 |
| G1 | Live | E7-26 | T, I | the runner (`mandate-paper`) | The connector chosen from the connection record and the environment from the confirmed mandate; the `live` feature and LT-1's build test; `ask` openings wait for the grant (LT-4); the quote cross-check | E1b, K1a, K1b, C2, O1, B2, B3 | no | T 300–400, I 250–350 |
| R0 | — | — | run, then D | — | The founder's rehearsal and live run (below); a docs PR records the evidence | G1, the paper path's E2, DEC-529 accepted | no | — |

**The paper runner is built generic from the start.** The paper path's E1a and E1b build the
runner so it takes the connector from a `BrokerConnector` chosen by the connection record and
the environment from the mandate, with Alpaca paper as the only connector compiled by default.
That is the same work as an Alpaca-only runner, and it removes a whole generalization slice from
this path's tail. The coordinator applies it to the paper brief's E1a and E1b. Renaming the
crate to `mandate-run` waits until after the live run.

**Totals.** 27 PRs (one docs, one spec, 13 tests and 12 implementation PRs), about 6,000 to
8,500 changed lines. All but G1 start now or as soon as P0 merges.

**Critical path.** The paper path's E1b → G1 (one tests PR, one implementation PR) → the
rehearsal → the live run. Everything else runs beside the paper path.

## Schedule: a target of Monday 2026-10-26

The founder's deadline is 2026-11-02. This schedule targets everything done a week earlier,
by Monday 2026-10-26, as requested in this brief's session on 2026-10-08, so a slip still lands
before the deadline. Working back from that target:

| Date | What is done |
|---|---|
| Fri 2026-10-09 | The founder answers DEC-529 and approves SP1. The coordinator adds the backlog rows, applies the generic runner to the paper brief, and starts six builders: the B, M, S and C lanes, K1a, and O1 |
| Fri 2026-10-16 | Every "now" tests PR merged; B1, M1, S1, C1 and K1a implementations merged. The paper path's E1b merged (its own brief's target) |
| Tue 2026-10-20 | Every "now" implementation merged; K1b merged; the paper path's E2 run done. G1's tests PR merged |
| Wed 2026-10-21 | G1's implementation merged. The path is complete in code |
| Thu 2026-10-22 | The founder's rehearsal against `mandate-rh-sim` (R0 steps 1 and 2) |
| Fri 2026-10-23 | The live run (R0 steps 3 to 8) |
| Mon 2026-10-26 | Buffer: a second live attempt if Friday's model said `Flat` or the gate refused, and R0's evidence PR merged. **Everything done** |

**What it takes, and the decisions I made to get there:**

- **Six builders from 2026-10-09**, one per lane plus K1a and O1. Every "now" slice touches its
  own crate, so they do not collide.
- **Reviews run in parallel across lanes; merges stay one at a time.** At one review at a time,
  27 PRs plus the paper path's remaining ones cannot finish by 2026-10-21. Each review is still
  independent and on a different model, and each pair is still tests first (DEC-77).
- **A tests PR and its implementation PR can be open together**, with the implementation
  rebased after the tests merge, as long as the tests PR's pending tests are shown failing on
  its stubs first. This saves a review cycle per pair without weakening DEC-77.
- **Cut, not deferred into a hack:** the account fingerprint (CN-5, needed only once there is a
  second connection) and the crate rename. Both follow the live run as normal stories.
- **The founder's answers by 2026-10-09.** DEC-529 and SP1 gate only G1 and the live run, but a
  late answer moves the whole tail.

**Risks, in order:** the paper path slipping past 2026-10-20 (the one thing this path cannot
absorb; the 2026-10-26 buffer is one trading day); OAuth discovery at Robinhood's server needing
something the published MCP authorization flow does not describe (a stop condition, found by
O1's tests against the published flow, so it surfaces by about 2026-10-14); and a model that says
`Flat` on both 2026-10-23 and 2026-10-26, which nobody works around (DEC-475).

### The founder's run (R0)

Only the founder runs it, on their own machine, with their own login; no agent runs any step or
sees its output during the run (`AGENTS.md` rule 8).

1. Build `mandate-paper` with `--features live` from a commit on `main`, and `mandate-rh-sim`.
2. **Rehearsal (DEC-124).** Start `mandate-rh-sim`; record a connection to it, register, confirm
   and deploy a rehearsal mandate on a separate journal; run the whole flow, including the
   grant, the fill, the stop, and a restart that sends nothing; then `journal export` and
   `journal verify`: clean.
3. **Live preparation.** The agentic account is flat, holds only what the founder will risk, and
   is a cash account or has margin disabled. Download and `inspect` the instrument's bars;
   register the configuration with a dated ETP classification; `connection record` the live
   connection, typing the account number; create, confirm and deploy the mandate with `ask` for
   openings and `max_order_usd` covering one share.
4. Between 09:30 and 15:50 New York on a trading day, run without `--place-one-order`: it logs in
   and reports the order it would ask for, or the exact refusal. A `Flat` model or a gate denial
   ends here; neither is worked around.
5. Run with `--place-one-order`; the order waits in `mandate inbox`; the founder reads it and
   grants it with `cli_confirm`; one `place_equity_order`; the run stays until LT-10 holds.
6. `mandate journal export` for the agent, account and control streams, then `mandate journal
   verify <file> --store <root>` on each: clean.
7. The same command again: refused before any place call (DEC-470 item 1).
8. The founder checks the position and the stop in the Robinhood app and records the evidence
   (no account number) in R0's docs PR.

## Decisions

### Founder-reserved (DEC-79), in DEC-529

| # | What | Meanwhile |
|---|---|---|
| F-1 | The live order and its narrowings of rule 8, ES-23, DEC-155 item 4 and V-001 | Everything is built; nothing live is run |
| F-2 | The instrument and `max_order_usd` (whole shares, so one share must fit) | Agents propose neither |
| F-3 | Recovery without a query by `ref_id` (DEC-529 item 4) | B2's fallback is built behind the profile and refuses every profile without a query by client id until accepted |
| F-4 | DEC-441 items 15, 16, 17 and 20 as DEC-529 reads them; counsel | No customer connection; no live run |
| F-5 | The live quote source (DEC-529 item 12) | No live run |
| F-6 | Founder-owned files (new crates' entries, the `getrandom` row) and SP1 | Each PR names it |

### Decided by agents

- [DEC-531](../decisions/DEC-531.md) and [ADR-0004](../../adr/0004-broker-capability-profiles.md):
  broker rules as capability profiles.
- The `ref_id` derivation: a UUID version 8 from SHA-256 (`mandate-canon`) of the intent's
  idempotency key; C1 records it.
- One runner for every environment, built generic in the paper path's E1a, not a live binary; the confirmation is the
  approval flow (DEC-529 item 1).
- E9-4 stays off this path: the approval record's method field lets it replace `cli_confirm`
  without rework.

## Commands

```bash
cargo nextest run -p mandate-mcp -p mandate-robinhood -p mandate-rh-sim
cargo nextest run -p mandate-executor -p mandate-builder -p mandate-alpaca -p mandate-cli -p mandate-paper
cargo nextest run -p <crate> --run-ignored only   # every pending test fails on its stub
cargo check -p mandate-paper --features live      # compile only; never run in CI
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci mutants
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
cargo xtask check
```

## Stop conditions

Stop and write a decision rather than continuing if:

- shared code would need to name a broker, or read the asset class to learn a broker rule;
- a broker fact cannot be expressed as a profile row;
- any test, CI job or agent run would reach a Robinhood host or need a real account, a recording
  or the founder's credentials;
- the connector would need to re-send an order or adopt one it cannot attribute (CN-7);
- the OAuth flow needs a client secret, a non-loopback redirect, or a token on disk;
- protection would leave a filled position unprotected after the run;
- moving Alpaca onto its profile changes any Alpaca outcome (LT-14);
- a new dependency beyond `getrandom` seems necessary.

## Definition of done

- [ ] DEC-529 accepted; SP1 merged; DEC-531's profiles in use by Alpaca and Robinhood.
- [ ] No broker name or asset-class branch for a broker rule in the builder, executor, gate or
      reconciliation (LT-2).
- [ ] The rehearsal against `mandate-rh-sim` ran clean, restart included.
- [ ] One limit order on the founder's own Robinhood agentic account through
      `ProductionCycle::run`, inside the regular session, within the mandate's envelope, after
      the founder's grant.
- [ ] The filled position covered by one resting GTC stop-limit (LT-10).
- [ ] `mandate journal verify` clean on every exported stream; a second run sent nothing.
- [ ] No agent called Robinhood or saw the credential.
- [ ] Every code slice: tests first, zero missed mutants, green CI, an independent review on a
      different model.
