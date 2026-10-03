# Data Plane Spec (v0.2, draft)

| | |
|---|---|
| **Status** | Draft v0.2: v0.1 plus the blockers and majors of the post-merge review on [#553](https://github.com/kunwarshivam/mandate/pull/553) |
| **Owner** | Engineering |
| **Decisions** | [DEC-433](../project/decisions/DEC-433.md) (items 1 to 14 accepted by the agent; items 15 to 20 decided by the founder on 2026-10-03; items 21 and 22 Proposed for the founder) |
| **Backlog** | E2-5 to E2-19 ([backlog](../project/06-backlog-v1.md#e2-market-data)) |

**v0.2** fixes the post-merge review's blockers and majors: DP-6 restated with its three known gaps
and a wider oracle, DP-3's time clause marked pending, items 15 to 20 shown as decided, items 21 and
22 put to the founder, the drift detector's input corrected in §4.6, and the check 15 gap recorded
in §11. Minors are backlog rows E2-16 to E2-19.

This spec covers the data the runtime and the research agent read: live market data, reference
data, news, filings, and fundamentals. It says where each comes from, how fresh it must be, how it
is stored point in time, and how it reaches workspaces. It adds no risk-gate rule of its own. Where
it touches the gate, the [trading domain spec](trading-domain.md), the [mandate spec](mandate.md),
and the [journal spec](journal.md) are authoritative; this spec only says what the data plane must
deliver so that their rules hold. The few tightening readings it takes are recorded in DEC-433.

Two sibling specs, drafted in parallel, consume what this one produces: the agent harness spec
(`docs/specs/agent-harness.md`, the research agent's tools and loop) and the inference spec
(`docs/specs/inference.md`, the model gateway). This spec stops at the research query interface
(§4.6); what the agent does with the answer is theirs.

## Contents

1. Scope
2. Invariants
3. Live market data
4. News, filings, and fundamentals
5. Storage
6. Fan-out and deployment modes
7. Lifecycle and failure walk
8. Adversaries
9. What exists and what is planned
10. Decisions
11. Open questions

---

## 1. Scope

### 1.1 What is consumed

| Data | Consumers | Source (v1) | Freshness need | Tier ([HLD §9](../HLD.md#9-intelligence-layer)) |
|---|---|---|---|---|
| Quotes (top of book) | Risk marks, collar, exit pricing, approval drift ([trading spec §8.2](trading-domain.md#82-marks-and-equity), [§9.6](trading-domain.md#96-market-conduct-controls-dec-31)) | The workspace's own Alpaca connection: `sip` live, `iex` paper, `crypto` ([§4.2](trading-domain.md#42-data-profiles-dec-35)) | Fresher than the profile's staleness threshold | Hot |
| Trades | Reporting marks, last-trade fallback in session, volume caps | Same | Same | Hot |
| Trading status and LULD bands | Halt checks ([§4.4](trading-domain.md#44-halts)) | Same, where the feed carries them (§11 question 1) | Connected and resynced | Hot |
| Bars (minute and daily) | Signal models, screens, eligibility statistics, backtests | Same; history from the market-data host (built, E2-1) | Bar complete | Hot (live), batch (history) |
| Corporate actions | Accounting and executor ([§8.5](trading-domain.md#85-corporate-actions)), price adjustment ([§4.5](trading-domain.md#45-adjustments)) | Broker corporate-action records | Before 19:45 ET on the day before the ex-date | Batch |
| Calendars and sessions | Session checks, settlement, risk day | Checked-in data files with provenance (`crates/mandate-time/data/`) | Valid for the date asked; an alert well before the file's end | Static |
| Instrument snapshot | Eligibility floor ([§3.2](trading-domain.md#32-eligibility-floor-dec-31)), order constraints | Broker asset records, daily after 19:45 ET and on events ([§3.1](trading-domain.md#31-instrument-fields)) | Same trading day | Batch |
| Eligibility statistics | Price floor, 20-day median dollar volume, average daily volume | Derived from daily bars | Computed from the last completed session | Batch |
| ETP and ETN classification | Eligibility floor item 6 | Open ([trading spec §15](trading-domain.md#15-open-questions) items 5 and 8) | Younger than the configured age, or ETP openings are denied | Batch |
| SEC filings | Research agent | SEC EDGAR (public) | Minutes | Slow |
| Fundamentals | Research agent, screens | SEC XBRL financial data (public); no paid vendor before the DEC-99 evaluation passes (DEC-433 item 16) | As filed | Slow |
| News | Research agent | The workspace's own Alpaca news access in v1 (DEC-433 items 3 and 16) | Minutes | Slow |

Equities and crypto spot only. Options, futures, and perpetual futures data are out of scope for v1
(trading spec §16; perpetuals are E16).

### 1.2 The two planes

- **Workspace data service.** Runs inside the workspace deployment, beside the runtimes ([HLD
  §4](../HLD.md#workspace-deployment)). It holds the workspace's market-data connections, the hot
  cache, the point-in-time store for what the workspace captured, and the research query interface.
  Everything tenant-specific lives here: which instruments are subscribed, what was queried, and
  what any decision used.
- **Shared data plane.** Optional, managed by us, computed once and fanned out ([HLD
  §4](../HLD.md#shared-data-plane)). In v1 it carries only data that may be shared without a license
  (DEC-433 item 3): SEC filings and fundamentals, their factual classification, calendars, and
  reference lists whose terms allow it. It carries no exchange market data in v1: the founder
  decided against licensing redistribution (item 15, [HLD §12 risk 3](../HLD.md#12-risks-and-open-decisions), RAID
  R-12).

### 1.3 Deployment modes

| | Managed | Hybrid | Fully on-prem or air-gapped |
|---|---|---|---|
| Market data | Each workspace's own broker connection, from our cell | Each workspace's own connection, from the customer's site | Same, or none (no live trading without a quote) |
| Shared data plane | Ours, pulled by cells | Optional subscription, pulled outbound | Optional offline bundle, or none |
| Point-in-time store | Object storage in the cell | Customer object storage (MinIO on the edge) | Same |
| News | Workspace's own Alpaca news access | Same | Same, or none |

### 1.4 Non-goals

- **No directional views from the shared plane** ([DEC-62](../project/04-decision-log.md#decisions)).
  It publishes data and factual classifications (form type, filer, tickers named in structured
  fields). Anything directional is a signal model the owner selects and pins
  ([mandate spec §8.1](mandate.md#81-signal-model-contract-dec-52-dec-97)).
- **No platform recommendations.** The data plane never ranks, scores, or flags an instrument as
  interesting. Screens are owner-configured filters run inside the workspace.
- **No order-book depth** in v1 (E2-3 stays Should).
- **No vendor-adjusted history.** Prices are stored raw, and adjustments are applied point in time
  from stored corporate actions ([trading spec §4.5](trading-domain.md#45-adjustments)).
- **No model in the shared plane.** Tagging is deterministic (DEC-433 item 10).

---

## 2. Invariants

Every rule below must preserve these. Each has a test named in the backlog story that implements
it; each is checked against an independent oracle, never against the implementation's own
predicate (AGENTS.md, "Getting it right the first time").

| ID | Invariant | How it is tested |
|---|---|---|
| DP-1 | **Point in time.** A query as of time *t* returns only records whose knowledge time (§5.2) is at or before *t*. No backtest, screen, or research query sees a record before it was knowable | Property test over random record streams, corrections, and late arrivals: an oracle that filters by its own knowledge-time ledger returns the same set |
| DP-2 | **Nothing is revised in place.** A stored record, a dataset day, or a journaled observation is never edited or deleted within its retention. A correction is a new record that names the one it corrects, and its knowledge time is when it arrived | Store tests: a second write of different content for a stored key is refused (already true for DEC-89 datasets); a correction leaves the original readable as of any earlier time |
| DP-3 | **What drives a decision is journaled.** Every quote, mark, bar window, news item, or filing that a decision, a gate check, or a thesis used is reachable from the journal: as `MarkUpdated` or `GateDecided.quotes_used` on the account stream, or as `ObservationRecorded` with its `data_ref` digest on the agent stream ([journal spec §9.1](journal.md#91-agent-stream-payload-schemas-dec-177)). Each carries its source and feed, and risk inputs carry `risk_clock` ([mandate spec §5.2](mandate.md#52-inputs-the-risk-clock-and-determinism)). **Pending:** each should also carry its vendor time and receive time. `MarkUpdated` holds only instrument, price, source, and feed today, so this clause does not hold until the journal spec change in E2-6 lands (§11 question 2) | Replay test: rebuilding every decision from the journal and its artifacts alone gives the same outputs (MI-8); a decision whose input is missing from the journal fails the test. The vendor-time and receive-time assertions are pending on E2-6's journal spec change and are not to be dropped or weakened in the meantime |
| DP-4 | **Stale or missing data never adds risk** (rule 3). Without a fresh sane quote, no opening or increasing order is allowed in that instrument, and a stale mark never triggers a flatten ([mandate spec §5.2](mandate.md#52-inputs-the-risk-clock-and-determinism)). Exits and protection continue: risk-reducing orders accept any mark source ([trading spec §8.2](trading-domain.md#82-marks-and-equity)) | Fault injection: drop, delay, or freeze the quote stream at every point; no opening is sent on a quote older than the threshold, and every exit still runs |
| DP-5 | **Unknown status is not "trading".** An instrument whose status feed is down or not yet resynced is a presumed halt: no market orders, exits as marketable limits ([§4.4](trading-domain.md#44-halts)), and no openings (DEC-433 item 6). One exception, decided by the founder (item 18): on paper with the `iex` profile, if that feed carries no status channel, an opening is allowed on a fresh IEX quote plus an asset read no older than 60 s showing `tradable` | Fault injection on the status channel alone, with quotes still flowing; on a profile with no status channel, an opening without the fresh asset read is refused |
| DP-6 | **One bad print cannot act alone on a limit.** A single wrong quote never latches a limit, starts a trim or a flatten, or lifts a restriction ([mandate spec §5.6](mandate.md#56-breach-confirmation-dec-49-dec-54); tripwires read no marks, mandate spec §6.7). **Three known gaps, where the invariant does not hold today:** (a) one high print raises the high-water mark H for good; (b) one high print that is the day's last regular-session quote sets E₀; (c) one high print on a held instrument raises E for that evaluation, which loosens the two E-scaled caps and the order builder's cap, so a larger buy is admitted (§8). Gaps (a) and (b) are DEC-433 items 17 and 21; gap (c) is item 22 | Fuzz: inject one outlier quote anywhere in a random mark sequence and compare two things against the run without it: the set of latched limits, trims, flattens, and lifted restrictions; and **the value of every admitted opening order** (computed by the oracle from its own equity ledger, not from the gate's). The H and E₀ cases are pending on items 17 and 21, and the admitted-value case on item 22; each is written as a pending test that fails today, never left out |
| DP-7 | **Backfill never marks.** Data fetched to fill a gap feeds bars and the store only. A risk mark comes only from a live quote received after the connection was resynced (DEC-433 item 7) | Unit test: a backfilled quote is never a `MarkUpdated` source |
| DP-8 | **Text is untrusted and inert.** News and filing text reach deterministic code only as bytes, a digest, a length, and structured vendor fields. Text can influence trading only through a research-agent thesis that passes every mandate spec §8.5 check (eligibility floor, allowlist, corroboration, `max_instruments`, autonomy) (RAID R-05) | Prompt-injection fixtures for every source never reach an order (E17-3, E17-7); a type test shows no text type reaches the order builder or the gate |
| DP-9 | **Only vetted sources.** The data plane fetches only from sources on the allowlist version in effect, and the research query returns only items from it (E17-7, [DEC-101](../project/04-decision-log.md#decisions)) | A fetch to an unlisted endpoint is refused before any byte is sent; a query never returns an unlisted source's item |
| DP-10 | **Tenant isolation.** No tenant-specific datum (subscriptions, the working universe, queries, credentials, broker responses, positions) enters the shared plane, and nothing one workspace captured reaches another ([DEC-09](../project/04-decision-log.md#decisions)) | Two-workspace test: the shared plane's inputs are identical whatever either workspace holds or asks |
| DP-11 | **No redistribution without a license.** A workspace receives exchange market data or licensed vendor text only through its own connection, unless the source is marked redistributable under a recorded license ([HLD §12 risk 3](../HLD.md#12-risks-and-open-decisions)) | Configuration test: a shared-plane dataset without a recorded redistribution grant fails to publish |
| DP-12 | **No directional output from the shared plane** (DEC-62) | Schema test: shared-plane records have no field that carries a direction, score, or rank |
| DP-13 | **Exits never wait on the data plane's slow parts.** No exit, protective order, or kill switch waits on news, filings, backfill, the shared plane, or the research query (rule 13) | Fault injection: with the shared plane and every news source down, the exit suites still pass |
| DP-14 | **Backtests are reproducible.** A backtest reads one store snapshot, named by its manifest digest and recorded in `BacktestRunRecorded` ([PRD FR-4.5](../product/04-prd-v1.md#64-backtest-and-paper-trading)) | Running the same mandate version on the same snapshot twice gives identical bytes |

---

## 3. Live market data

### 3.1 Normalized types

The data service converts every vendor message into the trading spec §4.1 types: exact decimals,
never floating point (ES-23); UTC nanosecond timestamps; the instrument by its `asset_id`, never by
symbol alone. It adds two fields to every record:

| Field | Meaning |
|---|---|
| `vendor_time` | The timestamp the vendor stamped on the message (exchange or consolidator time for equities) |
| `received_at` | When the data service first received it, from the host clock disciplined per [journal spec §5.4](journal.md#54-clock) |

A message that cannot be parsed exactly, names an unknown instrument, or has a side with no price
is refused and counted, never completed or carried forward. This is the rule the built latest-quote
read already follows (E7-8). Alpaca sends a zero price and size for an empty side
([DEC-116](../project/04-decision-log.md#decisions)); that side is absent, not a price of zero.

### 3.2 Alpaca streaming (first connector)

The live source for v1 is Alpaca's market-data WebSocket, on the workspace's own account
([DEC-35](../project/04-decision-log.md#decisions)): `sip` for live equity agents, `iex` for paper,
and the crypto stream for pairs. Robinhood (second connector) and Kraken Derivatives US (third) are
planned; each states its own data path in its connector story before it is built.

**Channels.** Quotes, trades, minute bars, updated bars (late trades revise a bar), daily bars,
trading statuses, LULD bands, and trade corrections and cancels for equities; quotes, trades, and
bars for crypto. Which channels each feed carries is checked against the vendor's documentation
before E2-5 starts (§11 question 1).

**One connection per account and feed.** Vendor plans cap concurrent connections and symbols. The
data service holds one stream per broker connection per feed, shared by that connection's agents
in the same workspace. Never across workspaces: that would be redistribution (DP-11) and a tenant
leak (DP-10).

**Subscription set.** The union of: every held instrument (exits need marks), every instrument in
any working universe on the connection, and any instrument an open approval or exit sequence names.
If the plan's symbol cap is reached, held instruments come first, then universe instruments in
admission order. An instrument left out has no fresh quote, so DP-4 denies openings in it, which is
the safe result. The overflow is reported to the owner.

**Connection states.**

| State | Entered when | Blocks | Ends when |
|---|---|---|---|
| `connecting` | Start, or after `down` | Openings in every instrument on the connection (no fresh quote, status unknown) | Authenticated and subscribed: `resyncing` |
| `resyncing` | Subscribed after a start or a reconnect | Openings, until each instrument has a live quote and a status snapshot received after the subscription | Every subscribed instrument resynced: `live`; an instrument that does not resync stays blocked alone |
| `live` | Resynced | Nothing beyond the trading spec's own rules | A disconnect, a missed heartbeat, an authentication error, or a vendor error: `down` |
| `down` | Any of those | Openings in every instrument on the connection; statuses become presumed halts (DP-5) | The next reconnect attempt: `connecting` |

**Reconnect.** Exponential backoff with jitter, capped at 30 seconds, with no attempt limit.
Every transition is counted for operators; the owner is alerted once a connection has been `down`
for longer than the staleness threshold of its profile. A rejected credential is not retried
in a loop: the connection pauses the affected agents' openings and alerts, as for any other
connection problem ([HLD §5](../HLD.md#5-agent-runtime)).

### 3.3 Gaps, order, and duplicates

- **Gap detection.** Alpaca's stream carries no sequence numbers, so a gap is detected by its
  cause: a disconnect, a missed heartbeat, or a vendor error message. A symbol that is silent is not
  a gap; a missing bar means no trade ([trading spec §4.2](trading-domain.md#42-data-profiles-dec-35)).
  Each gap is a window from the last message received to the moment of resubscription, recorded
  with the connection.
- **Backfill.** Bars and trades for the gap window are fetched from the historical host (the
  E2-1 client) and stored with their own `received_at`. Signal models that need a complete bar
  window wait for the backfill; until then their output counts as missing, which never increases a
  buy ([MI-10](mandate.md#11-invariants)). Backfilled quotes are never marks (DP-7).
- **Order.** For each instrument and feed, a quote whose `vendor_time` is at or before the last
  accepted quote's is stored but never becomes a mark or a collar reference (DEC-433 item 8).
- **Duplicates.** Trades are de-duplicated by feed, exchange, and vendor trade id. Quotes and bars
  are de-duplicated by feed, instrument, `vendor_time`, and content digest. A duplicate is counted
  and dropped from the live path, never stored twice.
- **Corrections and cancels.** A corrected or cancelled trade is a new record naming the original
  (DP-2). Risk marks are quote-based, so a trade correction moves a risk mark only where the last
  trade was the mark source (in session and fresh, trading spec §8.2); the correction then takes
  effect at its own arrival, never back-dated. Bars revised by late trades are stored as new
  versions of the bar, each with its knowledge time.

### 3.4 Time, freshness, and clock skew

- **Quote age** is measured on the risk clock from the earlier of `vendor_time` and `received_at`
  (DEC-433 item 5). For equities it counts regular-session time ([mandate spec
  §5.2](mandate.md#52-inputs-the-risk-clock-and-determinism)). The staleness threshold and spread
  limit belong to the data profile (trading spec §4.2), never to this spec.
- **Future stamps.** A quote whose `vendor_time` is later than its `received_at` by more than
  `future_skew_ms` (configuration, default 1,000 ms) is not sane: it is stored but never marks.
- **Host clock out of tolerance.** While the scheduler has journaled `ClockToleranceExceeded` and
  not yet a later in-tolerance `ClockOffsetRecorded` ([journal spec §5.4](journal.md#54-clock)), no
  quote counts as fresh for an opening (DEC-433 item 5). Exits continue.
- **Broker time wins for executions** (journal spec §5.4); `vendor_time` never drives risk timing,
  which is the risk clock alone.

### 3.5 Sessions, early closes, and halts

- **Sessions** come from the checked-in calendar in `mandate-time`, never from wall-clock
  guesses. A date beyond the file's validity is an error, which blocks openings; the backlog already
  asks for an alert 90 days before the file's last valid date.
- **Early closes** move the regular-session end and the close window (trading spec §9.6). The data
  service takes both from the calendar; it never infers a close from silence.
- **Equity risk marks** update E only in the regular session ([mandate spec
  §5.2](mandate.md#52-inputs-the-risk-clock-and-determinism)). Extended-hours quotes are still
  delivered for exit pricing ([trading spec §5.6](trading-domain.md#56-exit-pricing)).
- **Instrument status.** Each instrument holds one of `trading`, `halted`, `paused_luld`,
  `cooling_off` (after a resume, for the trading spec's cooling-off period), or `unknown`. `unknown`
  is entered at start, at every reconnect, and whenever the status channel is down, and it is a
  presumed halt (DP-5). On a profile whose feed carries no status channel, status stays `unknown`.
  Paper on `iex` may then still open on a fresh IEX quote plus an asset read no older than 60 s
  showing `tradable` (DEC-433 item 18, the founder's decision); live stays SIP-only. The current
  LULD band is kept beside the status.

### 3.6 How perception receives it

The data service pushes normalized records to the workspace's runtimes and executor over a local,
in-process or same-host channel; the hot path never crosses to the shared plane or to a model.
Queues are bounded. Under overload, intermediate quotes for an instrument are conflated to the
latest one; status, LULD, and correction messages are never dropped. A runtime that falls behind
reads the latest quote and its age, so lag shows up as staleness, which is safe (DP-4).

Marks enter risk state only as journaled `MarkUpdated` events, and gate decisions record the quotes
they used (`quotes_used`), per the mandate and journal specs. This spec does not change their
cadence. Signal-model inputs are recorded as `ObservationRecorded` with the bar window stored as an
artifact (DP-3).

### 3.7 Reference data

| Data | Refresh | Point in time |
|---|---|---|
| Instrument snapshot (trading spec §3.1) | Daily after 19:45 ET, after a reject that cites instrument properties, and on corporate-action events | Each snapshot is a configuration object by digest, referenced by gate decisions (journal spec `ins`) |
| Eligibility statistics (prior close, 20-day median daily dollar volume, average daily volume) | After each session's daily bars complete | Computed from bars with knowledge time before the decision; recorded in the gate's `checks` inputs |
| ETP and ETN classification | Per the source chosen for trading spec §15 items 5 and 8 | Versioned list; older than the configured age denies ETP openings (trading spec §3.2) |
| Corporate actions | Daily, and before the 19:45 ET preparation | Each action's knowledge time is when it was first seen; DEC-433 item 13 covers one first seen after preparation |
| Calendars | With releases | Versioned data files with provenance |

---

## 4. News, filings, and fundamentals

### 4.1 Sources

The founder chose the v1 sources (DEC-433 item 16): SEC EDGAR and XBRL in the shared plane, Alpaca
news per workspace, and no paid vendor before the DEC-99 evaluation passes. Categories and options:

| Category | Options | Cost and license | Recommendation |
|---|---|---|---|
| SEC filings | SEC EDGAR (filing index, documents, real-time feed of new filings) | Free, public; the SEC's fair-access rules (declared user agent, rate limit) | **v1 first source**, in the shared plane |
| Fundamentals | SEC XBRL financial data (company facts); commercial fundamentals vendors | XBRL free; vendors paid, usually no redistribution | XBRL in v1; vendors after the DEC-99 evaluation |
| News via the broker | Alpaca news (third-party content through the account) | Included with the account; terms govern display and storage | **v1**, per workspace, on its own connection |
| Licensed news vendors | Commercial news APIs and wire services | Paid; redistribution and retention per contract | Not before the DEC-99 evaluation passes |
| Issuer press releases | Wire services that carry issuer releases | Paid, or free with delay | Later, as a licensed source |
| Social media, forums, blogs | — | — | **Not allowed** in v1 (R-05: anyone can write them) |

A source enters the research agent's view only by being added to the vetted allowlist, a new
version of versioned configuration ([mandate spec §8.4](mandate.md#84-the-research-agent-dec-97-adr-0002)).
How a source is vetted is mandate spec §12 item 8, still open.

### 4.2 Ingestion

1. **Fetch** only from endpoints of allowlisted sources (DP-9). The endpoint list is compiled in or
   signed configuration, as ES-23 does for the broker hosts; anything else is refused before a
   byte is sent.
2. **Store raw bytes** under their SHA-256 digest ([journal spec §6.3](journal.md#63-artifacts)
   style), before any parsing.
3. **Parse structured fields only:** vendor item id, version or update time, published time,
   tickers the vendor lists, SEC form type, filer CIK, accession number, acceptance time. Free text
   is never parsed by deterministic code (DP-8).
4. **De-duplicate** (§4.4).
5. **Tag** (§4.3).
6. **Index** by knowledge time, instrument, source, and kind, and publish.

Every step is idempotent: re-running it on the same raw bytes gives the same records.

### 4.3 Entity tagging (factual only)

Tags come from structured fields alone (DEC-433 item 10): the vendor's own ticker list for news,
and the filer CIK mapped to instruments through a versioned CIK-to-ticker table for filings. No
model reads text to tag it, in the shared plane or the workspace. A tag says "this item names this
instrument"; it never says anything about direction or importance (DP-12), and it never makes an
instrument eligible or admits it (DP-8). Tickers are resolved to `asset_id` as of the item's
knowledge time, so a reused ticker does not attach old news to a new company.

### 4.4 De-duplication and syndication

- **Exact copies** (same digest) are stored once.
- **Vendor updates** of an item (same vendor id, later update time) are new versions linked to the
  first. Each version has its own knowledge time; nothing is overwritten (DP-2).
- **Syndicated copies** of one story are kept but grouped by a deterministic key: the same vendor
  id, the same original source attribution in structured fields, or the same normalized-body digest.
  **Copies in one group count as one source for corroboration** (DEC-433 item 14), so one planted
  story repeated by many outlets cannot corroborate itself (mandate spec §8.5 check 15).

### 4.5 Storage and retention

Each item is a record: source id, allowlist version at ingestion, vendor id and version, published
time, knowledge time, raw-bytes digest, byte length, structured fields, tags, and group key. Text is
stored only as the raw artifact. Filings and XBRL facts are public and kept indefinitely. Licensed
news text is kept as the license allows; what a thesis cited is captured as a journal artifact at
use (DP-3), so a record survives the vendor's retention terms. Only what a thesis cited is kept for
the six years, and a source is allowlisted only if its terms allow that copy; counsel confirms per
vendor (DEC-433 item 20, the founder's decision).

Amended filings (for example a 10-K/A) and restated XBRL facts are new records; the as-reported
value stays readable as of any earlier time (DP-1).

### 4.6 The research query interface

The research agent reads news, filings, fundamentals, and market data only through one read-only
interface in the workspace data service. The agent harness spec defines the tools that call it.

| Parameter | Rule |
|---|---|
| `as_of` | Required; the agent's decision time. Only records with knowledge time at or before it are returned (DP-1) |
| `allowlist_version` | The version in effect; items from other sources are never returned (DP-9) |
| `instruments`, `kinds`, `window`, `limit` | Filters; instruments by `asset_id` |

Every item returned is journaled as `ObservationRecorded` (source, instrument, `as_of`, `data_ref`)
**before** the model reads it.

The E17-5 input-drift detector does not fold the event's own members. `ObservationRecorded` is
closed with `source`, `instrument_id`, `as_of`, and `data_ref`, and `as_of` is the run's cut-off,
the same for every item of a run, so it cannot show a source's arrival pattern. For each news or
filing item the detector folds a **typed per-item observation**: the source, the class, the item's
observation instant, the content hash, and the byte length, never text
([DEC-266](../project/04-decision-log.md#decisions)). That typed record is the artifact the item's
`ObservationRecorded` names in `data_ref`, so the fold replays from the journal and its artifacts
([agent harness spec §6.3](agent-harness.md#63-retrieval)). The journal event needs no new member.

In backtests the same interface runs against a
store snapshot, so research inputs in a backtest are point in time too, although backtests of
LLM theses are evidence of mechanics only ([DEC-99](../project/04-decision-log.md#decisions)).

---

## 5. Storage

### 5.1 Tiers

| Tier | Holds | Where | Retention |
|---|---|---|---|
| Hot cache | Latest quote, trade, status, and LULD band per instrument; recent bars | Memory of the workspace data service | None; rebuilt on restart (§7) |
| Journal | Marks, gate inputs, observations, and the artifacts they reference | Journal hot and cold stores ([journal spec §6](journal.md#6-storage)) | Six years and more ([trading spec §13](trading-domain.md#13-records-retention-dec-33)) |
| Point-in-time store | Captured live data, backfill, history downloads, news, filings, fundamentals, reference data | Parquet datasets on local disk today; object storage in cells and MinIO on the edge later ([HLD §11](../HLD.md#11-technology)) | Operational; set by the operator. It is not the record: the journal is |
| Shared-plane store | Shared datasets (§6) | Ours, per region | Indefinite for public data |

### 5.2 Two time axes

Every stored record has an **event time** (the vendor's time for the market event, or the publication
or acceptance time of a document) and a **knowledge time** (DEC-433 item 4):

- For live captures, knowledge time is `received_at`.
- For history downloaded after the fact, knowledge time is the time the data was first published by
  its source: the end of a bar's interval (start plus interval), a trade's or quote's own time, a
  filing's acceptance time, a news item's published or update time.
- A correction's knowledge time is when the correction arrived, never the time of the record it
  corrects.

Point-in-time queries filter on knowledge time (DP-1). Backtests already treat a bar as known at
its close ([trading spec §6.4](trading-domain.md#64-backtest-fill-model)); this makes the rule the
store's, not each caller's.

### 5.3 Datasets and versions

The built layout stays ([DEC-89](../project/04-decision-log.md#decisions)): one Parquet file per
symbol per UTC day, a canonical-JSON manifest with each day's SHA-256, exact decimal columns, raw
prices, and corporate actions stored beside. Writes compare bytes first, and different content for
a stored day is a refused conflict (DP-2). This spec adds:

- **`received_at`** on captured live data, and a `knowledge_time` column on every new dataset kind.
- **Versions instead of overwrites.** When a vendor revises history, the new download is a new
  dataset version with its own manifest. The old version stays. A snapshot is a manifest digest.
- **Corrections datasets** for trade corrections and cancels, keyed to the originals.
- **New kinds:** quotes captured live, statuses and LULD bands, news items, filings, XBRL facts,
  and reference-data snapshots, each with a manifest.

### 5.4 Where it lives

| Data | Workspace deployment | Shared plane |
|---|---|---|
| Market data from a workspace's connection | Yes | Never (DP-10, DP-11) |
| Subscriptions, universe, queries, what was used | Yes | Never (DP-10) |
| News through a workspace's Alpaca access | Yes | Never (DP-11) |
| SEC filings, XBRL facts, their tags | A pulled copy | Yes |
| Calendars, CIK-to-ticker table, reference lists whose terms allow sharing | A pulled copy | Yes |

---

## 6. Fan-out and deployment modes

### 6.1 Managed

The shared plane publishes **whole datasets**, never per-workspace slices (DEC-433 item 9). A cell
pulls each dataset update and every workspace filters locally. The shared plane therefore never
learns which instruments a workspace watches: the working universe is strategy, rated High in
[HLD "Where data lives"](../HLD.md#where-data-lives).

Each update is a bundle: the records, a manifest with digests, and a signature from a release key.
The workspace verifies the signature and the digests before using a byte, and keeps the bundle
digest so a research observation can name the exact bundle it came from.

### 6.2 Hybrid

The customer's deployment pulls the same bundles over its outbound connection, if it subscribes.
Market data and news come from its own broker connections, from its own site. No inbound port is
opened ([HLD §4](../HLD.md#workspace-deployment)).

### 6.3 Fully on-prem or air-gapped

Bundles are delivered offline, signed, and imported by the customer, or the customer runs no
shared data at all. Market data still comes from the customer's own venue connections. A deployment
with no market data connection cannot place opening orders (DP-4); this is a property, not a mode.

### 6.4 When the shared plane is down

Trading is unaffected (DP-13): it never depends on the shared plane. The research agent sees no
new filings until bundles resume; its query answers stay correct as of their knowledge times, and
the drift detector sees the gap in arrivals. The shared plane's outage is shown to operators, not
to owners as a trading event.

---

## 7. Lifecycle and failure walk

Each row is walked to its exit. "Openings" means opening and increasing orders; exits,
protection, and kill switches are never blocked by the data plane (DP-13), only priced by what it
has (trading spec §5.6).

| Situation | Entered | What it blocks | How it ends, and who ends it | At the close, midnight, restart, version change |
|---|---|---|---|---|
| **Feed down** (whole connection) | Disconnect, missed heartbeat, vendor error | Openings on the connection; statuses become presumed halts (§3.2) | Reconnect, then resync (automatic) | Staleness keeps counting in session time; at restart see "Restart"; a version change does not touch it |
| **Partial outage**: status channel down, quotes flowing | Status messages stop, or the vendor reports the channel failed | Openings, market orders (DP-5) | Channel back and a status snapshot received | Same as above |
| **Partial outage**: some instruments silent | No quote for the profile's threshold | Openings in those instruments only (`stale_mark`, [mandate spec §5.2](mandate.md#52-inputs-the-risk-clock-and-determinism)) | A sane quote arrives | `stale_mark` never flattens; outside the session equity staleness does not count |
| **Symbol cap reached** | Subscription set larger than the plan allows | Openings in the instruments left out | Universe shrinks, or the plan changes | Re-evaluated at each universe change and at restart |
| **Clock skew**: host out of tolerance | `ClockToleranceExceeded` | Openings everywhere (DEC-433 item 5) | An in-tolerance `ClockOffsetRecorded` | Checked before the session opens (journal spec §5.4) |
| **Clock skew**: one quote stamped in the future | `vendor_time` beyond `future_skew_ms` | That quote as a mark | The next quote within the skew bound | — |
| **Duplicates, out-of-order quotes** | §3.3 rules | That message on the live path | Next in-order message | — |
| **Corrected or cancelled print** | Correction message, or a revised bar | Nothing; the correction applies from its arrival | — | Stored as a new record; a backtest sees it only after its knowledge time |
| **Halt or LULD pause** | Status message | Openings; resting marketable openings are cancelled ([§4.4](trading-domain.md#44-halts)) | Resume, then the cooling-off period | A halt across the close stays a halt; at restart status is `unknown` until a snapshot |
| **Corporate action announced before preparation** | Seen before 19:45 ET on the day before the ex-date | As trading spec §8.5 | Applied and posted by the broker | Trading spec §8.5 |
| **Corporate action first seen after preparation, or mid-session** | First seen later than its preparation time | Openings in the instrument (DEC-433 item 13) | Prepared, applied, and posted; or, for an action out of v1 scope, the §8.5 "anything else" rule (pause, cancel, alert) | The owner is alerted; protection stays; the instrument snapshot is refreshed |
| **Restart** of the data service or runtime | Process start | Openings until resync (§3.2): marks folded from the journal are old, so DP-4 applies by itself | Resync completes | Backfill runs for the outage window; bars wait for it; exits use whatever mark source exists (trading spec §8.2) |
| **Backfill fails** | The historical host refuses or times out | Signal models needing that window (output missing, MI-10) | A later successful backfill; the window stays marked as a gap in the store | `inspect` shows the gap as a true gap |
| **Calendar file near its end** | 90 days before its last valid date | Nothing yet; an alert | A release with the next file | Past its end, openings are blocked (§3.5) |
| **Shared plane down** | Bundles stop or fail verification | Nothing on the trading path | Bundles resume (operator) | §6.4 |
| **News source down** | Fetches fail | Nothing on the trading path; fewer theses | Source returns | The drift detector sees the arrival gap |
| **Source revoked** from the allowlist | New allowlist version | New admissions citing it (mandate spec §8.5 check 14) | — | Live theses citing it are invalidated: each instrument leaves the working universe, exits only, protection kept (DEC-433 item 19, the founder's decision; it lands in the mandate spec in its own PR) |

---

## 8. Adversaries

| Who or what | Attempt | What stops it | Residual |
|---|---|---|---|
| **Bad tick** (low) | A wrong low quote fires a drawdown or loss limit, or starts a flatten | Breach confirmation and the two-quote hard trigger ([mandate spec §5.6](mandate.md#56-breach-confirmation-dec-49-dec-54)); tripwires read no marks; `stale_mark` never flattens | A `scale_sizes` rung can tighten for a moment; it tightens only |
| **Bad tick** (high), on H | A wrong high quote raises the high-water mark, so ordinary prices later look like a drawdown that confirms on real quotes | Nothing today | **Open gap.** The founder decided two-quote confirmation for H (DEC-433 item 17). That fix has a cost the decision did not have in view: a genuine spike that prints once at its peak no longer raises H, so a real drawdown from that peak is measured smaller and a rung may not fire. Item 21 puts the trade-off and the alternatives to the founder. Until then the mandate spec is unchanged |
| **Bad tick** (high), on E₀ | A wrong high quote that is the day's last regular-session quote sets E₀ too high, so the next day's real prices confirm a daily loss | Nothing today | **Open gap.** Item 17's mechanism cannot work here: after the day's last quote there is no later regular-session quote to confirm it before E₀ is set at 00:00. Item 21 proposes the official close instead |
| **Bad tick** (high), on the caps | A wrong high bid on a held instrument raises E for one evaluation. The per-instrument cap min(`max_position_usd`, `max_position_fraction` × E), the gross-exposure cap min(`max_gross_exposure_usd`, E), and the order builder's `cap` all rise with it, so a buy that should be clipped or denied is sized larger and admitted | The owner's absolute caps: `max_position_usd`, `max_order_usd`, and `max_gross_exposure_usd` are required fields, so the loosened fraction never exceeds them. Nothing else | **Open gap, and the current exposure.** One wrong high print can enlarge one buy, up to the absolute USD caps. E corrects on the next quote and later openings are checked against the true E, but nothing unwinds the position already opened. Example: equity 100,000, `max_position_fraction` 0.50, `max_position_usd` 60,000, one holding worth 40,000. A bid 40% high on it lifts E to 116,000 and the per-instrument cap from 50,000 to 58,000, so a 58,000 buy in another instrument is admitted where 50,000 was the limit. Item 22 asks the founder whether the caps should read a confirmed mark |
| **Bad tick** (crossed or one-sided) | Mark from a nonsense quote | The sane-quote checks: 0 < bid ≤ ask, spread within the profile's limit (trading spec §8.2) | — |
| **Poisoned news** | A planted article makes the research agent admit and buy an instrument | Allowlist (no open web, no social media); syndication grouping (one story corroborates nothing); corroboration by an independent source or market data; the drift detector; the eligibility floor; `max_instruments`; admission defaults to `ask`; deterministic sizing; the gate (DP-8, R-05) | A plausible false story on a vetted source can still produce a thesis an owner then approves; the envelope bounds the loss |
| **Prompt injection in a filing or article** | Text that instructs the model | Text reaches deterministic code only as bytes (DP-8); the model's output is a thesis that passes §8.5 like any other | The harness spec owns model-side defenses |
| **Malicious or compromised source** | A vetted source starts publishing planted items | Drift detector escalates a changed traffic shape; the operator per-thesis halt (E17-6); revoking the source in a new allowlist version | Live theses citing it stay until DEC-433 item 19 is decided |
| **Replay of old news** | An old story republished as new | Knowledge time is when it arrived; the vendor's published time is kept and shown; the item's digest matches the old one and groups with it | — |
| **Future-dated items** | A timestamp in the future to look fresh, or to leak into an earlier backtest | Knowledge time is never later than receipt for live data, and future quote stamps are not sane (§3.4) | — |
| **Ticker confusion** | A reused or changed ticker attaches news or bars to the wrong company | Everything is keyed by `asset_id`, resolved as of knowledge time (§4.3); symbol changes are out of v1 scope and pause the agent (trading spec §8.5) | — |
| **Vendor outage** | Quotes or statuses stop | DP-4 and DP-5: openings stop, exits continue | Exit prices may be worse; trading spec §5.6 prices them |
| **Flood** | A source sends a burst to swamp the agent or the hot path | Bounded queues with conflation (§3.6); the drift detector's arrival-rate measure; slow-tier work never blocks the hot tier | — |
| **Careless user** | Runs live with an IEX-only plan | DEC-35: live equity agents require SIP, checked at deployment | — |
| **Malicious insider** | Edits stored history to flatter a backtest, or alters a record | Datasets refuse different content for a stored day; snapshots are manifest digests in `BacktestRunRecorded`; journal artifacts are hash-chained and object-locked (journal spec §6.2) | The bulk store is operational, not the record; what a decision used lives in the journal |
| **Cross-tenant leak** | Learn another workspace's universe from shared-plane traffic | Whole-dataset broadcast, pull only (§6.1); no per-workspace request reaches the shared plane (DP-10) | — |

---

## 9. What exists and what is planned

As of 2026-10-03.

| Part | State | Where |
|---|---|---|
| Historical download of bars and trades into Parquet, idempotent, exact decimals | **Built** (E2-1) | `crates/mandate-marketdata` |
| `inspect`: coverage, gaps classified by venue hours, duplicates, statistics, quotes datasets | **Built** (E2-2, DEC-116) | `crates/mandate-marketdata` |
| Corporate actions stored beside a dataset; split-adjusted and raw prices | **Built** (E2-4) | `crates/mandate-marketdata` |
| Exchange calendar and sessions, with validity to 2028-12-31 | **Built** | `crates/mandate-time` |
| Latest-quote read (`iex`, `crypto`) and asset read, refusing stale or one-sided answers | **Built** (E7-8) | `crates/mandate-alpaca` |
| Source allowlist type and admission checks 14 and 15 | **Built** as types and checks; no allowlist contents yet | `crates/mandate-research` |
| Input-drift detector over observation shapes | **Built** as a pure fold; not wired to inputs (DEC-266, DEC-267) | `crates/mandate-research` |
| A news and bars research loop | **Spike only**, outside product code (E17-0) | `python/research_spike` |
| Live streaming, reconnect, resync, status and LULD | Planned (E2-5, E2-6) | — |
| Knowledge time, versions, corrections, as-of queries | Planned (E2-7) | — |
| Reference data (eligibility statistics, ETP list) | Planned (E2-8) | — |
| Filings, fundamentals, news ingestion, tagging, grouping | Planned (E2-9, E2-10) | — |
| Research query interface | Planned (E2-11) | — |
| Shared plane and fan-out | Planned (E2-12); [HLD §11](../HLD.md#11-technology) lists it as having no code | — |
| Data-plane fault-injection suite | Planned (E2-13) | — |

The tracer (`mandate-shell`) reads stored bars and the latest-quote endpoint today. A presumed halt
is read from the latest quote only until a status feed exists (backlog, E7-4 slice 5's session part).

---

## 10. Decisions

All are in [DEC-433](../project/decisions/DEC-433.md).

**Accepted by the agent** (reversible engineering readings; each only tightens, per DEC-176):

| Item | Reading |
|---|---|
| 1 | This spec is a draft that adds no gate rule; where it overlaps, the trading, mandate, and journal specs win |
| 2 | Backlog stories continue epic E2 (E2-5 onward), not a new epic |
| 3 | No exchange market data and no licensed text in the shared plane; each workspace uses its own connection |
| 4 | Two time axes, event time and knowledge time; point-in-time queries filter on knowledge time |
| 5 | Quote age from the earlier of vendor time and receive time; future stamps beyond 1,000 ms are not sane; no fresh quote for openings while the host clock is out of tolerance |
| 6 | A status channel that is down or not resynced is a presumed halt that also blocks openings |
| 7 | Backfilled data never becomes a mark |
| 8 | Out-of-order quotes never mark; records are never edited; corrections are new records |
| 9 | Fan-out is whole-dataset broadcast, pulled; the shared plane never sees a workspace's instruments or queries |
| 10 | Tagging is deterministic from structured fields; no model in the shared plane |
| 11 | Fetches go only to allowlisted endpoints, checked before sending |
| 12 | What a decision used is captured into the journal by digest at use |
| 13 | A corporate action first seen after its preparation time blocks openings in the instrument until applied |
| 14 | Syndicated copies of one story count as one source for corroboration |

**Decided by the founder on 2026-10-03**, each as recommended (DEC-433, "Founder decisions"):

| Item | Decision |
|---|---|
| 15 | No redistribution of exchange market data in v1 |
| 16 | SEC EDGAR filings and XBRL facts in the shared plane; Alpaca news per workspace; no paid vendor before the DEC-99 evaluation passes |
| 17 | A quote that would raise H, or become E₀, counts only after a second sane quote at least min(`breach_confirm_s`, 10 s) later confirms it. It lands as a mandate spec and reference PR first. **Item 21 reports that the E₀ half cannot work as drafted and that the H half has a cost; until the founder rules on item 21, the mandate spec is unchanged** |
| 18 | Paper on `iex` with no status channel may open on a fresh IEX quote plus an asset read no older than 60 s showing `tradable`; live stays SIP-only |
| 19 | Revoking a source invalidates every live thesis that cites it |
| 20 | Keep only what a thesis cited, as journal artifacts; allowlist only sources whose terms allow that; counsel confirms per vendor |

**Proposed for the founder** (raised by the post-merge review; the mandate spec, the gate, and the
order builder stay unchanged until decided):

| Item | Question | Options | Recommendation |
|---|---|---|---|
| 21 | Item 17 as decided does not work for E₀, and its H half weakens the drawdown ladder on a genuine single-print spike | **E₀:** (a) set it from the official close the trading spec already defines (§8.2, "End of day"), with the last confirmed regular-session mark as the fallback if no official close has arrived by 00:00; (b) the last confirmed mark only; (c) leave it. **H:** (a) item 17 as decided; (b) raise H to the lower of two consecutive sane quotes' equity, with no waiting time; (c) a shorter one-sided wait; (d) leave H unconfirmed | E₀ (a). H (b): a wrong print followed by a right one cannot raise H, and a genuine spike keeps all but its single highest quote |
| 22 | One wrong high print raises E for one evaluation and so enlarges one buy (DP-6 gap (c)) | (a) leave it and disclose it: the absolute USD caps bound it; (b) the E-scaled caps and the builder's `cap` read the lower of the latest E and E on confirmed marks, which can only shrink an opening and never touches an exit; (c) hold any opening for a confirming quote after equity jumps | (b). It only tightens, but it changes the gate and the builder, so it is the founder's |

---

## 11. Open questions

1. Which channels each Alpaca feed carries: trading statuses, LULD bands, and corrections on `iex`
   and on `sip`; the symbol and connection limits per plan. Verify against the vendor's
   documentation before E2-5 (it decides whether item 18's exception is ever used).
2. `MarkUpdated` carries instrument, price, source, and feed ([journal spec §9](journal.md#9-event-catalogue)).
   DP-3 wants the quote's vendor time and receive time too. A journal spec change, proposed in its
   own PR (E2-6).
3. The ETP and ETN classification source and refresh cadence (trading spec §15 items 5 and 8).
4. Whether trading spec §8.5 should state DEC-433 item 13's rule for late corporate actions itself.
5. Robinhood's data path over MCP: polling cadence, quote freshness, and whether status data
   exists (RAID R-25). Decided in its connector story.
6. How a source is vetted and who signs off (mandate spec §12 item 8).
7. Whether DP-6's fix (items 17 and 21) should also treat a quote outside the current LULD band as
   not sane.
8. DEC-433 item 14 redefines "independent source" for mandate spec §8.5 check 15: syndicated copies
   of one story are one source. The mandate spec does not say so yet, and the built check in
   `crates/mandate-research` counts raw cited sources, so until both change a story repeated by
   several vetted outlets still corroborates itself at admission. The amendment tightens only, so an
   agent may take it under DEC-176; it lands as a mandate spec and reference-case PR first, then
   tests, then code (E2-15). E2-10's grouping alone does not close the R-05 path.
