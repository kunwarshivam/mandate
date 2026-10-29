# infrastructure

DEC-2000 to DEC-2099. The rules in force:

- **Hosting is OpenTofu + DigitalOcean managed Kubernetes + Flux GitOps**, one staging and one
  production cluster, all of it in the US West (clusters and VPCs in sfo2, the archive in R2),
  reconciled from `main` by Flux; the founder creates workload secrets at bootstrap
  (DEC-2011, DEC-2015, DEC-2047).
- **`infra.yml` alone plans and applies**: from `main`, on dispatch, under an environment the
  founder approves; `sha` is optional for preflight and plan, required for apply; staging's apply
  verifies read-only that staging is up (DEC-2014, DEC-2042, DEC-2050).
- **Release images are signed and admitted by signature**: a cosign key held in the protected
  `release-signing` environment; images publish only from a founder-approved signing run as
  `signed-<commit>`; hosted clusters check signatures at admission without blocking a brake
  (DEC-2024, DEC-2026, DEC-2016).
- **Secrets live in OpenBao**: the workspace vault in server mode on raft over TLS 1.3; staging's
  vault unseals itself with the static seal from a bootstrap Secret; per-connection processes log
  in by Kubernetes auth through roles a dedicated provisioner makes; the platform's own secrets
  are each read by one process (DEC-2019–2022, DEC-2060–2062).
- **Egress is deny-by-default and exact**: hosted processes reach external hosts by exact name
  through Cilium; only the token-exchange process reaches Alpaca's live host; every process that
  reaches a public TLS host gets the pinned CA bundle (DEC-2058, DEC-2059, DEC-2371).
- **CI's cell jobs run on self-hosted ephemeral DigitalOcean runners** under a cost cap, proving
  their isolated runtime before registration; `select` and the required aggregates fall back to
  GitHub's runners (DEC-2065, DEC-2072, DEC-2074–2078).
- Staging runs the workspace vault, the dispatcher with an opaque recipient, a model gateway from
  a registry file the chart renders, the IEX data process, and the news reader; production runs
  only signed releases and may pin per key any digest staging held on `main` (DEC-2051–2052,
  DEC-2080–2084, DEC-2048).


<a id="dec-2000"></a>

## DEC-2000: the kill-switch surface in the cell, for E21-34

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | The O4 builder agent (Claude Code), under DEC-79 (reversible engineering readings) and DEC-176. Each item either tightens a rule or fills a gap with the reading that adds no risk. None commits anything the spec does not already define. Every brake still commits whatever else fails |
| **Story or claim** | E21-34 (lane O4), claim [#1526](https://github.com/kunwarshivam/mandate/issues/1526); infrastructure design §3.6, OPS-4; [trading spec §5.5](../../specs/trading-domain.md#55-kill-switch); [journal spec](../../specs/journal.md) §9.2 (`OwnerCommandIssued`); mandate spec §6.1, MI-23; builds on DEC-158 option (c), DEC-290, DEC-485 item 1 and DEC-967 item 2 |

### Context

`crates/mandate-cli/src/agent.rs` has had `pause` and `kill` since M7, but the binary's `agent`
command offered only `deploy` (`deploy/README.md`, *Deferred*). E21-34 wants both in the binary
at agent, connection and workspace scope. Each must run as a one-off Job and a `docker compose
run` with only the journal's role, and a local-cell test must show each scope's
`KillSwitchActivated` in every stream owner while the API, the deployment manager, the exporter,
the model gateway and the control plane are all unreachable. The R0 freeze rules out the CLI's
live gesture entry (`gestures.rs`, which holds `AgentCommand`). The journal's `pause` is
agent-scoped only, and the cell runs no runtime or executor yet.

### Decision

1. **The surface.** `mandate agent pause` and `mandate agent kill` live in a new module,
   `crates/mandate-cli/src/brake.rs`. Its `AgentCommand` flattens `gestures::AgentCommand` (so
   `agent deploy` is unchanged and `gestures.rs` is untouched) and adds `Pause` and `Kill`. Each
   takes exactly one of `--agent <ID>`, `--connection <ID>` or `--workspace-wide`, which clap
   enforces, plus the owner and journal flags every control command takes.
2. **A wider pause fans out.** The journal's `OwnerCommandIssued` `pause` names one agent, so a
   pause of a connection or a workspace commits one agent-scoped pause for each agent it reaches.
   No new scope or schema is needed. An agent is reached when its latest `AgentDeployed` has no
   later `AgentStopped`. For a connection, the agent must also have a stored mandate whose
   `connection_id` names that connection, *or* a mandate that cannot be read. A pause adds no risk,
   so an unknown connection is paused rather than skipped (rule 3). `--agent` reaches the named
   agent whether or not it is deployed, as `agent.rs`'s pause always has. With no agent in scope,
   the command commits nothing, prints so, and succeeds.
3. **A failing pause does not stop the others.** Each agent's pause is tried in id order. One that
   cannot commit is reported on its own line, and the rest still commit. The command then exits
   non-zero, naming how many did not commit. An agent id that is not a stream id's is refused as
   `agent_invalid` before anything is appended.
4. **The kill switch carries no step-up.** `agent kill` commits `agent::kill` with no typed code:
   one `OwnerCommandIssued` of the scope, never refused, never folded into an earlier one
   (DEC-158 option (c)). The stream owners stop and flatten either way. Without step-up, equities
   sell in the regular session, not outside it (MI-23; trading spec §5.5 "Without confirmation,
   equity sells wait for the session"), which is the conservative side. Selling one position
   outside the session stays the web app's owner exit.
5. **How it runs.** In the cell, it runs as the chart's existing `control` run Job (role
   `mandate_control`, `templates/run.yaml`). On one host, it runs as `docker compose run control
   agent …`. No new process type is added. The commands reach only the journal's Postgres, though
   the `control` process type's egress also lists the vault and the identity provider for other
   commands.
6. **What the drill shows now.** The cell's `kill-switch` drill (OPS-4) scales to zero every
   Deployment of `api`, `deploy-manager`, `exporter`, `model-gateway` and `control-plane`, and waits
   until none of their pods is left. It then opens a control stream and commits, each through its
   own `control` Job, the agent, connection and workspace kill switches and an agent pause. It
   exports the stream, verifies it by journal spec §11, and checks every brake is in it. Seeded, it
   skips the connection kill switch, and the check must name it (`no kill_switch of scope
   connection`). The `compose` drill runs the same brakes through `docker compose run` and checks
   them in its exported segment. The story's last clause, each scope's `KillSwitchActivated` in
   every stream owner, cannot be shown in this PR. The cell runs no runtime or executor yet, and
   `mandate-executor` applies only the agent scope (DEC-485 item 1). So the drill prints that part
   as `skipped` with its reason, never as held, and E21-34 stays open for it.
7. **`Verdict::Skipped` goes.** With the kill-switch drill running, no drill is skipped by name any
   more, so the variant is removed. A future drill that cannot run yet adds it back.

### Rationale

The fan-out needs no journal spec change and lands every brake the story asks for now. Each pause is
the event every stream owner already folds. Choosing "pause when unsure" over "skip when unsure"
follows rules 2 and 3: a pause never adds risk. Committing the kill switch without step-up keeps it
one command with nothing to type, which is what a brake on a degraded cell needs.

### Alternatives considered

- **A connection- or workspace-scoped `pause` in the journal:** a spec change (ES-22) for something
  the fan-out already gives. Rejected for now. It can follow if one event per scope is wanted.
- **Adding the subcommands in `gestures.rs`:** that is the R0 live gesture entry (DEC-967 item 2).
- **`--code` on `agent kill`:** the CLI has no way yet to show the scope's code without a second
  command, and a brake that asks for one invites waiting. It can come with E9-4's step-up.
- **Holding the drill until the runtime and executor run in the cell:** that leaves OPS-4 entirely
  unproven. The command and journal half is what an outage of the API and the tooling threatens.

<a id="dec-2002"></a>

## DEC-2002: the demo's scenario on the local cell runs what is on `main` and names every gap

| | |
|---|---|
| **Status** | Accepted; items 5 and 8 superseded by [DEC-2010](infrastructure.md#dec-2010) |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | the agent (Claude Code, lane E2E), as reversible engineering and process choices under DEC-79 and DEC-176; none loosens a rule |
| **Story or claim** | E21-54, claim [#1528](https://github.com/kunwarshivam/mandate/issues/1528) |

### Context

The demo of 2026-10-26 (DEC-995) tells one story: sign in, deploy under a confirmed envelope,
research, an asked opening approved and filled with its protective stop, a backtest of the same
version, and the kill switch. No single story covers it, and most of its links are in open PRs or
not yet started. The coordinator needs one command that shows the whole story, and a list of
exactly which links are missing.

### Decision

1. **One command, one scenario.** `cargo xtask cell demo --scenario demo` runs the demo's six steps
   in order on the local cell (`xtask/src/cell/scenario.rs`). `--scenario` takes a name so that a
   later scenario (a refusal, a rule-13 exit) is added beside it rather than as a flag on this one.
2. **Real where on `main`, named where not.** A step's gesture runs only through the processes and
   commands on `main`, and passes only when the command printed what its success prints. A
   missing link is printed as `gap`, with the PR or story that closes it, and an xtask test refuses
   a gap that names neither. The scenario never fakes a step: no fixture stands in for a process,
   and a gap does not fail the run.
3. **Gestures in one pod.** The cell's run Jobs each get an empty `/work`, so an artifact store
   written by `version create` would be gone before `version confirm` reads it. The scenario runs
   every gesture as an init container of one Job of the chart's own `control` process, in order,
   after the chart's egress gate and credentials copy, with the demo's files mounted read-only
   from a ConfigMap. The chart is not changed; the Job is the chart's run Job, rendered and then
   extended in xtask, as the journal drill's seeded tamper already is. A persistent artifact store
   in the cell is lane S's (E21-40), not this scenario's. The closing `journal export` and
   `journal verify` run as the pod's last two containers, against that same store, so journal
   spec §11's artifact check covers every record the gestures wrote; a separate verify Job has no
   store and reads a record's artifact as missing.
4. **The owner's codes are typed as shown.** Each two-step gesture first shows its code, which
   commits nothing, then types it. The scenario computes DEC-530 item 4's code itself; after the
   Job it checks that the CLI showed that same code, and the CLI itself refuses a typed code that
   differs, so a drift in either fails the run. A gesture reads as run only when its container
   exited 0 and printed what its success prints.
5. **A fresh workspace each run.** A confirmed version refuses a second confirmation
   (`version_invalid`), so each run opens `demo<run id>` and founds it for the demo's founding
   member, and never depends on an earlier run.
6. **The demo's envelope asks.** `deploy/cell/demo/mandate.json` is paper, has no autonomy rule (so
   every opening is ASK), keeps protection on, and pins the one instrument the scenario registers.
   It is stored as canonical JSON, so its version is the SHA-256 of the file, and a test checks
   both properties.
7. **An optional CI job.** `cargo xtask ci cell-demo` runs the scenario on the CI runner's cell as
   its own job, `cell-demo`, which no required check depends on. It keeps the ready steps green
   without adding minutes to the `cell` job, which is already near its ten-minute bound
   (DEC-464). Being optional, it is not in `cargo xtask check`'s per-PR jobs; run it with
   `cargo xtask ci cell-demo`.
8. **The founding member and the owner's user.** The CLI's owner gestures accept only a lower-case
   user (DEC-527 item 3), while a member is an upper-case ULID (identity spec §3.1). The scenario
   uses one ULID, lower-cased for `--user`. Binding the owner's gestures to a membership is the
   identity lanes' work, not this one's.

### Consequences

- The coordinator has the gap list as running output, and a PR that closes a gap changes the
  scenario's plan in the same change, or in the next slice of E21-54.
- The scenario adds no dependency, no crate, and no change to a safety-critical crate or to any
  R0-path file.

### Alternatives considered

- **A ConfigMap-backed or persistent volume for `/work` in the chart:** a chart change for a demo
  need, and lane S's job (E21-40) is the real artifact store.
- **Stubbing the missing processes so that every step shows something:** it would show a story
  the code cannot tell yet, which rule 2 above refuses.
- **Adding the scenario to the `cell` job:** that job is near its ten-minute bound.

<a id="dec-2004"></a>

## DEC-2004: the demo's scenario names, for each gap, what is on `main` and every lane it waits on

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the agent (Claude Code, lane E2E), as a reversible engineering and process choice under DEC-79 and DEC-176; it only tightens DEC-2002 item 2 |
| **Story or claim** | E21-54, claim [#1528](https://github.com/kunwarshivam/mandate/issues/1528); DEC-2002 |

### Context

DEC-2002 item 2 prints each missing link of the demo as `gap` with free text naming "the PR or
story that closes it". The text drifted: by 2026-10-18 six of the PRs it named as still needed were
on `main` (#1537, #1559, #1560, #1572, #1575, #1582), so the report undersold what `main` can do.
A single sentence also could not say which lanes a gap waits on when it waits on several, and the
coordinator, with a lane per gap, had to read every line to find what each lane holds back.

### Decision

1. **A gap is structured.** Each gap carries what it would show, what of it is on `main` (citing
   at least one PR), and one or more waits. A wait names the lane, the backlog story, the open PR
   when there is one, and what that lane still has to land for this gap. A wait may name lane E2E
   itself, when the pieces are on `main` and only this scenario's wiring is missing.
2. **Tests refuse a vague gap.** An xtask test refuses a gap whose on-`main` text cites no PR, a
   gap that waits on no lane, a lane that is not a short upper-case name, a story the backlog has
   no entry for, a wait that lands nothing, and a PR numbered zero.
3. **Each step reads `ready`, `partial` or `skipped`.** A step with no gap is ready, one made only
   of gaps is skipped, and any other is partial. The step's heading names the lanes it waits on.
4. **The report closes by lane.** After the steps, every wait is listed once under its lane, with
   the step it holds back, printed whether or not a gesture failed.
5. **The README's table is a dated snapshot.** `deploy/cell/README.md` shows the report as of its
   date; the run's own output is the up-to-date list.

### Consequences

- A PR that lands a lane's piece edits that wait (or drops it), and the test keeps the rest named.
- No step's verdict changes: a gap still never fails the run and is never faked (DEC-2002 item 2).

### Alternatives considered

- **Keeping free text and fixing the stale sentences:** the drift would recur, and the text could
  still not be grouped by lane.
- **Reading lane state from GitHub at run time:** the run would need the network and a token, and
  the scenario runs with neither.

<a id="dec-2005"></a>

## DEC-2005: The stub issuer's tier comes from `deploymentTier`, the chart's one tier setting

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It keeps every refusal DEC-1025 item 3 names and adds a test; it weakens no safety rule) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the coordinating agent, resolving the merge of `main` into #1539 |
| **Story or claim** | E21-51 (lane H, claim #1527), E21-52 (lane SI, claim #1532) |

### Context

#1539 makes `deploymentTier` the workspace chart's single source of `MANDATE_DEPLOYMENT_TIER` and
removes `api.tier`. Meanwhile, on `main`, DEC-1025 wired the stub token issuer's sidecar to read
`api.tier`. Merging the two left the sidecar reading a value that no longer exists.

### Decision

This supersedes DEC-1025 item 1's second bullet and the third check of its item 3. The rest of
DEC-1025 stands.

1. The stub's tier is the chart's `deploymentTier`, the same value the server reads. The two can no
   longer differ.
2. The chart refuses the stub unless `deploymentTier` is `dev` or `ci`. The other two checks of
   DEC-1025 item 3 are unchanged: the schema allows only a local `mandate.local/` build, and the
   chart allows a local build only under `localImages`.
3. The local cell's values set `deploymentTier: ci`, so the local cell's stub runs as `ci`, which it
   admits.
4. `deploy/tests/test_cell.py`'s `test_stub_sidecar` refuses `deploymentTier` `staging` and
   `production`. It also refuses hosted cell #1's values (`deploy/hosted/values/cell1.yaml`) with a
   stub image. Their tier is `dev` under DEC-2001 item 6, so only the local-build gate stops the
   stub there, and the test pins that.

### Rationale

One tier setting cannot let the sidecar and the server disagree. Hosted cell #1's `dev` tier would
pass the tier check on its own, so item 4's case makes the local-build gate a tested guarantee
rather than an incidental one.

### Alternatives considered

- **Restore `api.tier` beside `deploymentTier`.** That brings back the two-sources problem #1539
  removed.

<a id="dec-2006"></a>

## DEC-2006: Cell #1's journal takes main's synchronous standby and WAL archive, and its bucket waits for the founder

| | |
|---|---|
| **Status** | Accepted (agent: a reading under DEC-176 that only tightens DEC-2001 and adds no risk; reversible engineering under DEC-79). The archive's bucket stays the founder's (DEC-79: spending), so the hosted `cluster` step refuses until it is named |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | `claude-code`, fixing #1539's `hosted-k3d` job for the coordinating agent |
| **Story or claim** | E21-51 (lane H, claim #1527); [DEC-2001](DEC-2001.md) items 9 and 11; E21-41 and [DEC-1050](earlier-900-1999.md#dec-1050); [DEC-995](DEC-995.md) (Proposed) |

### Context

Main's journal-db chart (E21-41, DEC-1050) now requires at least two instances, a primary and a
synchronous standby, and an archive for the WAL and base backups through the Barman Cloud plugin.
Its schema refuses an empty `archive.destinationPath` or `archive.endpointURL`. #1539's
`deploy/hosted/values/cell1-journal-db.yaml` was written before that, with one instance and no
archive. Nothing rendered that file except CI's `hosted-k3d` job, which then failed. On k3d, the
local cell's test store can serve as the archive. On cell #1 the archive is an external
S3-compatible bucket, and choosing and paying for it is the founder's decision (DEC-79, DEC-995).

### Decision

1. **Cell #1's journal is a primary and a synchronous standby**: `instances: 2` with
   `failureDomain: separate`, one on each of DEC-995's two droplets. DEC-2001 planned the standby
   for lane S2; main's chart now requires it.
2. **The k3d trial archives exactly as the local cell does.** `cell1.py --target k3d` installs the
   Barman Cloud plugin (with `deploy/cell/charts/backup-plugin-tls`) and the
   `deploy/cell/charts/archive-store` test store, with `clientNamespace` set to the workspace's
   namespace, as `cargo xtask cell up` does. It also passes `deploy/hosted/values/k3d-journal-archive.yaml`,
   whose archive block a test holds equal to `deploy/cell/values/k3d-journal-db.yaml`'s.
3. **On cell #1, no archive is invented.** The committed hosted values name no destination. The
   hosted `cluster` step, dry run included, refuses before it calls any tool unless the founder
   passes `--archive-destination s3://<bucket>/<path>`, `--archive-endpoint https://<host>` and
   `--archive-egress-cidr` (each /24 or narrower). Each value is checked against the chart's own
   patterns. The archive's access key arrives only as two 0600 files
   (`--archive-access-key-id-file`, `--archive-secret-access-key-file`) and goes on kubectl's stdin
   into the Secret `journal-archive-key`. Nothing commits it, and the existing secret guard covers
   it. The runbook lists this as gate 5, beside DEC-995's gate 2.
4. **The Barman Cloud plugin's pins are the local cell's**: chart, image and sidecar, in
   `cell1.json`. `test_pins_match_xtask` holds them equal to `xtask/src/cell.rs`.
5. **Every existing refusal stands**: `--accept-dev-tier`, the secret-file and command-line
   guards, the R2 and GHCR flags, and `k3d` having no Cloudflare or DigitalOcean step.

### Rationale

The chart's rule is a durability rule: no commit without a standby, and no WAL without an archive.
Taking it on cell #1 only tightens DEC-2001. A refusal is the conservative default for the one
input that is spending, and it says which gate to clear. The tests now render both targets' journal
values through the chart's schema, so this kind of drift fails before CI's k3d job.

### Alternatives considered

- **Reuse the R2 bucket `mandate-cell1-journal-cold`.** Rejected: it is the journal's §6.2 cold
  store, which the archive is not (DEC-973 item 6). Pointing the archive at R2 would also choose a
  provider for the founder.
- **One instance and no archive on cell #1, through a chart override.** Rejected: it weakens
  E21-41's durability rule, which only the founder could approve.

<a id="dec-2007"></a>

## DEC-2007: the executor's binary runs on a fixture paper broker behind Alpaca's client, and journals the connect check of a paper connection

| | |
|---|---|
| **Status** | Accepted (agent: reversible engineering decisions under DEC-79 and DEC-176; paper and fixtures only, and nothing loosens a rule) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | `claude-code` (builder of lane X, the executor slice of E21-30), under DEC-79 and DEC-176 |
| **Story or claim** | E21-30, claim [#1498](https://github.com/kunwarshivam/mandate/issues/1498) (lane X, slice X2); infrastructure design §3.1, §3.4, §3.5; connections spec §8.1; journal spec §9.8; DEC-2003, DEC-1171 |

### Context

DEC-2003 built the executor process as a library. The cell needs a binary that runs it, and a
broker it can reach without a broker host. E21-30's criteria say the executor's broker in CI is a
fixture, never a broker host.

Lane D1's deployment manager starts an executor only for a version-2 `AgentDeployed` (DEC-1171).
`mandate connection record --checked` needs a passing `ConnectionChecked` on the account stream
before it can write the version-2 `ConnectionEstablished` such a deployment follows. Until now, only
`mandate-paper`'s Robinhood mode wrote that record.

### Decision

1. **The fixture paper broker answers below `mandate-alpaca`'s client.**
   - **What it is.** `paper::FixturePaper` is a `TradingTransport` that answers the paper trading
     host's requests in process, with the bodies that host sends, so every rule the client holds
     applies to it unchanged.
   - **What it answers.**
     - an active paper account with 100,000 dollars and no account number;
     - no position and no fill;
     - an order accepted and resting, or `422` for a `client_order_id` it already holds;
     - open orders, a lookup by client order id (`404` for one it never took), and a cancel.
   - **Ids.** They come from a counter in UUID form, so a run is deterministic.
   - **What it never does.** It names no host, holds no credential, and opens no socket.
   - **Fills.** Nothing fills in this slice. Fills and the periodic reconciliation that reads them
     land with the order slice (X1b).
2. **The process reaches it through `alpaca::AlpacaBroker`.** This adapter drives the same
   `TradingClient` the shell drives, over whatever transport it is given. It owns one
   current-thread tokio runtime (`tokio` `rt` and `time`) because the client is async and the
   process is not. This crate names no host and builds no URL.
3. **The binary, `mandate-executord`.**
   - **`run --probe <socket>`.** It runs over Postgres (`MANDATE_JOURNAL_DSN`) or, with
     `--fixture`, an in-memory journal. It passes once a second and drains on SIGTERM (`tokio`
     `signal`).
   - **Where it learns its account.** It names the account by flags, or by the deployment manager's
     `MANDATE_PROCESS` (`executor`), `MANDATE_WORKSPACE` and `MANDATE_SUBJECT` (the `account_ref`),
     as the runtime's launch does (DEC-1015).
   - **Refusals.**
     - A pod for another process is refused (`process_not_executor`).
     - A flag that disagrees with a variable the manager set is refused (`account_conflict`).
     - A launch with no workspace or account is refused (`account_missing`).
   - **`probe livez|readyz --probe <socket>`.** This is its `exec` probe (DEC-994 item 5).
   - **The broker.** It is the fixture in every mode. A host transport comes with the vault's lease
     (E21-32) and is its own change.
4. **`connect-check` journals the connect check of a paper Alpaca connection.**
   - **Arguments.** `--workspace`, `--account`, `--connection`, `--pii-ref`, and optionally
     `--fixture`.
   - **The checks.** It reads the account through the broker and runs
     `mandate_connections::checks::run` (checks 1, 2, 3 and 7; scope as an Alpaca key whose
     permissions the venue does not report, DEC-685).
   - **What it writes.** It appends `StreamOpened` (new stream only) and `ConnectionChecked`
     (occasion `connect`) on the account stream in one batch, as `mandate-paper`'s Robinhood check
     does. It prints `checked <event id>`, or `check_failed <event id>` with a failure still
     journaled, so `connection record --checked <event id>` can bind it.
   - **The personal-data reference.** The account's number is never read. The record carries the
     `pii_` and ULID reference it is given, refused before anything else when it is not one
     (journal spec §6.4), and names it in the envelope's `pii_refs` (rule 62). `mandate-store`
     gains `draft_bytes_with_pii`, a helper beside `draft_bytes`; every existing caller is unchanged.
   - **It fences.** It takes the account stream's writer epoch, so it fences an executor already
     running there, and that executor's restart replays. The cell runs the check before the manager
     starts the account's executor, as a one-off Job like the CLI's (X3).
5. **The binary's wiring is the fixture wiring** (`fixture::wiring`, as DEC-1013 item 2 does for
   the runtime).
   - **What it knows.** No mandate version, no instrument and no binding-gate fact, so the core
     admits no opening: an unavailable input is never an allow.
   - **The kill switch.** It reads none of these for an agent that holds nothing, so it is applied
     as ever.
   - **Ids.** The account stream's deterministic ids are space `1`, then epoch, head and ordinal: 26
     digits, as a ULID is. The runtime uses space `0`.
   - **Production ports.** They come with the order slice and the post-R0 hook (P5).

### Slices

Each slice is one PR of about 400 lines of non-test code or less, as the coordinator directed.
- **X2a:** the fixture paper broker (item 1) and the adapter (item 2). It also reserves this
  decision.
- **X2b:** the connect check (item 4), `mandate-store`'s `draft_bytes_with_pii`, and the binary's
  fixture wiring (`fixture.rs`), which the check's ids come from.
- **X2c:** the binary (item 3) and its launch.

### Rationale

The fixture sits below the client, not in place of it, so the cell exercises the client's request
building, response parsing and status table as production does. The connect check reuses the
connections crate's checks and the Robinhood check's record shape, so `connection record` reads it
with no change.

### Alternatives considered

- **A fixture at the `BrokerConnector` level.** Rejected: it skips the client, so the cell would
  prove less than it seems to.
- **The executor runs the connect check itself at start, when the connection is `connecting`.**
  Not now: no executor runs before the connection is recorded (DEC-1171), and a one-off check
  matches the CLI's connect flow. Revisit with the connection manager (E10-13).
- **Simulated fills in this slice.** Deferred to X1b: the executor learns fills only through
  reconciliation, so fills need the periodic reconciliation the order slice adds.

<a id="dec-2010"></a>

## DEC-2010: each demo run takes a fresh ULID workspace founded for the demo owner, and the API is pointed at it

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the agent (Claude Code, lane E2E), with the coordinator's agreement, as reversible engineering choices under DEC-79 and DEC-176; it supersedes DEC-2002 items 5 and 8 and loosens no rule |
| **Story or claim** | E21-54, claim [#1528](https://github.com/kunwarshivam/mandate/issues/1528); DEC-2002, DEC-978 item 8, DEC-1025 |

### Context

The scenario's gestures ran in a fresh `demo<run id>` workspace (DEC-2002 item 5), founded for a
member of its own (item 8). The cell's API, and so `web/`, hosts one fixed workspace
(`01KDEM0000000000000000000W`, `api.workspace` in `deploy/cell/values/k3d.yaml`) and signs in the
stub issuer's `demo-owner` subject as another member. A step through the API or `web/` would act
on a workspace where the demo's agent was never deployed, by an owner who is not its member. A
fresh workspace per run is still needed, since a confirmed version refuses a second confirmation.

### Decision

1. **A ULID per run.** The run's workspace is `01KDEM`, the run id's ten digits, and the fixed tail
   `000000000D`: 26 Crockford base-32 characters, so the API, which hosts only a ULID, can host it,
   and never the chart's fixed workspace.
2. **Founded for the demo owner.** The founding member is the demo owner (`01KDEM…P`), the member
   the stub's `demo-owner` subject is linked to (DEC-1025), so the owner who signs in is the one
   the workspace was founded for. The CLI's owner gestures take its lower-case form.
3. **The API and the deployment manager are pointed at it.** Before the gestures, the scenario runs
   `helm upgrade --reuse-values --set api.workspace=<ulid>` on the workspace release, then waits
   on `kubectl rollout status` for the `api` and `deploy-manager` Deployments, so no pod of an
   earlier run still serves another workspace, and stops any earlier `kubectl port-forward` to the
   API, so no check reaches an old pod through a forward that still answers its 401. The chart is
   unchanged; `api.workspace` already names both processes' workspace. A cell without an API skips
   this.
4. **Step 1 checks what `main` serves.** A read of the run's workspace with no session must answer
   the one 401, and the demo owner's sign-in (a stub token traded at `POST /v1/sessions`) must
   answer 204 with the session cookie. The read with that session stays a gap: the API still wires
   `members::until_parser`, which fails every signed-in read and command closed with
   `membership_unavailable` until it reads memberships through `control_entry` (DEC-978 item 8).

### Consequences

- Each run adds one `helm upgrade` and two rollouts to `cell-demo`, which stays under its cap.
- After a scenario run the cell's API hosts that run's workspace; `cargo xtask cell demo` or
  `cell up` installs from the values file again and points it back.
- The API's membership read gates every signed-in step of the demo (steps 1, 2, 5 and 6 through
  the API or `web/`); the report names it as lane MB's wait (the membership parser) on each of them.

### Alternatives considered

- **One fixed workspace, reset each run:** the journal is append-only, and dropping a stream to
  reset it would be the wrong habit to show.
- **Leaving the API on its fixed workspace:** every API and `web/` step would read a workspace the
  gestures never touched, a partial story the report would have to explain on every step.

<a id="dec-2011"></a>

## DEC-2011: Hosted infrastructure is OpenTofu, DigitalOcean managed Kubernetes and GitOps, with staging and production, under a $150 monthly cap

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the founder, on 2026-10-18, in the managed-cells coordinator session: [DEC-2605](https://github.com/kunwarshivam/mandate/pull/1624) rows 1 to 4 (open in #1624) hold the founder's answers verbatim; items 1 to 4 below are lane IAC's dispatch as written |
| **Story or claim** | E21-56 (lane IAC), claim [#1634](https://github.com/kunwarshivam/mandate/issues/1634); lane IAC's readings of what it changes are [DEC-2012](infrastructure.md#dec-2012) items 1 and 2; [ADR-0006](../../adr/0006-hosted-infrastructure-as-code.md) |

### Decision

The founder's words, as relayed in lane IAC's dispatch:

1. Hosted infrastructure is OpenTofu + DigitalOcean managed Kubernetes (DOKS) + GitOps, with a
   staging and a production environment. Cloud tokens live in GitHub environments behind the
   founder's approval, so agents never see them. This replaces deploy/hosted/cell1.py's droplet
   bring-up (DEC-2001's droplets; amend DEC-2001 and DEC-2005/2006 where they conflict).
2. The monthly cloud budget cap is $150 for staging plus a small production, with alerts at 80%.
3. The target is staging up by the demo on 2026-10-26.
4. Separately, DEC-2103 item 7, passkey-only sign-in in v1, is accepted. Lane L1 records that one,
   not lane IAC.
5. The journal's backup archive (Postgres WAL and base backups) lives in "Cloudflare R2 + bucket
   lock" ([DEC-2605](https://github.com/kunwarshivam/mandate/pull/1624) row 4).

Item 4 is not lane IAC's, and [DEC-2605](https://github.com/kunwarshivam/mandate/pull/1624) records that the passkey-only answer it rests on was given to a
question put wrongly (its item 4), so it is not a decision. DEC-2103 item 7 is settled by
[DEC-2605](https://github.com/kunwarshivam/mandate/pull/1624) row 5 instead, for lane L1. Items 1 to 3 are [DEC-2605](https://github.com/kunwarshivam/mandate/pull/1624) rows 1 to 3.

### Rationale

Recorded as the founder gave it. Lane IAC's engineering readings of it, and what each one changes
in earlier decisions, are in [DEC-2012](infrastructure.md#dec-2012).

### Alternatives considered

The founder chose between the coordinator's options. This record does not reopen that choice.

<a id="dec-2012"></a>

## DEC-2012: how `deploy/tofu/` builds the hosted environments: modules per concern, a root per environment, encrypted state in Spaces, the journal archive in R2, and checks that need no credential

| | |
|---|---|
| **Status** | Accepted; its `nyc3` region (items 5 and 7 and DEC-995 item 2 as it cites them) superseded in part by [DEC-2047](infrastructure.md#dec-2047) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | an agent (Claude Code, lane IAC), under DEC-79: reversible engineering readings of [DEC-2011](infrastructure.md#dec-2011) and the accepted decisions it touches. None spends beyond DEC-2011's cap or weakens a safety rule. Item 9 carries out DEC-2011 item 5 ([DEC-2605](https://github.com/kunwarshivam/mandate/pull/1624) row 4), as [DEC-973](DEC-973.md) item 6 does |
| **Story or claim** | E21-56, claim [#1634](https://github.com/kunwarshivam/mandate/issues/1634); [ADR-0006](../../adr/0006-hosted-infrastructure-as-code.md); [DEC-995](DEC-995.md), [DEC-967](earlier-900-1999.md#dec-967) items 6, 8 and 9, [DEC-973](DEC-973.md), [DEC-1005](DEC-1005.md), [DEC-1050](earlier-900-1999.md#dec-1050), ES-12 |

### Context

DEC-2011 picks OpenTofu, DOKS and GitOps, two environments, and a $150 monthly cap with alerts at
80%, and keeps cloud tokens in GitHub environments the founder approves. Lane IAC builds
everything up to a reachable, empty cluster with its buckets, DNS and Access. Lane GO (E21-58)
installs the cluster's add-ons and the applications.

### Decision

1. **What DEC-2011 changes in earlier decisions.**
   - [DEC-995](DEC-995.md) item 5 says "no DigitalOcean managed database, managed Kubernetes or
     load balancer". Managed Kubernetes is now the platform.
   - DEC-995's other items stand where DEC-2011 does not replace them: New York (`nyc3`), no
     public port, and Cloudflare Tunnel and Access.
   - DEC-2001, DEC-2005 and DEC-2006 are open in #1539 and not on `main`.
     - DEC-2001's droplets, cloud-init, k3s units, and `cell1.py`'s `droplets` and `nodes` steps
       are replaced by `deploy/tofu/`.
     - DEC-2001's rules that do not depend on droplets carry over: no inbound port from the
       internet, ingress only through a Cloudflare Tunnel, Access in front of the API's hostname,
       and no secret in a committed file or on a command line.
     - DEC-2005's and DEC-2006's chart rules stand: one `deploymentTier`, and a synchronous
       standby with a WAL archive.
     - Lane H points those files' status lines at DEC-2011 when #1539 next merges `main`.
2. **Spend.**
   - DEC-2011 item 2's $150, for both environments together, replaces DEC-995 item 3's price.
   - A managed database and a load balancer stay out. The journal's Postgres is CloudNativePG in
     the cell (DEC-967 item 8), and the only ingress is the tunnel.
3. **Layout.** `deploy/tofu/modules/` holds one module per concern:
   - `network`: the VPC and the firewall;
   - `doks`: the cluster and its one node pool;
   - `spaces`: OpenTofu's state bucket only;
   - `cloudflare`: the tunnel, DNS, Access, and the journal archive's R2 bucket with its lock rule;
   - `budget`: the cost estimate and its cap;
   - `cell`: the DigitalOcean modules wired together.

   `deploy/tofu/environments/staging/` and `.../production/` are each a root module that calls
   them with its own values. So the two differ only in values, and a plan of one cannot touch the
   other.
4. **Pins.**
   - OpenTofu `1.13.1`.
   - Providers `digitalocean/digitalocean` `2.103.0` and `cloudflare/cloudflare` `5.27.0`, exact.
     Every module and root commits its own `.terraform.lock.hcl`, with the hashes for
     `linux_amd64` and `darwin_arm64`.
   - tflint `0.64.0`, with only its bundled `terraform` ruleset, so it downloads no plugin.

   Each was published at least two weeks before this decision. The `docs/dependencies.md` rows
   name the workflows and the install script that pin them; those pins land with
   [DEC-2013](infrastructure.md#dec-2013) and [DEC-2014](infrastructure.md#dec-2014), so the rows run ahead of them until then.
5. **State.**
   - Each environment's state is in its own Spaces bucket, `<prefix>-tofu-state-<env>`, through
     the `s3` backend.
   - The founder makes that bucket and the Spaces key once, by hand: OpenTofu cannot keep its state
     in a bucket it has not made yet, and agents hold no credential. Each root then adopts the
     bucket with an `import` block, and keeps it private and versioned with `prevent_destroy`.
   - State and plans are encrypted on the client with OpenTofu's own state encryption, `enforced`:
     - the `pbkdf2` key provider, set explicitly to SHA-512, 600,000 iterations, a 32-byte salt
       and a 32-byte key;
     - the `aes_gcm` method;
     - a passphrase of at least 32 characters, which lives only in the environment secret
       `TOFU_STATE_PASSPHRASE`.
   - Runs are serialised by the workflow's `concurrency` group per environment, not by a lock
     file, since Spaces' support for the conditional writes that OpenTofu's S3 lock needs is not
     documented.
   - **The state bucket holds OpenTofu's state and nothing else.** No journal spec §6.2 object
     (segments, manifests, artifacts, configuration objects, timestamp tokens, verifier releases)
     and no backup is ever written to it.
6. **Network.**
   - One VPC per environment in `nyc3` (DEC-995 item 2), with its own range.
   - One cloud firewall on the cluster's node tag, whose inbound rules admit only the VPC's own
     range.
   - DigitalOcean firewalls add up rather than narrow each other, so this firewall cannot close
     what DOKS's own managed worker firewall opens. The guarantee is that:
     - nothing in `deploy/tofu/` opens a public port;
     - no `digitalocean_loadbalancer` exists;
     - lane GO creates no Service of type `LoadBalancer` or `NodePort`, and the chart already
       creates none (DEC-967 item 6).
7. **The cluster.** One DOKS cluster per environment:
   - The Kubernetes version is pinned exactly. A precondition fails the plan when the pin is not
     one DigitalOcean offers, and lists the versions it does.
   - `auto_upgrade` off, `surge_upgrade` on, no HA control plane, no registry integration.
   - One node pool of exactly two nodes, `s-2vcpu-4gb` by default, autoscaling off.
   - The control plane's firewall is on. It admits only the CIDRs the founder sets in the GitHub
     environment's `KUBE_API_ALLOWED_CIDRS`. The variable's validation refuses an empty list and
     any range wider than /24 (IPv4) or /48 (IPv6), so every world range too.
8. **Ingress.**
   - A remotely managed Cloudflare Tunnel per environment. Its token becomes the chart's
     `cloudflared-tunnel` Secret.
   - One proxied DNS record for the API's hostname, pointing at the tunnel.
   - One Access application on that hostname, whose policy allows only the full emails in
     `ACCESS_ALLOWED_EMAILS`. *Amended by [DEC-2030](infrastructure.md#dec-2030): staging admits the Cloudflare
     account's members instead; production keeps its named emails, now an environment secret.*
   - The tunnel's ingress maps that hostname to the API's origin, and everything else to
     `http_status:404`.
9. **The journal archive is R2 with a bucket lock rule** ([DEC-973](DEC-973.md) item 6, Accepted:
   "R2 stays the endpoint for what §6.2 does not cover: the journal Postgres's WAL archive and base
   backups").
   - The `cloudflare` module makes one R2 bucket per environment,
     `<prefix>-journal-archive-<env>`, with location hint `enam` (DEC-967 item 9).
   - It gives the bucket one enabled lock rule over every key: objects cannot be deleted or
     overwritten before an age of at least 30 days. 30 days is the journal-db chart's
     `archive.retention`, so Barman can still drop what its own retention drops.
   - R2's lock is governance-equivalent: a token that can edit the bucket's configuration can
     lift it (DEC-973 item 5). That is acceptable here only because §6.2 does not cover the WAL
     archive (DEC-973 item 6). Nothing §6.2 lists is written to this bucket.
   - Barman's S3 credential is an R2 API token the founder scopes to this bucket alone, with
     Object Read and Write. It is made by hand and never enters OpenTofu's state. *Amended by
     [DEC-2029](infrastructure.md#dec-2029): OpenTofu mints it, and it lives in encrypted state.*
   - The placement is the founder's: [DEC-2605](https://github.com/kunwarshivam/mandate/pull/1624) row 4, "Cloudflare R2 + bucket lock (Recommended)",
     recorded as DEC-2011 item 5. DEC-973 item 6 says the same.
10. **Secrets and the route into the cluster.**
    - The tunnel token is a `sensitive` output. The workflow runs no `tofu output` and prints no
      output.
    - The only secrets OpenTofu's state holds are the tunnel's token and secret, and state is
      encrypted (item 5). *Amended by [DEC-2029](infrastructure.md#dec-2029): it also holds Barman's minted R2
      token.*
    - Getting the tunnel token and Barman's R2 token into the cluster is lane GO's, agreed with
      lane IAC. Either the founder creates them as Kubernetes Secrets at the one-time Flux
      bootstrap, or they are committed only as SOPS-encrypted files whose age key only the
      cluster holds. They are never plaintext in git, a workflow log, or an issue.
11. **GitOps is pull-based.**
    - After the founder's one-time Flux bootstrap, run from an address in
      `KUBE_API_ALLOWED_CIDRS`, the cluster pulls its desired state from the repository.
    - Neither CI nor `apply` needs the Kubernetes API. `apply` reaches only DigitalOcean's API
      (Spaces included) and Cloudflare's.
12. **The budget.**
    - No DigitalOcean API sets a billing alert. So DEC-2011 item 2's 80% alert ($120) is a manual
      step the founder takes in the billing settings, named in the runbook.
    - In code, the `budget` module adds up each environment's monthly estimate:
      - the node size's price, as DigitalOcean's size list reports it, times two nodes;
      - Spaces' $5 subscription, counted in both environments, though it is per account, so
        neither undercounts;
      - block storage at $0.10 per GB.
    - The estimates come to:
      - staging: $57 ($48 of nodes, $5 of Spaces and $4 for 40 GB);
      - production: $55 ($48, $5 and $2 for 20 GB).
    - R2's storage at this size is within its free tier.
    - The plan refuses an estimate over **$65 per environment**. That keeps both together at or
      under $130, and leaves at least $20 under the cap for what the estimate does not count:
      bandwidth over the included transfer, snapshots, and egress.
13. **Checks without credentials.** Every pull request that touches `deploy/tofu/` runs:
    - `tofu fmt -check`;
    - in each root, `tofu init -backend=false` and `tofu validate`;
    - tflint;
    - `tofu test` with mocked providers, as the policy check.

    The policy tests cover:
    - no public firewall source and no load balancer;
    - the state bucket private and versioned;
    - the archive bucket's lock covering every key for at least 30 days;
    - auto-upgrade off, the pinned version, two nodes, and the control plane's firewall on;
    - the DNS record proxied, and Access in front of the API host;
    - the estimate under the cap.

    Each refusal also has a test showing it fails. None of these reads a secret or reaches a cloud
    API. `infra.yml` alone runs `plan` and `apply` ([DEC-2014](infrastructure.md#dec-2014)).

### Rationale

A root per environment is the simplest layout in which a staging plan provably cannot change
production, and modules keep the two from drifting. Client-side encryption keeps the tunnel token
out of readable state without another service. Keeping Barman's credential out of OpenTofu
altogether means the state never holds a key that can write the archive. Testing the plan against
mocked providers checks the same resources `apply` would create, with no credential, which is what
ES-12 requires of a pull-request job.

### Alternatives considered

- **Workspaces in one root**, instead of a directory per environment: one backend and one set of
  variables for both, so a mistyped workspace plans production.
- **Terraform:** its licence is BUSL since 1.6, and DEC-2011 names OpenTofu.
- **The journal archive in Spaces:** it has no object lock at all, and DEC-973 item 6 already puts
  the archive on R2.
- **A Barman key made by OpenTofu:** its secret would sit in state, and the Cloudflare token would
  need the right to create API tokens.
- **A separate policy engine (OPA/conftest, checkov) over a plan's JSON:** a new tool, and a plan
  needs credentials. `tofu test` with mocks needs neither.
- **The state bucket made by a bootstrap root with local state:** that state would live on a CI
  runner and be lost, or need its own bucket.
- **Isolated workers and a NAT gateway**, so nodes have no public address: the gateway's price
  would take a large part of the cap. Recorded as a later hardening.

<a id="dec-2013"></a>

## DEC-2013: `deploy/tofu/`'s pull-request checks run as `full-checks`' `tofu` part, selected by path, with no credential

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | an agent (Claude Code, lane IAC), under DEC-79 (reversible engineering and process choices) and DEC-176 (item 4 admits one program to the closed world under rules as tight as `tar`'s, and loosens nothing else) |
| **Story or claim** | E21-56, claim [#1634](https://github.com/kunwarshivam/mandate/issues/1634); [DEC-2012](infrastructure.md#dec-2012) item 8; DEC-1190; DEC-873; DEC-464; ES-12 |

### Context

DEC-2012 item 8 asks every pull request that touches `deploy/tofu/` to run `tofu fmt -check`,
`tofu validate`, tflint and `tofu test` against mocked providers, with no credential, and DEC-1190
asks every per-PR job to follow the diff. The workflows' closed world (DEC-873) admits only the
programs it lists in a workflow's runs, and `tofu` and `tflint` are not among them.

### Decision

1. **A `tofu` part of `cargo xtask ci full`.** It runs `tofu fmt -check -recursive`, then tflint
   over `deploy/tofu/` with `.tflint.hcl`, then in each directory under `deploy/tofu/modules/` and
   `deploy/tofu/environments/`: `tofu init -backend=false -lockfile=readonly`, `tofu validate`
   and `tofu test`. The working directories and provider cache are under `target/tofu`. A
   directory there with no `.tf` file fails the part, so no root is skipped unnoticed. The tools
   are run by xtask, a cargo word the closed world already reads.
2. **Selected by path.** `Selection` gains `tofu`: on when a changed path under `deploy/tofu/` is
   not documentation, and whenever everything is selected. `full-checks` already runs for any
   `deploy/` path, so no new job, output or aggregate is added. The selection's reasons line says
   whether `tofu` runs.
3. **Required in CI, optional locally.** `full-checks` sets `MANDATE_TOFU_REQUIRED`, so a missing
   tool fails there. Locally, without `tofu` or `tflint`, the part says so and skips, as the
   Postgres and object-store parts do. `.cursor/install.sh` installs both at the versions in
   `ci.yml` (OpenTofu from its tar.gz, tflint from its zip through Python's `zipfile`). `tofu test`
   fetches the pinned providers from `registry.opentofu.org`, checked against each lock file.
4. **`unzip` in the closed world, shaped like `tar`.** tflint is published only as a zip, and no
   pinned action installs it. A workflow's `unzip` is read only with no flag, an archive, and at
   least one named member, every word a plain local name that is no build input: the same rule
   as `tar`'s members. So it can write nothing a cargo build reads. The closed-world test refuses
   `unzip` with a flag (`-d crates`), with no member, with a build input (`Cargo.toml`), with a
   path, and with a hidden name.
5. **No credential.** The part reads no secret, and its tests' mocked providers make no API call.
   `plan` and `apply` belong to `infra.yml` alone (E21-56's next slice, under the environments
   `infra-staging` and `infra-production`).

### Rationale

Folding the checks into `full-checks` keeps the required checks as they are (DEC-464, DEC-1190):
the part adds about a minute, mostly the providers' download, and only to a diff that touches
`deploy/tofu/`. Running the tools through xtask keeps the closed world's program list as it was,
apart from one extraction program bounded like the one it already reads.

### Alternatives considered

- **A separate `tofu` job:** another selection output, another aggregate input, and another
  runner for about a minute of work.
- **An action that installs tflint:** none of the pinned actions does. A new action needs its own
  review and pin.
- **Downloading the tools inside xtask:** it would move a tool install, which every other job does
  in a reviewed workflow step, into code.

<a id="dec-2014"></a>

## DEC-2014: `infra.yml` alone plans and applies, from `main`, on dispatch, under an environment the founder approves, and applies only the plan it showed

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | an agent (Claude Code, lane IAC), under DEC-79: reversible engineering choices that carry out [DEC-2011](infrastructure.md#dec-2011) item 1 ("cloud tokens live in GitHub environments behind the founder's approval") on the pattern of [DEC-1005](DEC-1005.md); none spends or loosens a rule |
| **Story or claim** | E21-56, claim [#1634](https://github.com/kunwarshivam/mandate/issues/1634); [DEC-2012](infrastructure.md#dec-2012) items 3 and 8; [DEC-2013](infrastructure.md#dec-2013); ES-12; AGENTS.md rules 7 and 8 |

### Decision

1. **One workflow, one job.** `.github/workflows/infra.yml` runs on `workflow_dispatch` alone. Its
   inputs are an `environment` (`staging` or `production`) and an `action` (`plan` or `apply`).
   It grants nothing at the top. Its one job, `tofu`, runs only when `github.ref` is
   `refs/heads/main`, under `environment: infra-${{ inputs.environment }}`, reads contents only,
   and checks out with no credential kept. `concurrency` serialises runs per environment, which
   stands in for a state lock (DEC-2012 item 3).
2. **The founder approves every run.** The environments `infra-staging` and `infra-production` are
   restricted to `main` and have the founder as their required reviewer. GitHub hands their
   secrets to a job only after that approval. No pull request, and no agent, can start or approve
   one. The founder creates them (the runbook in `deploy/tofu/README.md`).
3. **Secrets reach one step.** The job installs OpenTofu by checksum and builds xtask in steps
   that hold no secret, so no build script sees one. Only its last step, `cargo xtask infra`,
   receives the secrets:
   - `DIGITAL_OCEAN_API_KEY`, as the `DIGITALOCEAN_TOKEN` the provider reads. The name is the
     secret the founder created in `infra-staging`.
   - `SPACES_ACCESS_KEY_ID` and `SPACES_SECRET_ACCESS_KEY`, as themselves and as the `AWS_*` names
     the `s3` backend reads.
   - `TOFU_STATE_PASSPHRASE`.
   - `CF_INFRA_API_TOKEN`.

   It also receives the variable `KUBE_API_ALLOWED_CIDRS` and, for production, the secret
   `ACCESS_ALLOWED_EMAILS`, as `TF_VAR_*`; the account id is read from Cloudflare instead
   ([DEC-2030](infrastructure.md#dec-2030) items 1 and 2). The Cloudflare names avoid the `CLOUDFLARE_` prefix,
   which `web/scripts/workers.mjs` keeps to `web-deploy.yml`'s deploy step (DEC-1005 item 2).
4. **Apply applies what it showed.** `cargo xtask infra` refuses to run outside GitHub Actions on
   `main`. It runs `tofu init` against the remote state with `-lockfile=readonly`, then
   `tofu plan -out`, which saves the plan encrypted, and prints the plan's changes by address and
   action (below). For `apply`, it then runs `tofu apply` on that saved file, never
   `-auto-approve` on a fresh plan, so the log shows exactly what was applied. The founder's way of working: dispatch `plan` and read it, then
   dispatch `apply` on the same commit and approve it straight away. Anything that changed in the
   cloud between the two runs is in the apply run's printed plan, which the founder reads
   afterwards.

   The public log carries no value, since the repository is public and so are its Actions logs.
   Every `tofu` command runs with `-no-color` and its output captured; `plan` and `apply` also take
   `-compact-warnings`. The log gets each command line, a failure's diagnostics (OpenTofu redacts
   every sensitive value in them), and after `plan` one line per resource change, its actions and
   address, read from `tofu show -json` as untyped JSON. No diff, no `Outputs:` block and no plan
   file reaches the log or an artifact. The rest of the coordinator's audit (sensitive inputs, the
   providers' own flags, masks for fetched values, the built binary in the secret step, and a
   reviewed-commit input) lands with E21-56's later slice, #1711.
5. **The shape is checked.** `cargo xtask lint` refuses an `infra.yml` that loses any of items 1
   to 3: another trigger, the job off `main`, another environment, a kept checkout credential, a
   second job, another input option, a secret outside the last step or not on the list, or a step
   after it. It also refuses any other workflow that reads one of those secrets or names
   `infra-staging` or `infra-production`.
6. **No hidden code or host in `deploy/tofu/`: a raw-text scan.** The `tofu` part of `cargo
   xtask ci full` reads every `.tf`, `.tofu` and `.hcl` file (OpenTofu's own lock files aside) as
   raw text, before any tool runs and whether or not the tools are installed. It refuses, as whole
   words and in any case, on any line:
   - `provisioner`;
   - `external`;
   - `http` unless `://` follows at once;
   - `source` on any line other than exactly `source = "<value>"`, where the value is a pinned
     provider (`digitalocean/digitalocean`, `cloudflare/cloudflare`), a sibling module
     (`../<name>`), or a module of `deploy/tofu/modules` (`../../modules/<name>`).

   The name of any of OpenTofu's file-reading functions (`file`, `templatefile`, `filebase64` and
   the rest) and the word `terraform_remote_state` are refused too, as whole words wherever they
   appear, since either would load configuration or state the scan never reads, and HCL still
   calls a function across a line break or a comment before its `(`. Prose that needs the word is
   reworded.
   Any JSON file and any symlink under `deploy/tofu/` are refused outright, and so is any line
   holding a backslash, since an escape (`\u0065`) is decoded by OpenTofu and not by the scan.
   Comments, strings and heredocs count like code.
7. **Secrets and environments only on reviewed lines: a raw-text scan.** `cargo xtask lint` refuses
   any workflow line that names the word `secrets` or `environment`, in any case and comments
   included, unless that exact trimmed line is listed for that file in xtask's
   `GUARDED_WORKFLOW_LINES`. It also refuses the word `inherit` on any line, and a backslash on
   any line at all, since YAML's double-quoted escapes (`\x73ecrets`) and line continuations are
   decoded by GitHub and not by the scan, and a multi-line double-quoted scalar can carry one on a
   line holding no quote and no `${{`. The only exceptions are the exact lines listed in
   `BACKSLASH_WORKFLOW_LINES` (six reviewed shell lines today). A new
   secret, a new environment, or a reworded comment is a reviewed change to those lists. An
   allowed line is bound to its file, not to a job: the same line in another job of that file
   passes the word scan, and only `infra_workflow_problems`' shape check keeps `infra.yml`'s
   secrets to its one step.

### Why items 6 and 7 read raw text

Three review rounds each found a new way past a scan that parsed: indexes and whole-context
functions, multi-line expressions, quoted and flow `inherit`, `#` inside a `run: |` block, a bare
`if:`, block-scalar and flow `environment` values, HCL's `$${` escape, unquoted labels, and
comments. A parser that is not GitHub's or OpenTofu's own will always differ from theirs
somewhere, and each difference is a bypass. A raw-text scan has far less to disagree with: a
construct is refused wherever its word appears, in code, strings or comments, and the one
character that could spell a word without writing it, the backslash, is refused on every line of
every workflow and every file under `deploy/tofu/` but the reviewed ones. The fourth round found
the last place a backslash had still been allowed: a middle line of a multi-line double-quoted
scalar, with no quote and no `${{` on it. With every backslash refused, a word can reach GitHub or
OpenTofu only spelled out, where the scan sees it. That rests on YAML and HCL having no escape
but the backslash in the places a word could hide, which is what the check assumes and the tests
exercise; a new encoding would be closed the same way, by refusing the character that carries
it. Its false refusals cost a rewording or a reviewed allowlist line, and they fail closed. The committed tree was
reworded where it named these words in prose (the network module's comments and its test's loop
variable), so the allowlists hold only lines that must say the word.

### Rationale

DEC-1005 already holds the web deploy's token this way, and the founder asked for the same.
Building before the secrets arrive, and applying a saved plan, close the two gaps a single
`tofu apply` step would leave: a dependency's build script with the cloud token in its
environment, and an apply that differs from the plan the founder read.

### Alternatives considered

- **A plan job and an apply job passing the plan as an artifact:** two approvals per apply, and an
  artifact download action that is not on the pinned list. The saved plan in one job gives the
  same guarantee.
- **Plan on pull requests with read-only tokens:** any token on a pull-request job breaks ES-12.
- **A push to `main` that applies:** the founder asked for approval of each apply, not for merge
  to equal apply.

<a id="dec-2015"></a>

## DEC-2015: Flux reconciles the hosted clusters from `main`, and the founder creates the workload secrets at bootstrap until the vault

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79: a reversible engineering choice. It spends nothing beyond the founder's DEC-2011 budget, adds no live path, and weakens no safety rule |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); the founder's DEC-2011 (OpenTofu, DigitalOcean DOKS, GitOps, staging and production; DEC-2605 rows 1 to 4); lane IAC's handoff from #1639; infrastructure design §7.1; DEC-971 item 6; DEC-822 item 4 |

### Context

DEC-2011 puts staging and production on DOKS, deployed by GitOps. Lane IAC (E21-56) delivers an
empty cluster, the journal archive bucket (Cloudflare R2 with bucket lock, DEC-2605 row 4), DNS,
and the Cloudflare Tunnel and Access application. Its OpenTofu outputs the cluster's id and
endpoint, the tunnel token, and the archive bucket and key. The cluster's control-plane firewall
admits only the founder's addresses (`KUBE_API_ALLOWED_CIDRS`), so no CI runner can reach the
Kubernetes API. Everything on the cluster comes from this
repository: the CloudNativePG operator and its Barman Cloud plugin, the journal's Postgres
(`deploy/cell/charts/journal-db`), the plugin's certificates (`backup-plugin-tls`), and the
workspace chart with its cloudflared ingress. It is the same set `cargo xtask cell up` installs in
the local cell.

Two properties of those charts decide how they can be reconciled:

- `journal-db/templates/roles.yaml` and `backup-plugin-tls/templates/tls.yaml` call Helm's
  `lookup` to keep each password and certificate across upgrades. `journal-db`'s egress reads the
  `kubernetes` Service and EndpointSlice the same way.
- `workspace/templates/migrate.yaml` runs the migrations as a `pre-install,pre-upgrade` hook Job
  before any process rolls (OPS-13).

### Decision

1. **Flux, not Argo CD.** Each hosted cluster runs Flux v2.9.6's source-controller,
   kustomize-controller and helm-controller, and nothing else of Flux. The manifests come from
   `flux install --export` for that version, with each controller's image pinned by digest. No UI,
   no API server, and no notification, image-reflector or image-automation controller run.
   Promotion is a pull request (DEC-2016), so no controller in the cluster needs to write to git.
2. **Read-only, from `main`.** Flux reads this public repository anonymously over HTTPS, on
   `main` only. The cluster holds no git credential and cannot push. A cluster reconciles only its
   own directory, `deploy/gitops/clusters/<environment>` (`staging` or `production`). Nothing
   deploys from a branch (DEC-971 item 6).
3. **Charts are reused, never forked, and pulled by digest.** Every chart reaches the cluster as
   an OCI artifact pinned by manifest digest, never as a chart directory read from git.
   source-controller's git artifacts keep only regular files and directories
   (`fluxcd/pkg/artifact` v0.18.3, `storage/archive.go`), so it would drop the symlinks
   `journal-db/files/processes.json` and `workspace/files/migrations`, and both charts would
   install broken.
   - `deploy/cell/charts/*` are packaged by `helm package`, which reads through those symlinks. On a
     push to `main`, `release.yml`'s `publish-index` job pushes them to GHCR and signs each digest
     keyless with its own OIDC identity, the images' identity (E21-58 slice B1). Flux checks that
     signature with its cosign verification before it uses the chart.
   - The operator's and the plugin's upstream charts come from GHCR the same way. Each
     manifest's chart layer is the tarball whose SHA-256 `xtask/src/cell.rs` already pins
     (`cloudnative-pg` 0.29.1, `plugin-barman-cloud` 0.8.1).
   - Values come, in order, from the chart's own `values.yaml`, from
     `deploy/cell/values/managed.yaml` (generated into a ConfigMap from the file itself, not
     copied), from `deploy/hosted/values/` where a file applies, and last from the overlay. The
     overlay sets only the environment's chart and image digests, tier, archive destination and
     sizes.
4. **Interim workload secrets: none in git, created by the founder at bootstrap.** The charts take
   every secret by name and never as a value (DEC-822 item 4). The Secrets they name, which the
   cluster cannot make itself, are the archive's key (`journal-archive-key`) and the tunnel's token
   (`cloudflared-tunnel`). The founder creates them in the one bootstrap (DEC-2018), from IAC's
   OpenTofu outputs, on a machine whose address the control-plane firewall admits.
   - Flux neither creates nor prunes those Secrets.
   - No plaintext, no encrypted secret, and no key to one is ever committed.
   - No CI job holds a Kubernetes credential. The firewall would refuse one anyway, and Flux's
     pull model needs none.
   - The `infra-staging` GitHub environment, which holds staging's cloud credentials (DEC-2605
     rows 1, 4 and 10), stays IAC's and V2's. GitOps adds nothing to it.

   This holds until lane V2's vault (E21-32) serves workload secrets. Moving to the vault is then
   its own decision.
5. **Agents never apply.** Agents never run the bootstrap, a `kubectl` write, or a `flux` command
   against a real cluster, and they hold no kubeconfig. After the founder's one bootstrap,
   every change reaches the cluster as a commit to `main` that Flux pulls.

### Rationale

- **Helm semantics.** Argo CD renders a chart with `helm template` and applies the result, so
  `lookup` returns nothing. Every sync would then make new journal role passwords and new plugin
  certificates, and the egress rule to the API server would render empty. Argo CD also maps Helm
  hooks onto its own sync phases, approximately. helm-controller runs real Helm installs and
  upgrades, with `lookup` against the cluster and hooks as written, as `cargo xtask cell up` does.
  The local cell's drills therefore prove the same install path the hosted cells take.
- **Small cluster, small surface.** The three controllers request well under 1 GiB together.
  Argo CD adds a repo server, Redis, an API server and a UI, and the UI would need another
  Cloudflare Access application to expose safely. Under the $150 monthly cap, that memory is
  better spent on the journal's two instances.
- **No write path.** Promotion by pull request leaves the cluster with read access only, so
  nothing that compromises the cluster can change what `main` deploys.
- **Secrets.** A Secret the founder creates at bootstrap needs no new controller, puts no
  ciphertext in git, and needs no CI job to reach the Kubernetes API, which IAC's firewall
  refuses. The plaintext stays in IAC's OpenTofu state and the cluster.

### Alternatives considered

- **Argo CD:** rejected for the `lookup` and hook semantics above, and for its footprint and UI.
- **`flux bootstrap github`:** it creates a deploy key with write access and commits Flux's own
  manifests through a token. A read-only anonymous source needs neither (item 2).
- **SOPS with an age key, or Sealed Secrets:** each puts ciphertext in git. Someone must still
  hold the plaintext to encrypt it, and the cluster must hold the decryption key, so the
  founder's step remains with more parts. Sealed Secrets also adds a controller. IAC's handoff
  allows SOPS; it adds nothing here that the bootstrap does not already do.
- **External Secrets Operator:** it needs a backing store, and that store is the vault E21-32
  brings. It can be reconsidered with the vault.

<a id="dec-2016"></a>

## DEC-2016: the hosted clusters check release signatures at admission without blocking a brake, and production runs only what staging ran

| | |
|---|---|
| **Status** | Accepted; item 2 superseded in part by [DEC-2045](infrastructure.md#dec-2045), and item 5 by [DEC-2048](infrastructure.md#dec-2048) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79 and DEC-176, item 2 at the coordinator's review of #1797 (fail closed, with a tight exemption for the kill switch). Items 1, 3, 4 and 5 add checks, and item 2 keeps `AGENTS.md` rule 13 whole, so none weakens a safety rule |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); E21-35 (signed images, the start-up check); DEC-971 items 6 and 7; DEC-2015; DEC-2017 |

### Context

`release.yml` publishes, from `main` only, each binary's multi-arch image index and the hosted
clusters' charts, each pinned by digest and signed keyless by `release.yml`'s OIDC identity on
`refs/heads/main`. Flux already refuses a chart without that signature (DEC-2015 item 3). Nothing
yet checks the images a pod in the workspace runs, and nothing moves an environment to new
digests. DEC-2015 ruled out a write path from the cluster to git, so Flux's image automation, which
commits from inside the cluster, is out.

### Decision

1. **Signatures at admission.** Each cluster runs sigstore's `policy-controller` (chart 0.10.8 by
   OCI digest, every image by digest), with one `ClusterImagePolicy` in
   `deploy/gitops/policies/`, which its own Kustomization applies once the controller's CRD exists.
   Any image under `ghcr.io/kunwarshivam/mandate/` must carry a keyless signature whose issuer is
   GitHub Actions and whose subject is `release.yml` on `refs/heads/main`, logged in Rekor. Only
   namespaces labelled `policy.sigstore.dev/include: "true"` are checked: the workspace's,
   `ws-ws1`. An image from elsewhere (CloudNativePG, cloudflared) is allowed, and the overlay
   checks keep it pinned by digest (DEC-2017).
2. **The webhooks fail closed, except for the kill switch, and only the Job controller can claim
   that exception.** Both pod webhooks (`policy.sigstore.dev`, mutating and validating) have
   `failurePolicy: Fail`, so while the controller is down no pod in `ws-ws1` is admitted unchecked.
   The one exception is a pod labelled `mandate.dev/process: control`, the Job that commits a kill
   switch, which `AGENTS.md` rule 13 requires to be always available: a Flux post-render patch adds
   an `objectSelector` that excludes exactly that label from both webhooks (the chart has no value
   for it; policy-controller's reconciler rewrites a webhook's rules and namespace selector but
   keeps its object selector). Neither a webhook outage nor an unreachable registry or Rekor can
   then refuse the kill switch. So that the label is not a way around the check,
   `base/kill-switch-admission.yaml` adds two `ValidatingAdmissionPolicy` objects, which the API
   server evaluates itself, with no webhook to go down:
   - in `ws-ws1`, only the Job controller (`system:serviceaccount:kube-system:job-controller`, or
     `system:kube-controller-manager` where the controllers share one identity) may create a pod
     with that label or add it to one;
   - no service account may create a Job whose pods carry it. The founder's CLI creates the kill
     switch's Job as a person (E21-34), so a workload in the namespace, such as the deployment
     manager, cannot mint one. A later kill switch triggered by a service needs its own decision.

   The exempt pod still runs only the chart's digest-pinned image, and the start-up signature check
   (E21-35, OPS-15) still refuses, in `staging` and `production`, any binary without the release
   key's signature; until the release keys are wired (DEC-971 item 7), it starts no process there
   at all. A drill in the local cell (follow-up) proves it live: with the webhook down, the kill
   switch's Job is admitted, an unlabelled pod with an unsigned image is refused, and a pod
   labelled `control` by anyone but the Job controller is refused.
3. **Checked in CI.** `deploy/tests/test_gitops.py` (DEC-2017 item 3) refuses a policy that covers
   other images, trusts another identity or drops the Rekor log, a second policy, an unlabelled
   workspace namespace, a webhook that fails open or denies unmatched images, an exemption wider
   or other than the control label, admission policies that match other objects, allow other
   creators, fail open or only warn, and a controller chart or image not pinned by digest. Each is
   shown failing on a seeded bug.

4. **Promotion is a command, and its output is a pull request.** `cargo xtask gitops promote
   staging <dir>` reads a `release.yml` run on `main`'s `CHARTS` and `PUBLISHED` from `<dir>` and
   rewrites staging's `charts.yaml` digests and `images.yaml`. It refuses a line that is not this
   repository's, not pinned by SHA-256, listed twice or missing, and it rewrites only the charts
   the overlay pins, so a published chart the environment does not run yet (the vault's) waits for
   its own change. A binary fills a chart `images` key only through `CHART_IMAGE_FOR_BINARY` in
   `xtask/src/promote.rs`; today that is `mandate`, and the rehearsal simulator is never charted.
   `images.yaml` keeps the environment's tier. The change reaches `main` as an ordinary pull
   request, reviewed and merged like code, and Flux rolls the environment from `main`. A workflow
   that opens staging's pull request after each release run on `main` is a later change.
5. **Production copies staging.** `cargo xtask gitops promote production` copies staging's two
   files' digests as they stand. `deploy/tests/test_gitops.py` refuses any digest production pins
   that staging's files never held on `main` at the pull request's merge base, so a pull request
   cannot add a digest to both environments at once and pass.

### Rationale

A pull request is the only write path, so every staging change is visible, checked by CI and
merged by the same rules as code, and production can be audited against staging from git alone.
Admission verifies the publisher's identity; the start-up check verifies the release key. Neither
depends on the other. The kill switch never waits on a webhook, and nothing else gets past one:
the only exemption is a label that only the Job controller, acting for a person's Job, can set.

### Alternatives considered

- **Flux image automation:** it needs a git credential with write access in the cluster, which
  DEC-2015 item 2 refuses.
- **Promoting production from a release run:** it would let a digest reach production without
  staging running it first (item 5).

- **Kyverno's `verifyImages`:** a heavier controller with a broader surface, for the one rule
  needed here.
- **`failurePolicy: Ignore`:** while the controller is down, any image would be admitted to the
  workspace, the window an attacker would aim for.
- **`failurePolicy: Fail` with no exemption:** a controller, registry or Rekor outage would refuse
  the kill switch's Job, breaking rule 13.
- **An exemption by label alone:** anyone who can create a pod in the namespace could set the
  label; the admission policies close that.
- **The kill switch in a namespace of its own:** its Job needs the workspace's journal credentials
  and network policies, which live in `ws-ws1`.
- **Every namespace checked:** CloudNativePG's and cloudflared's images carry no `release.yml`
  signature; matching only this repository's images in the workspace's namespace checks what can
  carry one.

<a id="dec-2017"></a>

## DEC-2017: the journal reaches its R2 archive only at R2's two addresses, and every overlay is checked without a credential

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79 and DEC-176. Item 1 takes the narrowest egress that works and fails closed; items 3 to 5 add checks. None weakens a safety rule |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); the founder's choice of Cloudflare R2 with bucket lock for the journal's archive (DEC-2605 row 4); E21-41, DEC-1050; DEC-1190; DEC-2005 |

### Context

The journal-db chart lets the archive's clients (the instances' Barman Cloud sidecars) reach the
archive endpoint only through `archive.egress.cidrs`, each /24 or narrower, on one port. The
founder put the hosted archive in Cloudflare R2 with bucket lock. R2's S3 endpoint,
`<account>.r2.cloudflarestorage.com`, sits on Cloudflare's anycast network, whose published ranges
are /12 to /20 blocks shared by every site behind Cloudflare. Those are wider than the chart
allows, and they would open the journal's pods to any of those sites.

### Decision

1. **R2's two addresses, each /32, on 443.** On 2026-10-18, every account's endpoint in R2's
   default jurisdiction resolved to the same two addresses, `172.64.190.1` and `172.64.66.1`. They
   are R2's own, not Cloudflare's whole network. The hosted overlays allow exactly these two /32s
   on TCP 443, and nothing else, to the archive's clients. The bucket must be in the default
   jurisdiction; the EU jurisdiction resolves elsewhere (`141.101.90.96/30`) and would need its own
   reviewed change.
2. **It fails closed.** If Cloudflare moves R2's addresses, archiving stops and nothing opens
   wider. CloudNativePG's `ContinuousArchiving` condition goes false, the standby still holds every
   commit (OPS-2), and a reviewed pull request updates the two /32s. No proxy, wildcard, or DNS
   rule is added.
3. **Every overlay is built and checked in CI with no credential.** `deploy/tests/test_gitops.py`
   runs in the required `fast-python` job. `deploy/` already selects that job (DEC-1190), and CI
   now installs flux 2.9.6 and kubeconform there, each checked by SHA-256, and sets
   `MANDATE_GITOPS_REQUIRED`, so a missing tool fails the job rather than skipping the checks.
   Other jobs that run `deploy/tests` without flux, such as `cell`, skip this file only. It checks every
   environment's overlay under `deploy/gitops/clusters/`, and a fixture of staging's under
   `deploy/tests/fixtures/gitops/`, with made-up chart digests and R2 account, which also takes
   the seeded bugs; the checks therefore run before the first real overlay lands, and keep their
   seeded bugs whatever the real digests are. For each overlay it:
   - builds the overlay with `flux build --dry-run`, exactly as kustomize-controller would;
   - merges each HelmRelease's values as helm-controller does;
   - renders each of the repository's own charts from `deploy/cell/charts/`;
   - checks the renders with kubeconform against Kubernetes 1.35.
4. **What the checks refuse.**
   - A chart or Flux source not pinned by digest.
   - One of the repository's charts without Flux's cosign verification of `release.yml`'s
     keyless signature on `main`.
   - An upstream chart other than the pinned one.
   - A chart read from git.
   - A git source other than this repository's `main`, or one holding a credential.
   - A cluster reconciling another environment's directory.
   - A namespace or the journal that Flux could prune.
   - The stub tiers `dev` or `ci` (DEC-2005), or another environment's tier.
   - `live` anywhere in the values or the rendered environment.
   - Local images, or the stub issuer.
   - Any image not pinned by digest, in the values or in what the charts render.
   - A journal not on two instances in separate failure domains.
   - A journal archiving anywhere but R2's S3 endpoint, under another environment's bucket, or
     through any egress but item 1's.

   Each check is shown failing on a seeded bug, and the expected pins and signer are written in
   the test, not read from the overlays. The admission policy's checks come with it (DEC-2016).
5. **The checks cover the CRDs the schemas lack by fields.** kubeconform has no schema for Flux's
   or CloudNativePG's kinds. Those objects are checked field by field by item 4's properties, and
   kubeconform skips them by kind, never by a missing-schema wildcard.
6. **The archive's endpoint comes from the bootstrap, not git.** R2's endpoint,
   `https://<account id>.r2.cloudflarestorage.com`, names the founder's Cloudflare account, which
   is an OpenTofu input that no file in this repository holds. The journal's HelmRelease reads it
   with `valuesFrom` from the ConfigMap `flux-system/journal-archive-endpoint`. The bootstrap
   writes that ConfigMap from IAC's `archive_endpoint` output, as it writes the workload Secrets
   (DEC-2018), and git never holds it, so Flux neither overwrites nor prunes it. Until it exists,
   helm-controller does not install the journal, so nothing archives to a guessed address: the
   reference is never `optional`, and the chart has no default endpoint. The ConfigMap holds the
   endpoint's URL alone, never a credential; the key stays the Secret `journal-archive-key`. In
   CI, the bootstrap masks the value like the other OpenTofu outputs it holds (`xtask/src/mask.rs`).
   The checks refuse an overlay that sets the endpoint itself, ships that ConfigMap, reads it
   optionally or not at all, and any tracked file that names an account in an R2 endpoint. The
   coordinator chose this over committing the account id, since the founder keeps identifiers out
   of this public repository.

### Rationale

Two /32s are the narrowest rule that reaches R2, and they need no change to the chart, no proxy
and no new component. An address change costs archiving until a reviewed fix, never an open path.
The checks run in the required `fast` check on every change under `deploy/`, so an overlay that
would deploy a stub tier, `live`, or an unpinned image never reaches `main`.

### Alternatives considered

- **Cloudflare's published ranges:** the chart refuses anything wider than /24, and they would
  open the journal to every site behind Cloudflare.
- **An egress proxy allowing only the R2 hostname:** a new component in the journal's path, and a
  chart change to point the Barman Cloud sidecar at it.
- **Through the Cloudflare Tunnel:** cloudflared does not carry outbound S3 traffic; Access TCP
  would put a second tunnel client in each journal pod.
- **A hostname rule (a DNS-aware network policy):** DOKS's Cilium can express it only through its
  own CRD, outside the chart's portable policies.

<a id="dec-2018"></a>

## DEC-2018: each hosted cluster is bootstrapped from lane IAC's infra workflow, with the founder's terminal as break-glass

| | |
|---|---|
| **Status** | Accepted; item 4 superseded in part by [DEC-2041](infrastructure.md#dec-2041) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79, for items 1 to 6 (the founder's terminal) and the design of item 7; the founder decided the CI path's two openings (DEC-2605 row 11: each infra run admits its own runner's /32 to the Kubernetes API for the run; row 12: OpenTofu mints the archive's R2 token), which lane IAC's DEC-2028 and DEC-2029 carry out. No agent ever holds a Kubernetes credential (ES-12, DEC-1005) |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); DEC-2015 items 4 and 5; the founder's DEC-2011 (DEC-2605 rows 1 to 4); the founder's answers for the CI path (DEC-2605 rows 11 and 12); lane IAC's handoff from #1639 and its DEC-2028 and DEC-2029 |

### Context

DEC-2015 has Flux reconcile each hosted cluster from `main`, read-only, and the workload secrets
created at bootstrap until the vault (E21-32). Something has to install Flux and hand it its first
`sync.yaml` once per cluster. Lane IAC's control-plane firewall admits only the founder's
addresses (`KUBE_API_ALLOWED_CIDRS`), and no agent may apply anything to a real cluster. The
founder then required no by-hand steps and access that does not depend on one device, and
allowed each infra run to admit its own runner for the run (DEC-2605 rows 11 and 12), so a CI job
now holds a 15-minute kubeconfig during that window (item 7).

### Decision

1. **The founder's terminal (break-glass, item 7 is the primary path).** `cargo xtask gitops bootstrap <staging|production>`
   runs on the founder's machine, from an address the firewall admits, with the founder's own
   kubeconfig. No workflow runs it; a test refuses any workflow that names it.
2. **It refuses to run elsewhere, and confirms first.** It refuses when `CI` or `GITHUB_ACTIONS`
   is set, and when stdin is not a terminal (an agent's shell has none). Everything else is read
   and checked first: `sync.yaml`, the pinned flux, the pinned images, the secrets. Then it prints
   the kubeconfig's current context and the environment, and applies nothing until the founder
   types that context's name exactly.
3. **What it applies.**
   - Flux v2.9.6's three controllers from `flux install --export`, each image pinned to the
     digest `docs/dependencies.md` records. It refuses a flux that is not v2.9.6, an image it does
     not pin, and a pin the export never uses. It applies them server-side and waits for each to
     roll out.
   - The workload secrets `ws-ws1/journal-archive-key` (the R2 archive's key, DEC-2605 row 4) and
     `ws-ws1/cloudflared-tunnel`, from `JOURNAL_ARCHIVE_ACCESS_KEY_ID`,
     `JOURNAL_ARCHIVE_SECRET_KEY` and `CLOUDFLARED_TUNNEL_TOKEN` in the founder's shell. The first
     two are Barman's bucket-scoped R2 API token, which the founder makes by hand and which never
     enters OpenTofu's state (`deploy/tofu/README.md` §2 item 3, #1652). The third is IAC's
     sensitive OpenTofu output `tunnel_token`. Each Secret is built in memory, base64-encoded and handed to
     `kubectl` on stdin. Nothing prints a value, and an empty value is refused.
   - The ConfigMap `flux-system/journal-archive-endpoint`, holding only the archive's R2 endpoint
     from `JOURNAL_ARCHIVE_ENDPOINT` (IAC's sensitive output `archive_endpoint`), which names the
     founder's account and so is never in git (DEC-2017 item 6). It must be
     `https://<32 hex>.r2.cloudflarestorage.com`, and a failed write stops the bootstrap with an
     error.
   - The environment's `deploy/gitops/clusters/<environment>/sync.yaml`. From then on, Flux keeps
     it and everything under the directory it names.
4. **`cargo xtask gitops secrets <environment>`** writes the two Secrets alone, under the same
   refusals and confirmation, to set or rotate them.
5. **Flux upgrades go the same way.** Flux's own manifests are not in git (5,000 lines of CRDs for
   three controllers). To upgrade, change the pinned version, its SHA-256 and the digests in
   `xtask/src/gitops.rs`, `docs/dependencies.md` and `.cursor/install.sh` in one pull request, merge
   it, and the founder runs `bootstrap` again. Re-applying is idempotent. `install.sh` installs the
   pinned flux, checked by SHA-256, so the founder runs one command; `bootstrap` refuses any other
   version.
6. **No GitHub environment secret.** GitOps adds nothing to `infra-staging`. The founder's
   kubeconfig, the R2 token and the tunnel token stay on the founder's machine, in Cloudflare, and
   in IAC's encrypted OpenTofu state.

7. **From CI, as the primary path (the founder's requirement of 2026-10-18: no by-hand steps, and
   access that does not depend on one device; DEC-2605 rows 11 and 12, carried out by lane IAC's
   DEC-2028 and DEC-2029).** Lane IAC's `cargo xtask infra apply`, in `infra.yml` under
   `infra-staging` on `main` behind the founder's review, opens the runner's own public /32 on the
   control-plane firewall for the run (DEC-2605 row 11), fetches a 15-minute kubeconfig from
   DigitalOcean's API, and calls `gitops::bootstrap_from_infra` in the same process. The tunnel
   token and Barman's R2 token (minted by OpenTofu, DEC-2605 row 12) come from IAC's sensitive
   outputs, in memory. Then it closes the window on every exit path.
   - **The security boundary is GitHub's, not the code's.** Only a job under `infra-staging` holds
     the cloud credentials, and that environment admits only `main` and only after the founder
     approves the run. `bootstrap_from_infra` also refuses unless `GITHUB_ACTIONS` is `true`,
     `GITHUB_REPOSITORY` is this repository, `GITHUB_EVENT_NAME` is `workflow_dispatch`,
     `GITHUB_REF` is `refs/heads/main`, `GITHUB_WORKFLOW_REF` is this repository's
     `.github/workflows/infra.yml@refs/heads/main`, and the kubeconfig's current context is the
     expected one (`do-nyc3-mandate-staging` for staging). Those are environment variables any
     process can set, so they are a guard against running it by mistake, never a boundary.
   - It applies exactly what item 3 applies, idempotently, so every infra run re-asserts Flux, the
     Secrets and `sync.yaml`.
   - The secrets never reach the environment, argv, a file or a log. Each is a `Secret`, which in
     GitHub Actions masks the value as soon as it is made, with `::add-mask::`, in every form a
     line could print it (`xtask/src/mask.rs`): raw, base64, base64url, URL-encoded,
     JSON-escaped, and each line of at least four characters (a shorter one would hide every match
     of it in the log; the whole value is still masked). Each command's data escapes `%`, CR and
     LF (`%25`, `%0D`, `%0A`), since GitHub decodes them. This
     matters because the repository is public, so its Actions logs are, and GitHub masks only
     `secrets.*` values on its own. `Debug` is redacted, `kubectl` runs with `--v=0`, and no
     secret-bearing JSON is parsed into a typed struct, whose errors could echo a value.
   - Items 1 to 6 remain as the break-glass path, for when CI is unavailable.

### Rationale

The firewall decides where the bootstrap can run: the founder's addresses always, and each
founder-approved infra run's own runner for that run (DEC-2605 row 11). Flux's pull model then
needs no further access from outside; every change between runs is a commit to `main`. On the CI
path the kubeconfig lives 15 minutes and the window closes on every exit path, so the credential
exists only while a founder-approved run on `main` is going. On the founder's path the refusals
and the typed context make an accidental run against the wrong cluster, or a run from an agent's
shell, apply nothing.

### Alternatives considered

- **Opening the firewall to GitHub's runner ranges:** it would admit any runner, any time.
  Item 7 opens only the run's own /32, for the run (DEC-2605 row 11).
- **The founder's terminal as the only path:** the founder ruled out by-hand steps and access
  tied to one device; it stays as break-glass.
- **`flux bootstrap github`:** it needs a GitHub token able to create deploy keys, and it commits
  Flux's manifests. DEC-2015 item 2 needs neither.
- **Flux's manifests committed under `deploy/gitops/`, kept by Flux itself:** this adds 5,000
  generated lines to review, and Flux would apply its own upgrades on merge.

<a id="dec-2019"></a>

## DEC-2019: the workspace vault in the cell, OpenBao in server mode on raft, over TLS 1.3, unsealed by hand

| | |
|---|---|
| **Status** | Accepted; item 6 superseded for staging only by [DEC-2021](infrastructure.md#dec-2021) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the agent (Claude Code, lane V2), as reversible engineering choices under DEC-79 and DEC-176; each item applies DEC-977 (items 4 to 6) and DEC-990 (D-5) to the cell, and none spends, holds live money, or weakens a safety rule |
| **Story or claim** | E21-32, claim [#1442](https://github.com/kunwarshivam/mandate/issues/1442); DEC-434 item 14; DEC-977, whose "Not in this decision" left the cell's OpenBao to this one; DEC-2011 (staging) |

### Context

DEC-977 shipped `SecretSource` and its OpenBao client, tested against a dev server in CI. The cell
still ran no vault: the workspace chart's egress already reaches "the vault's pods" by label on
8200, but no chart rendered one. Staging (DEC-2011) needs its workload secrets from the vault.

### Decision

1. **A chart of its own,** `deploy/cell/charts/vault` (`mandate-vault-server`), installed into the
   workspace's namespace before the workspace chart, so its pods carry the labels the workspace's
   vault egress selects (`mandate.dev/workspace`, `mandate.dev/process: vault`). It renders one
   StatefulSet of one replica, its config, a ClusterIP Service on 8200 alone, its TLS, and its
   network policy. The image is DEC-990's OpenBao 2.7.1, by the same digest CI runs, and the
   values schema accepts only an image pinned by digest.
2. **Server mode on integrated storage, never the dev server.** The command is fixed in the
   template (`bao server -config=…`), the values schema has no field that adds an argument or an
   environment variable, and the storage is `raft` on a volume of the pod's own. `cargo xtask cell
   up` refuses a vault whose status reports any other storage, as the client does outside its own
   tests (DEC-977 item 6).
3. **TLS 1.3 only, from a CA made for this vault.** At the first install the chart makes a CA and
   a server certificate the CA signs, for the Service's names and the pod's loopback. The CA's key
   is never stored, so nothing can sign another certificate this vault's clients would trust.
   Clients mount the CA certificate from the ConfigMap `vault-ca`, which is not a secret. The pair
   is kept on upgrade by reading the Secret back, and remade when it is within 30 days of its end
   (365 days by default), as the backup plugin's TLS is (DEC-1050 item 3); then every client
   restarts to read the new CA.
4. **Reached only by its own processes.** Ingress is the workspace's pods with
   `mandate.dev/egress-vault: "true"` on 8200, from the same namespace only; egress is none. The
   operator reaches the vault through `kubectl exec`, never the network. The pod is hardened as the
   workspace's are: non-root, read-only root filesystem, no capability, no escalation, the
   runtime's seccomp profile, no service-account token, and requests equal to limits. Memory
   locking is off (`disable_mlock`), since the pod holds no `IPC_LOCK` and raft storage needs none.
5. **Sealed is not ready, and nothing restarts it.** The readiness probe runs `bao status` in the
   pod, over TLS to its loopback, which exits 0 only for an unsealed vault, so the Service routes
   nothing to a sealed or uninitialized one; an exec probe needs no network policy to admit the
   kubelet, whichever plugin the cluster runs. There is
   no liveness or startup probe, since a restart seals the vault again (DEC-977 item 5).
6. **Init and unseal.** In the local cell, `cargo xtask cell up` initializes the vault once with
   one unseal key and keeps it with the root token in `target/cell/vault-init.json`, mode 0600, and
   unseals with the key on stdin, never in an argument or a log. That cell holds fake secrets only.
   A hosted cell is initialized by the founder with three key shares and a threshold of two,
   held offline, never in a file the cell, the repository, CI, or an environment variable can read,
   and unsealed by hand after every restart of its pod, as DEC-977 item 5 says; auto-unseal stays
   H4, the founder's spend.
7. **Values per cluster.** The chart's defaults are the local cell's; `deploy/cell/values/
   vault-managed.yaml` sets DigitalOcean's block-storage class for staging.

### Residuals, disclosed

- The chart keeps its TLS pair by Helm's `lookup`, which a GitOps controller that renders with
  `helm template` cannot do (Argo CD does not; Flux's helm-controller does). Under such a
  controller every sync would make a new pair. Lane GO must install it through a controller that
  runs Helm itself, or supply the Secret `vault-tls` and the ConfigMap `vault-ca` another way, such
  as cert-manager, before the chart is used.
- Policies, roles and AppRole secret ids are not yet provisioned in the cell: `provision`
  (DEC-977 item 2) has no process that runs it there, and no process reads a secret id from a
  mounted file yet. Until then a workspace process can reach the vault's port and log in as
  nothing. That, and an egress drill that shows only the vault's processes reach it, is the next
  slice.
- The vault's own audit device is not enabled, so no read is audited yet (K1, DEC-977).
- One replica: the vault is down while its pod reschedules, and a running executor keeps its lease
  through that (DEC-977 item 7).
- The local cell's unseal key and root token sit in a 0600 file on the developer's machine; they
  guard only fake secrets.

### Rationale

Raft is the storage OpenBao recommends and needs no second service. A file of the server's own
reaches no journal, object store, or Postgres role, so a compromise of those reveals no secret.
Running `bao` past the image's entrypoint keeps the pod free of the entrypoint's `setcap` and
`chown`, which need root. Manual unseal costs nothing and adds no risk (DEC-977 item 5).

### Alternatives considered

- **The upstream OpenBao Helm chart.** Rejected for now: it carries an injector, a CSI provider and
  a UI that the cell does not use, so more surface to pin and review, and its defaults (a dev mode
  flag among them) would have to be pinned off in values rather than absent from the template.
- **The workspace chart renders the vault.** Rejected: the vault's lifecycle (init, unseal, its
  volume) differs from the processes', and an upgrade of the workspace must never restart it.
- **PostgreSQL storage in the journal's cluster.** Rejected: it would put the vault's ciphertext
  and a database role able to reach it in the journal's Postgres, and its availability with it.

<a id="dec-2020"></a>

## DEC-2020: the platform's own secrets in the vault, the model provider's key and the email credential, each read by one process type

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the agent (Claude Code, lane V2), as reversible engineering choices under DEC-79 and DEC-176: it adds rows to DEC-977 item 2's table that each allow one process type one read, refuses everything else, and weakens no rule |
| **Story or claim** | E21-32, claim [#1442](https://github.com/kunwarshivam/mandate/issues/1442); DEC-977; DEC-2019; DEC-997 (OpenRouter); DEC-2011 (staging) |

### Context

Staging's workload secrets must come from the vault, never an environment variable or a committed
file (DEC-2011, E21-32's "no secret is ever an environment variable"). DEC-977's table holds only a
connection's code and token and the OAuth client secret, so the model gateway's provider key and
the key the platform's email is sent with have no path and no identity. A broker's paper key is a
connection's credential: it stays the connection's token, which only its executor leases.

### Decision

1. **Two identities and one path kind.** `Identity::ModelGateway` (role `model-gateway`) and
   `Identity::EmailSender` (role `email-sender`); `SecretPath::Platform(PlatformSecret)`, where
   `PlatformSecret` is `ModelProviderKey` (`platform/model-provider-key`) or `EmailCredential`
   (`platform/email-credential`) under the KV mount.
2. **The rows.** `policy::holders` names, for each platform secret, the one identity that reads it
   and the one that writes it:

   | Secret | Read | Write | Delete |
   |---|---|---|---|
   | The model provider's key | Model gateway | Secret import | — |
   | The email credential | Email sender | Secret import | — |

   Every other identity, the runtime included, is refused all three. No identity deletes a platform
   secret: a rotation writes over it, and the reader picks the new value up at its next read. The
   OpenBao policies are generated from the same table, every path spelled out, no wildcard;
   `provision` makes one role for each new identity.
3. **A read, not a lease.** A platform secret is read with `SecretSource::read`, at the process's
   start and after a rotation; the process keeps it in memory, so a vault outage after the read
   stops nothing, and a read during one is an outage (`Unavailable` or `Sealed`), never a refusal.
4. **The local backend** stores each as a client-secret entry of its own `key_ref` in the
   executor's vault; on the demo host the model gateway and the email sender would run as the
   executor's user, so the filesystem does not separate them from the executor there, and the
   table checked in code does.
5. **Who consumes them.** Lane AH/I2's model gateway (OpenRouter, DEC-997) and lane M7's email
   transport each open a session as their identity and read their one secret; neither reads a key
   from its environment.

### Rationale

One reader per secret keeps a compromised process to its own secret, as DEC-977 does for a
connection's token. A lease would add renewal machinery for a value that rotates by hand.

### Alternatives considered

- **One "platform" identity reading every platform secret.** Rejected: the model gateway would
  hold the email credential.
- **The secrets as Kubernetes Secrets mounted as files.** Rejected for staging: E21-32 puts every
  credential behind `SecretSource`, and a Secret is readable by whoever reads the namespace's
  Secrets.

<a id="dec-2021"></a>

## DEC-2021: staging's vault unseals itself with OpenBao's static seal, from a key mounted from a bootstrap-created Secret; the cell and production stay unsealed by hand

| | |
|---|---|
| **Status** | Accepted; items 3 and 4 superseded for staging by [DEC-2052](infrastructure.md#dec-2052) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the founder decided that staging unseals itself (DEC-2605 row 10, PR #1624, "Auto-unseal for staging (Recommended)"); the agent (Claude Code, lane V2) designed how, under DEC-79 and DEC-176, refusing it everywhere but staging |
| **Story or claim** | E21-32, claim [#1442](https://github.com/kunwarshivam/mandate/issues/1442); supersedes [DEC-2019](infrastructure.md#dec-2019) item 6 for staging only; DEC-977 item 5; DEC-2011 (staging); DEC-2015 (Flux, the founder's bootstrap) |

### Decision

1. **The seal is a value, off by default.** The vault chart's `seal.type` is `shamir` (the
   default: unsealed by hand, DEC-977 item 5) or `static`. Its schema refuses any other type, and
   refuses `static` unless `environment` is `staging` and `seal.keyId` names the key. The chart's
   defaults (`environment: cell`) and `deploy/cell/values/vault-managed.yaml`
   (`environment: production`) render no seal block and mount no seal key. A test pins both, and
   that the static seal is refused in each.
2. **Staging's seal.** `deploy/cell/values/vault-staging.yaml` sets `environment: staging` and
   `seal: {type: static, keyId: staging-1, keyObject: vault-seal-key}`. The config gets one
   `seal "static"` block, with `current_key_id` the key's name and `current_key` the file
   `/openbao/seal/key`. That file is the `key` entry of the Secret `vault-seal-key`, mounted
   read-only at mode 0440 into the vault's pod; no other chart of the cell's mounts it, though
   nothing in the cluster stops another pod in the namespace from naming it (see the residuals).
3. **The key never touches git, a value, a log, or an environment variable.** The founder's local
   `cargo xtask gitops bootstrap staging` (lane GO) creates the Secret before the chart's first
   install. It runs at a terminal outside CI, after a typed context confirmation, and reads the key
   from the founder's shell (`read -rs`), piping it to kubectl's stdin, never into an argument, a
   file, or a log. No CI job can create it: staging's API admits only the founder's addresses
   (DEC-2015 item 4, DEC-2018). The key's master copy may stay in the `infra-staging` GitHub
   environment. The chart only names it. It is mounted as a file, not read through
   `valueFrom`, so the chart's no-Secret-in-the-environment invariant holds without an exception.
   The only Secret the chart references outside its own TLS is this one, in staging alone.
4. **Its own init path.** A vault with the static seal is initialized once with recovery keys,
   `bao operator init -recovery-shares=3 -recovery-threshold=2`. The founder keeps them offline, as
   DEC-2019 item 6 keeps unseal shares; they are needed only to generate a root token or rotate the
   seal, never to unseal. Restarts need nothing from the founder.
5. **Rotation.** A new key gets a new `keyId`; the old one moves to `previous_key` in a one-off
   change. That is a staging operation with its own steps, not a value this chart renders today.
   The chart refuses a second key, which the test pins.
6. **Production stays open.** Production's unseal is not decided; until it is, it unseals by hand,
   as the cell does.

### Residuals, disclosed

- `environment` is declared by the values file, not detected: the schema stops a mistake (a
  production values file asking for the static seal), not someone who sets `environment: staging`
  on purpose in another cluster. Which values file each cluster installs is the overlays' (lane GO,
  DEC-2015), reviewed like any other change to them.
- Any pod in the workspace's namespace whose spec names the Secret `vault-seal-key` could mount it;
  only the vault's chart does. Kubernetes has no per-Secret mount restriction without an admission
  policy, which the cell does not add for this Secret yet.
- Anyone who can read Secrets in the workspace's namespace, and who holds the vault's volume, can
  decrypt its storage. The deployment manager's Role reads no Secret (DEC-1172), and no workspace
  process mounts this one. The cluster's administrators remain the founder alone.
- OpenBao's static seal is meant for cases like this one, a key held beside the cluster; it adds
  nothing over manual unseal against an attacker with the cluster's Secrets. That is why it is
  staging's alone, holding paper credentials only.
- The seal's syntax follows OpenBao 2.7's static seal. Its first boot is staging's own; the cell
  keeps Shamir, so CI does not exercise it.

### Alternatives considered

- **The key through an environment variable (`env://`, from `valueFrom`).** Rejected: a file
  mount keeps every secret out of the process environment (DEC-822 item 4), so no exception is
  needed.
- **A cloud key service (H4).** Rejected for staging: it is a spend, and the founder chose the
  key in the GitHub environment.

<a id="dec-2022"></a>

## DEC-2022: provisioning the vault in the cell: credential files owner-only, `mandate vault provision`, and a drill that proves each identity's reach

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the agent (Claude Code, lane V2), as reversible engineering choices under DEC-79 and DEC-176; the coordinator set the order and left the secret-id mounts to V3 |
| **Story or claim** | E21-32, claim [#1442](https://github.com/kunwarshivam/mandate/issues/1442); DEC-977 item 2 (`provision`); DEC-2019; DEC-2021 |

### Decision

1. **Credential files** (`mandate_vault::credentials`).
   - `write_credentials(dir, credentials)` writes each identity's `<role>.role-id` and
     `<role>.secret-id`. Each is a plain file of mode 0600 holding the value alone. A file already
     there is replaced and set to 0600 again.
   - It makes `dir` with mode 0700 when it is absent. It refuses, as
     `SourceError::CredentialFile`, a `dir` that is a symbolic link, is not a directory, or whose
     mode is anything but 0700 (0500 and 0300 included, so the refusal is the same whatever user
     runs it), and then it writes nothing.
   - It refuses an output name that is anything but a plain file or absent, a planted symbolic
     link included: the link is neither followed nor replaced, and its target is neither read nor
     written. Refusal, the stricter of the two readings, is chosen over replacing the link.
   - It checks every output name before it writes any, so a refusal changes no file.
   - It refuses a `dir` whose parent directory has a group or other write bit without the sticky
     bit, since anyone who may write there could rename `dir` away and put a link in its place. A
     sticky parent, as the temporary directory is, is allowed. Only that immediate parent is
     checked: the directories above it are the operator's own, as `dir` is, so a widened ancestor is
     the operator's to fix and not a case the writer guards.
   - `check_credential_dir(dir, connections)` applies these rules before any vault is reached:
     the parent, `dir` when it exists, and every name `write_credentials` would write for the
     credentials `provision` issues for `connections` (every identity's but the token
     exchanges'). An absent `dir` passes and is not made.
   - It writes each value to `.<name>.<pid>.<attempt>.tmp` in `dir`, opened with `create_new` so no
     existing file or link is ever opened. When that name cannot be made, taken or not, it tries the
     next attempt, up to 8, then refuses. It syncs the file, renames it over the output name, and
     syncs `dir`. A hard link at an output name passes the check as a plain file; the rename
     replaces it, so the file it shares is never written. A crash leaves the earlier file or the new
     one, never an empty one. A plain file `.<name>.<anything>.tmp` that a crashed write left behind
     holds a secret at mode 0600, so the next write of that name removes it first. A temporary file
     is also removed when a later step of its own write fails.
   - `read_token_file(path)` reads an operator's token from a plain file with no group or other
     bit, not a symbolic link, holding the token and at most one trailing newline. A missing,
     empty, linked, multi-line, or group- or world-readable file is refused, and so is one holding
     a carriage return anywhere, a CRLF ending included.
2. **`mandate vault provision`** (in `mandate-cli`). It takes the vault's address, a CA
   certificate file, an operator token file read by `read_token_file`, and the connections. It
   checks `--out` with `check_credential_dir` before it reaches the vault, so a refused
   directory never leaves secret ids issued and unwritten, then runs `provision` and writes the
   credentials with `write_credentials` into `--out`. The token
   is never an argument or an environment variable, and the command prints only the file names.
3. **The cell.** After it unseals the vault, `cargo xtask cell up` runs the provisioning once in a
   one-off Job labelled with the vault egress. A new `vault` drill logs in from probe pods as each
   identity and shows the acceptance in the cell:
   - a runtime reads nothing;
   - an executor reads exactly its own connection's token;
   - no policy holds a wildcard.

   Seeded, a planted wildcard policy must be caught.

### Not done

- Mounting each process's secret id into the workspace chart is V3's, with the executor's wiring
  (E21-9). No process reads one before then.
- Removing a removed connection's roles, and revoking earlier secret ids, stays the deployment
  manager's (DEC-977, residuals).

### Alternatives considered

- **Provisioning from xtask directly.** Rejected: staging needs the same command, run by the
  founder, so it belongs in the CLI.
- **Secret ids as Kubernetes Secrets now.** Rejected until a process reads them (V3).

<a id="dec-2023"></a>

## DEC-2023: the demo's step 5 runs the backtest runner and records its run in the gestures' pod

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the agent (Claude Code, lane E2E), with the coordinator's agreement, as reversible engineering choices under DEC-79 and DEC-176; it loosens no rule |
| **Story or claim** | E21-54, claim [#1528](https://github.com/kunwarshivam/mandate/issues/1528); DEC-1144 item 8, DEC-2002, DEC-2010 |

### Context

DEC-1144 item 8 leaves the cell's run of the backtest's two steps to the cell's lanes: the runner
binary is in the `mandate` image (#1581), but nothing runs `mandate-backtest-runner run` and then
`mandate backtest record` in a cell. `record` refuses `rule_set_missing` without a registered rule
set, needs the version created in the same store the runner reads, and needs the demo's inputs in
the pod. The demo's step 5 named all of this as lane E2E's wait.

### Decision

1. **Three gestures in the gestures' pod, after the version is confirmed and before the kill
   switch:** `config register --kind rule_set`, the runner's `run`, and `backtest record`. One pod
   holds the store `version create` wrote, so the runner finds the mandate document there, and the
   closing `journal verify` checks every artifact the record names against the same store.
2. **The rule set is the run's settings.** No schema defines a rule set's content and the CLI
   accepts any object, so the demo registers the runner's `settings.json`: the gate, fee, regime
   and simulation parameters the run applies. A defined rule-set document replaces it when one is
   specified.
3. **The runner's committed demo inputs.** The five files under
   `crates/mandate-backtest-runner/tests/fixtures/demo/` are mounted at `/in/backtest`, a directory
   of their own (the runner's `instrument.json` is not the CLI's instrument snapshot), through the
   same ConfigMap with per-item paths, since a ConfigMap key admits no `/`.
4. **A fixed id and payload.** The backtest id is `bt-demo-01`; each run's workspace is fresh, so
   it never collides. The runner is deterministic, so the scenario expects the golden store's
   `payload=` reference from the runner and passes it to `record`, which must print the golden
   `report=`. An xtask test pins both constants to `demo-store.json`, so a change to the runner or
   its inputs fails that test before it fails a cell run.
5. **A gesture may run another binary of the image.** A gesture whose first argument is the
   runner's path runs it as the container's `command`; every other gesture keeps the image's
   `mandate` entry point.
6. **The API read stays a gap.** `GET /v1/workspaces/{ws}/backtests/bt-demo-01` with the owner's
   session, and web/'s report screen, wait on lane MB's membership parser and on W2.

### Consequences

- Step 5 reads partial instead of skipped; the record and its artifacts are journaled and verified
  in every `cell-demo` run, which adds three short containers to the gestures' pod.
- The same change takes the two review nits on #1625 (DEC-2010 item 3): the API re-point's
  `helm upgrade` waits at most 3 minutes instead of 5, so the worst case stays inside DEC-464, and
  a cell with no deployment manager says so instead of skipping its rollout wait silently.
- DEC-1144 item 8's "the cell's run of both steps" is done for the demo; `POST /backtests`, its
  `running` state, and a data-plane manifest remain as that item lists.

### Alternatives considered

- **A separate Job for the runner:** it would need the gestures' store shared across pods, which
  the cell's `work` volume is not.
- **Reading the payload reference from the runner's output at run time:** the pod's containers
  cannot pass values to one another without a script in the image; the fixed reference is what
  DEC-1144 item 8 already names.
- **A hand-written rule set:** it would invent a format no spec defines.

<a id="dec-2024"></a>

## DEC-2024: The release key is a cosign key held in the protected `release-signing` environment, and CI signs a release only on a dispatch from `main` the founder approves

| | |
|---|---|
| **Status** | Accepted; item 4 superseded in part by [DEC-2049](infrastructure.md#dec-2049) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | The founder decided the route: no hardware key; paper-only staging runs before any key; before production, a release signing key lives in a protected GitHub environment and signs each release in CI only after the founder approves (DEC-2605 rows 8 and 9, recorded in PR #1624). The design below is the lane RS builder's (`claude-code`), as reversible engineering choices under DEC-79 and DEC-176, and it only tightens what those rows say |
| **Story or claim** | E21-35 (lane RS), DEC-971 item 7 |

### Decision

1. **The release key.** One cosign ECDSA P-256 key pair, generated once by the founder on their own
   machine with `cosign generate-key-pair` (as decided in DEC-2605 row 8, no hardware key). The
   encrypted private half (`cosign.key`) and its password are stored only as the secrets
   `RELEASE_SIGNING_KEY` and `RELEASE_SIGNING_PASSWORD` of the GitHub environment
   `release-signing`. That environment has the founder as its required reviewer and admits only the
   `main` branch. The public half is `crates/mandate-release/release-keys/cosign.pub`, compiled
   into `startup::RELEASE_KEYS` and pinned there by its fingerprint (#1689), the only place a key
   is trusted (DEC-971 item 2). No agent
   generates, reads or handles the private half or its password. The steps are in the runbook
   [release-signing-key.md](../../runbooks/release-signing-key.md).
2. **Three jobs in `release.yml`** run on a manual dispatch from `main` only, never on a push or a
   pull request, once per arch on a runner of that arch, each with only `contents: read`.
   `release-signing-build` rebuilds the release and hands it on. `release-signing` signs it, and
   waits for the founder to approve its environment before any step runs (DEC-2605 row 9).
   `release-signing-check` proves the signatures. Only `release-signing` reads a secret or names
   an environment. An xtask test (`only_the_dispatched_signing_job_holds_the_release_key`) holds
   each of these.
3. **What is signed.** `release-signing-build` requires the digest its arch's `build-a` printed,
   so only what `compare` proved reproducible is signed. `release-signing` signs each binary and
   `SHA256SUMS` itself. Each binary gets its own signature because the start-up check verifies a
   signature over the executable's own bytes (DEC-971 item 2). The signature over `SHA256SUMS`
   names the whole release in one file. Signatures are cosign `sign-blob` files, made offline. The
   run keeps them, with `SHA256SUMS`, as the artifact `release-signatures-<commit>-<arch>`.
4. **Nothing untrusted runs beside the key.** A build script or dependency of the build could
   otherwise plant a cosign, a `PATH`, an `LD_PRELOAD` or a cargo configuration for the step that
   holds the key. So `release-signing` checks out only `.github/scripts` and runs no cargo and no
   xtask. Its one step runs `.github/scripts/release-sign.sh`, which:
   - requires `dist` and `dist/bin` to be real directories and `SHA256SUMS` a regular file, none
     a symlink, and `dist/bin` to hold exactly the names `SHA256SUMS` lists, once each, as
     regular files, each line well formed, so a file planted in the handed-on artifact is never
     signed;
   - copies those files into a private directory, checks the copies against `SHA256SUMS` and
     `SHA256SUMS` against `build-a`'s digest, and signs the copies, so nothing changes between
     the check and the signature;
   - installs cosign from its release, pinned to the same SHA-256 as `cargo xtask release tools`;
   - decodes the key into a 0600 file in a 0700 directory under `RUNNER_TEMP`, deleted on every
     exit;
   - starts in POSIX mode (`#!/bin/bash --posix`), where bash refuses to start when an exported
     function shadows a special builtin such as `set` or `unset`; turns off any trace the job's
     environment asked for (`set +x +v`), so `SHELLOPTS=xtrace` cannot echo the decoded key;
     removes a planted `compgen`, the check's own tool, and refuses to go on while any other
     exported function is defined, so nothing planted can silence the trace switch or stand in
     for a check; then leaves POSIX mode;
   - starts each signature from an empty environment (`env -i`) running the checked-in
     `release-sign-cosign.sh`, which sets `HOME` and `PATH`, reads the password from its stdin
     and execs cosign without the variables bash adds. cosign's environment holds exactly `HOME`,
     `PATH` and `COSIGN_PASSWORD`, an allowlist, so nothing cosign or sigstore reads later
     (`SIGSTORE_*`, proxy variables), no exported function and no oddly named variable reaches
     it, and the password is never on a command line, where `ps` would show it. The helper reads
     one line, so a password holding a newline is cut there and cosign refuses the key, which
     fails closed. A test runs the
     signer with a stand-in cosign, plants all of those and `SHELLOPTS=xtrace`, and checks that
     cosign's environment holds exactly the three names, its arguments no password, and the
     signer's stderr neither secret.

   The key secret is cosign's `cosign.key` as single-line base64, because GitHub masks a
   multi-line secret unreliably. No command line or log carries either secret, and xtask prints
   a command's program and arguments only, never its environment.
5. **The run proves its own signature.** `release-signing-check`, which holds no secret, verifies
   every signature against the committed public half (`cargo xtask release check-signed`). It then
   runs `mandate-agentd`, unpacked from an image with its new signature beside it, and requires it
   to start at tier `production`. A pair other than the committed one therefore fails the run
   instead of leaving a release nothing would start.
6. **Staging.** A signed release starts at staging through the existing rule (DEC-971 item 2),
   so signed staging needs no change to the start-up check. The unsigned, paper-only staging
   admission that DEC-2605 row 9 allows before the key exists is not built: it would loosen the
   start-up rule, and the founder's key now exists.
7. **Supersedes** DEC-971 item 7's "the founder's hardware key is the only release key" and
   ADR-0001 ES-17's "cosign signing with a founder hardware key", for the reason in DEC-2605 row 8.
   Everything else in DEC-971 stands, the ephemeral test key in CI included.

### Rationale

The founder declined a hardware key on time and cost (DEC-2605 row 8). A protected environment
with a required reviewer is the closest equivalent GitHub offers. The private half never leaves
GitHub's secret store after the founder uploads it. A run can use it only on `main`, only in one
job, and only after the founder approves that run. Signing each binary keeps the start-up check
exactly as DEC-971 built it: offline, against a compiled-in key, with no change to a
safety-critical crate beyond adding the key. The drill at tier `production` turns a mismatched key
into a red job rather than a release that refuses at start.

### Alternatives considered

- **A hardware key (YubiKey, ES-17).** Declined by the founder (DEC-2605 row 8).
- **Keyless signatures checked in process.** `publish` already signs images keyless under `main`'s
  OIDC identity. Those signatures cover the image manifest, not the binary. Checking them needs
  Fulcio's certificate chain, an X.509 parser the crate does not have, and Rekor. DEC-971 rejected
  this too.
- **Signing only `SHA256SUMS`.** The start-up check would then need the sums file and a lookup of
  its own digest in it: new safety-critical logic for no gain over signing each binary.
- **The public half read from a file at start.** Whoever can change the pod could add a key
  (DEC-971's alternatives).
- **An agent generating the pair and handing over the private half.** A credential would pass
  through an agent (`AGENTS.md` rule 7).

<a id="dec-2025"></a>

## DEC-2025: A burst of merges to `main` is released once, at its tip, and a manual signing run waits in a group of its own

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | The lane RS builder (`claude-code`), as a reversible engineering choice under DEC-79 and DEC-176, at the coordinator's request after `release` runs queued behind each other on `main` |
| **Story or claim** | E21-35 (lane RS); supersedes the "each `main` commit" of [DEC-972](earlier-900-1999.md#dec-972) item 3; [DEC-2024](infrastructure.md#dec-2024) |

### Context

`release.yml` gave every `main` commit its own concurrency group, so each merge queued a full
release: four builds, `compare`, and the publish jobs. On 2026-10-18, 33 `main` release runs were
queued, the oldest still in progress had started at 22:45, and work waiting on a published image
waited behind all of them.

### Decision

1. **Pushes to `main` share one group, `release-refs/heads/main`, with `cancel-in-progress: false`.**
   GitHub keeps the running release and, of the runs behind it, only the newest pending one. A
   burst of merges is therefore released once, at its tip. No running release is cancelled, so no
   publish stops half done.
2. **A manual dispatch has its own group, `release-dispatch-<ref>`, also never cancelled.**
   A dispatch's `release-signing` job waits for the founder to approve the `release-signing`
   environment (DEC-2024 item 2), and a run waiting on approval holds its group. In the pushes'
   group it would hold every push release behind it until the founder answered. Two dispatches of
   one ref keep the running one and the newest pending one, as pushes do.
3. **Pull requests keep their own group per ref, with `cancel-in-progress: true`**, as before.
4. **DEC-972 item 3 now reads "each released `main` commit"**: the two builds and `compare` prove
   every commit that is released. A commit skipped by item 1 is never published, signed, or run by
   a cell, so no proof is owed for it. Nothing DEC-2024 needs is lost: a signing run is dispatched
   on the tip and rebuilds and compares that commit itself (DEC-2024 item 3). An xtask test pins
   the concurrency block.

### Rationale

Only a released commit reaches a cell, so proving each released commit keeps everything the
reproducibility check protects, at one release per burst instead of one per merge. Keeping
`cancel-in-progress: false` means a publish already pushing images or signing them keyless always
finishes, so the registry never holds half a release.

### Alternatives considered

- **`cancel-in-progress: true` on `main`.** A new merge would cancel a release mid-publish,
  leaving images pushed without their signature or index.
- **One group for pushes and dispatches.** The founder's pending approval would hold every push
  release, as item 2 says.
- **Keep one group per commit and add runners.** That costs money (DEC-79), for proofs of commits
  nothing runs.

<a id="dec-2026"></a>

## DEC-2026: A hosted cell's images are published only from a founder-approved signing run, as `signed-<commit>`, each carrying its binary's release signature

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | The lane RS builder (`claude-code`), as reversible engineering choices under DEC-79 and DEC-176, on the plan the coordinator approved. The route is the founder's: the release key signs a release in CI only after the founder approves (DEC-2605 rows 8 and 9). This only tightens it |
| **Story or claim** | E21-35, claim [#1865](https://github.com/kunwarshivam/mandate/issues/1865); [DEC-2024](infrastructure.md#dec-2024); [DEC-971](earlier-900-1999.md#dec-971) item 2; [DEC-1060](earlier-900-1999.md#dec-1060) |

### Decision

1. **Where they come from.** E21-35 says only the release key, on the founder's approval, signs
   an image for a hosted cell. So the images a hosted cell runs are published only by a manual
   dispatch of `release.yml` from `main`, in two jobs:
   - `release-signing-publish`, once per arch on a runner of that arch;
   - `release-signing-publish-index`, after both arches.

   Both run after `release-signing-check` has proven that run's signatures (DEC-2024 item 5).
   Neither holds a release secret or names the `release-signing` environment. Each holds only the
   run's own token, with `packages: write` and `id-token: write`, as `publish` does on a push.
2. **Nothing the key did not sign is pushed.** Before any image is built or the job signs in to
   the registry, `cargo xtask release publish-signed` checks two things:
   - every binary `SHA256SUMS` lists matches its line;
   - every binary, and `SHA256SUMS` itself, carries a signature that verifies against the
     committed public half (`crates/mandate-release/release-keys/cosign.pub`).

   A missing signature, another key's signature, or a binary the sums do not list stops it.
3. **What is pushed.** Each image is the image `publish` builds, plus a `<name>.sig` layer
   holding the binary's release signature byte for byte (DEC-971 item 2's layout). The tags are:
   - `signed-<commit>-<arch>` for each arch's image;
   - `signed-<commit>` for the multi-arch index over both, as DEC-1060 builds `main-<commit>`.

   Each image and the index are signed keyless under the run's OIDC identity, and each image's
   SBOM is attested, as `publish` does. The run's `PUBLISHED` lists the `signed-*` references by
   digest.
4. **`main-*` stays for dev and CI.** The images a push to `main` publishes carry no release
   signature, so they start only at tiers `dev` and `ci` (DEC-2024).
5. **The hosted overlays are not changed here.** Pointing them at `signed-*` references, and
   promoting from a signing run's `PUBLISHED` instead of a push run's, is lane GO and IAC's work
   (E21-58, DEC-2016). This lane documents it in the runbook and changes no overlay.
6. **Tests.**
   - `the_signed_images_carry_each_binary_and_its_release_signature` unpacks each image as a
     runtime would. It checks the image holds exactly the binary and its signature, and that every
     file was verified against the key first.
   - `the_signed_images_refuse_a_release_the_key_did_not_sign` checks each refusal in item 2,
     using a stand-in cosign.
   - The job's shape is pinned beside DEC-2024's: dispatch from `main` only, after
     `release-signing-check`, no secret, no environment.

### Rationale

A hosted cell refuses at start any binary the release key did not sign (DEC-971 item 2). So an
image without the signature layer is useless there, and one with another key's signature fails
only at start. Checking every signature against the committed key before any push turns either
mistake into a red job, not a published image no cell would start. Publishing from the signing
run keeps one approval per release: the founder approves `release-signing`, and nothing after it
holds the key.

### Alternatives considered

- **Signing the images inside `release-signing`.** That job would then need the registry token
  and the network beside the key, which DEC-2024 item 4 keeps away from it.
- **Re-tagging the `main-*` images.** They have no signature layer, and adding one changes the
  digest, so they would have to be rebuilt anyway. Rebuilding from the verified `dist` is that.
- **Checking only the image's keyless signature at admission.** That proves where the image came
  from, but not that the release key signed the binary, which is what the start-up check reads.

<a id="dec-2027"></a>

## DEC-2027: The release build refuses rustflags, compiler, wrapper and profile overrides, from its environment or any cargo configuration it reads

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | `claude-code` (lane SI, taking over lane R1's remainder at the coordinator's direction), as a reversible engineering choice under DEC-79 and DEC-176. It only tightens the release build, and loosens no rule |
| **Story or claim** | E21-52's last acceptance item and E21-8, claim [#1532](https://github.com/kunwarshivam/mandate/issues/1532); [DEC-2100](DEC-2100.md) item 2; [DEC-972](earlier-900-1999.md#dec-972) item 2 |

### Context

DEC-2100 item 2 requires that no build without debug assertions holds the stub issuer, and that
the release build "fails when either variable is set": `RUSTFLAGS` or
`CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS`.

DEC-972 item 2 already clears the build's environment down to its own settings. Neither variable
reaches the build, but setting one did not fail it: a run that asked for a flag shipped without
it, silently.

Two other paths could still change what the build compiles:
- a cargo configuration file that cargo reads, in the checkout, in any directory above it, or in
  `CARGO_HOME`;
- a `--config` argument on the build's own command line.

### Decision

1. **Variables.** `cargo xtask release build` refuses to start, naming each variable, when its
   environment sets any of these:
   - `RUSTFLAGS`, or any `CARGO_*RUSTFLAGS` variable (`CARGO_ENCODED_RUSTFLAGS`,
     `CARGO_BUILD_RUSTFLAGS`, `CARGO_TARGET_<triple>_RUSTFLAGS`);
   - any `CARGO_PROFILE_*` variable;
   - `RUSTC`, `RUSTC_WRAPPER` or `RUSTC_WORKSPACE_WRAPPER`, or their `CARGO_BUILD_*` forms.

   What `cargo run`, rustup and a runner set (`CARGO`, `CARGO_PKG_*`, `CARGO_HOME`,
   `RUSTUP_TOOLCHAIN`, `CARGO_TERM_COLOR`) is not refused.
2. **Configuration files.** It reads every cargo configuration the build's `cargo` would read:
   `.cargo/config.toml` and `.cargo/config` in the checkout and in each directory above it, then
   `CARGO_HOME`'s. It refuses, naming each file and key, when any of them sets:
   - a `profile`;
   - an `include`. Cargo follows `include` without any `-Z` flag, so an included file could set
     a profile; refusing the key fails closed rather than following the chain. An independent
     review confirmed an included `[profile.release] debug-assertions = true` turns
     `cfg!(debug_assertions)` on in a release build, and both runs of `release.yml`'s two-build
     compare read the same file;
   - an `env` table, which sets variables for the compiler and build scripts;
   - `build.rustflags`, `build.rustc`, `build.rustc-wrapper` or `build.rustc-workspace-wrapper`;
   - `rustflags` under any `target`.

   An unreadable or malformed file is a refusal too. Aliases, registry and network settings are
   not refused.
3. **The command line.** The build's `cargo` arguments carry no `--config` and no `-Z`, and a test
   pins this.

   A `--config` given to the outer `cargo xtask` cannot reach the build: cargo does not pass it to
   the processes it runs (checked), and the build's environment is cleared.
4. **When it runs.** All of this runs before any tool is installed or anything is built, so a
   refused run writes nothing. A test runs `release build` on a checkout whose configuration sets
   a profile, and requires this refusal, not any later step's error.
5. **Residual: the linker.** `target.<triple>.linker` and `CARGO_TARGET_<TRIPLE>_LINKER` are not
   refused.
   - The second is a variable the cleared environment already drops.
   - The first is a configuration setting. A linker runs after compilation, so it cannot set
     `cfg(debug_assertions)` or bring code back that `#[cfg]` left out, and so it cannot put the
     stub issuer into a release.
   - It could still change a release's bytes. That is the reproducible build's residual, recorded
     in DEC-972 item 2: `release.yml`'s compare catches a difference between runners, but not one
     the same configuration makes on both.

   It is accepted as residual risk, and the configuration that names a linker is the runner's.

### Rationale

The cleared environment already kept these overrides out of the build. Refusing them as well
turns a silent drop into a failure, which is what DEC-2100 item 2 asked for.

The configuration files were the one remaining channel. A `CARGO_HOME/config.toml` with
`[profile.release] debug-assertions = true`, or with a `rustc-wrapper` that adds
`-C debug-assertions`, would have built a release that holds the stub issuer. The workspace's own
`Cargo.toml` and `.cargo/config.toml` are guarded by xtask's
`release_builds_keep_debug_assertions_off` (#1620). This decision covers every other file cargo
reads.

### Alternatives considered

- **Pinning `--config profile.release.debug-assertions=false` on the build's command line:**
  rejected. It does not reach package overrides (`[profile.release.package."*"]`) or a wrapper, and
  it would hide an override rather than refuse it.
- **Refusing every foreign configuration file outright:** rejected. A runner or a developer may
  keep registry or proxy settings in `CARGO_HOME`, and those change nothing that is compiled.

<a id="dec-2028"></a>

## DEC-2028: each automated deploy that bootstraps the cluster (staging first) opens the control plane to its own runner's /32 for the run, and closes it on every path

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | decided by the founder, [DEC-2605](process.md#dec-2605) row 11 (DEC-79: it widens who may reach the cluster's API). The design below is lane IAC's, under DEC-79, and only narrows the answer |
| **Story or claim** | E21-56 (lane IAC), claim [#1634](https://github.com/kunwarshivam/mandate/issues/1634); with lane GO's E21-58 bootstrap; [DEC-2012](infrastructure.md#dec-2012) items 7 and 11, [DEC-2014](infrastructure.md#dec-2014) |

### Context

The founder asked for staging's bootstrap to run fully automatically, in CI, after the apply. The
cluster's control plane admits only the CIDRs in `KUBE_API_ALLOWED_CIDRS` (DEC-2012 item 7), which
a GitHub runner's changing address is never among.

### Decision

The founder's answer, as relayed:

> Q: "Staging's Kubernetes control panel accepts only fixed addresses. So that setup runs fully
> automatically with no step from you, may each automated deploy run open it to just that run's
> own GitHub runner address for a few minutes, then close it again?"
> A: "Yes, per-run window (Recommended)"

Lane IAC's design, which only narrows it:

1. **One /32, validated.** Each root takes `bootstrap_runner_cidr`, default empty and
   `sensitive`, which the `doks` module adds to the control plane's allowlist beside the founder's
   CIDRs. Its validation admits only the empty string or exactly one public IPv4 `/32`, with no
   leading-zero octet. It refuses a wider prefix, a list, IPv6, and the unspecified (`0.0.0.0`),
   private, loopback, link-local, shared (`100.64.0.0/10`), multicast and reserved ranges.
2. **Set only by `cargo xtask infra apply` on staging, for its own run.** No committed value or
   GitHub variable sets it, and a `plan`, a `preflight` or a production apply never opens one.
   The window opens in the same saved plan the run prints and applies (DEC-2014 item 4), so the
   founder's approval of the run covers it.
3. **Closed on every path, and read back.** The same command closes the window after `tofu`,
   whether the apply or the bootstrap succeeds or fails, and `infra.yml`'s last step, `if:
   always()` once the binary is built, closes it again when the first command was killed or
   cancelled; that step closes the window before it deletes the run's Spaces keys, and attempts
   each whatever the other did. Closing goes through DigitalOcean's API, not OpenTofu: it sets the
   cluster's control-plane firewall back to the founder's CIDRs, or to TEST-NET-1's
   `192.0.2.1/32` when there are none, which is what the `doks` module computes with no window,
   so the next plan sees no drift. It needs only the DigitalOcean token and
   `KUBE_API_ALLOWED_CIDRS`, the two secrets the last step holds, and it carries the cluster's own
   name, user tags and upgrade settings so nothing else changes. A close fails closed (the review
   of #1771): a cluster listing that is refused, unreadable or not one complete page is an error,
   never "no cluster"; a missing cluster after a successful apply is an error; a firewall that is
   off counts as open; and after the update the cluster is read back, for up to 30 seconds, until
   its firewall is on and admits exactly the closed addresses, or the run fails saying the window
   may be open. The log says the window is closed only then. A runner that dies before both closes
   leaves the window open until the next run closes it; that residual is disclosed here.
4. **The runner's address** comes from the echo service `https://api.ipify.org` and is checked by
   the same rule before it is used, documentation ranges refused too. It is masked, and so is its
   `/32`, before any `tofu` call. A service that lied could open a different public `/32` for the
   run, which still needs the cluster's credentials to do anything.
5. **The bootstrap** follows the applied plan, in the same command: the three workload secrets
   from OpenTofu's outputs, each wrapped in lane GO's `gitops::Secret`, which masks it, as it is
   read; cluster credentials from DigitalOcean's `credentials` endpoint asked for 900 seconds,
   refused unless their own `expires_at` is in the future and at most 900 seconds (and two
   minutes of clock skew) away, their token masked as it is read, written as a JSON kubeconfig at
   mode 0600 to `$RUNNER_TEMP` and deleted however the bootstrap ends; and
   `gitops::bootstrap_from_infra`, which confirms the context `do-nyc3-mandate-staging` before it
   applies anything (DEC-2018 item 7). `kubectl` and `flux` are installed by checksum in a step
   that holds no secret, and start without any of the secrets `infra.yml` hands the step in their
   environment.

### Alternatives considered

- **A self-hosted runner with a fixed address:** a machine to run and pay for.
- **Reaching the API through the tunnel and Access:** a second ingress path, which is lane GO's
  later hardening, not the demo's.
- **Opening a wider range (GitHub's published runner ranges):** thousands of addresses for minutes
  a run, which the founder was not asked about.

<a id="dec-2029"></a>

## DEC-2029: OpenTofu mints Barman's bucket-scoped R2 token, and each run first verifies the Cloudflare token's permissions

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | decided by the founder, [DEC-2605](process.md#dec-2605) row 12 (DEC-79: it widens a credential's scope). The design below is lane IAC's, under DEC-79, and only narrows the answer |
| **Story or claim** | E21-56 (lane IAC), claim [#1634](https://github.com/kunwarshivam/mandate/issues/1634); [DEC-2012](infrastructure.md#dec-2012) items 9 and 10 (amended here), [DEC-973](DEC-973.md) item 5 |

### Decision

The founder's answer, as relayed, with the option's description as it was shown:

> Q: "The database backup archive needs a Cloudflare R2 token scoped to one bucket. How should it
> be made?"
> A: "Let automation mint it". Description shown: "Your existing Cloudflare token gains 'API Tokens
> Edit'. Nothing by hand, but that permission can create any token, including one that lifts the
> archive's bucket lock, so it's admin-level."

Lane IAC's design, which only narrows it:

1. **The minted token.** The `cloudflare` module mints one account-owned token per environment,
   `<name>-journal-archive`. It has one `allow` policy, whose one permission group is Cloudflare's
   `Workers R2 Storage Bucket Item Write` (Object Read and Write). That group is looked up by name
   and scope, and a precondition fails the plan unless exactly one matches. Its one resource is
   the archive bucket (`com.cloudflare.edge.r2.bucket.<account>_default_<bucket>`). A test asserts
   exactly that, and fails on a token widened to the account.
2. **Its value is only a sensitive output.** Barman's S3 credentials are the token's id and the
   SHA-256 of its value (R2's derivation), as the sensitive outputs `archive_access_key_id` and
   `archive_secret_access_key`. They live in encrypted state (DEC-2012 item 5), are never printed,
   and reach the cluster only through lane GO's in-process bootstrap. This replaces DEC-2012 item
   9's hand-made token and item 10's "the only secrets OpenTofu's state holds are the tunnel's".
3. **`CF_INFRA_API_TOKEN` gains `Account API Tokens: Edit`.** The runbook lists it. As the founder
   was shown, that makes the token admin-level: whoever holds it can mint any token, including one
   that lifts the archive's lock, which is already governance-equivalent (DEC-973 item 5). It stays
   only in `infra-staging` (and later `infra-production`), behind the founder's approval of each
   run.
4. **Preflight: the token's permissions are checked before anything else** (lands with the
   preflight, [#1703](https://github.com/kunwarshivam/mandate/pull/1703), DEC-2030 item 5; until then a missing permission fails the
   plan or apply that needs it). `cargo xtask infra`
   first reads `CF_INFRA_API_TOKEN`'s own policies from Cloudflare's API. If any permission the run
   needs is missing, it stops before `init` and names each missing one, so a missing scope is never
   discovered halfway through an apply. The needed permissions are Tunnel, Access apps and
   policies, R2 storage, API tokens, DNS and zone read.

### Alternatives considered

- **The founder makes the R2 token once by hand** (lane IAC's recommendation): the founder chose
  automation.
- **A second Cloudflare token holding only API Tokens Edit:** no narrower, since that permission is
  admin-level whichever token holds it, and one more secret to rotate.

<a id="dec-2030"></a>

## DEC-2030: the founder's by-hand list shrinks to one secret and two console steps: the account id is read, Access admits the account's members, staging's control plane may rely on the run's window, each run mints its own Spaces key, a preflight names what is missing, and the public log carries no value

| | |
|---|---|
| **Status** | Accepted; item 1 superseded in part by [DEC-2043](infrastructure.md#dec-2043), and the `sha` input's rule by [DEC-2042](infrastructure.md#dec-2042) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the founder, for the DigitalOcean token's Spaces Keys scope (item 4; DEC-79: it widens a credential's scope), answering the coordinator at 22:09 UTC: [DEC-2605](https://github.com/kunwarshivam/mandate/pull/1692) row 13. The rest is lane IAC's, under DEC-79, at the coordinator's direction (Claude Code), and only narrows: no address, identity or credential gains access it did not have |
| **Story or claim** | E21-56 (lane IAC), claim [#1634](https://github.com/kunwarshivam/mandate/issues/1634); amends [DEC-2012](infrastructure.md#dec-2012) items 5, 7 and 8, [DEC-2014](infrastructure.md#dec-2014) item 2 and [DEC-2029](infrastructure.md#dec-2029) item 4; with [DEC-2028](infrastructure.md#dec-2028) |

### Context

The runbook asked the founder to make a Spaces bucket and key, copy an account id, list their
email and their address, and store five secrets and three variables. The founder asked for
staging to need as little by hand as possible, with nothing loosened.

### Decision

1. **The Cloudflare account id is read, never configured.** Each root reads the accounts
   `CF_INFRA_API_TOKEN` can see (`data "cloudflare_accounts"`). A postcondition fails the plan
   unless it sees exactly one, so a token that reaches two accounts never picks one silently. The
   variable `CF_ACCOUNT_ID` is gone.
2. **Staging's Access admits the members of the founder's own Cloudflare account**, through
   Access's `cloudflare_account_member` rule on that account, in place of a list of emails. No
   email is in a variable, the repository or state for staging, and there is no identity provider
   to set up. The coordinator's first idea, an Access rule on one GitHub login, does not exist:
   Access's GitHub rule is an organisation and team, and a GitHub identity provider needs a GitHub
   OAuth application made by hand, with a secret of its own. **Production keeps its named emails**
   (`ACCESS_ALLOWED_EMAILS`, now an environment secret, at least one, each a full address), as
   DEC-2012 item 8 set it: account membership would admit any future member of the Cloudflare
   account to production, a loosening DEC-79 leaves to the founder, so it is not proposed. The
   `cloudflare` module takes the emails and uses the membership rule only when it is given none,
   which only staging's root does.
3. **Staging's control plane may rely on the run's window alone** (DEC-2028; DEC-2605 row 11).
   Staging's `KUBE_API_ALLOWED_CIDRS` may be unset or empty. Production's root still refuses an
   empty list. With no founder CIDR and no window, the `doks` module sets the firewall to
   `192.0.2.1/32` (TEST-NET-1, RFC 5737, which no internet host holds). So the firewall stays on and
   admits nobody, and an empty list never reaches DigitalOcean, where it could read as no
   restriction. Tests cover both, and fail when the fallback is removed.
   An unset or empty `KUBE_API_ALLOWED_CIDRS` is left out of `tofu`'s environment, so staging's
   root takes its empty default; `""` is no list and is never passed.
4. **Each run mints its own Spaces key and makes the state bucket if it is missing** (lands
   with [#1703](https://github.com/kunwarshivam/mandate/pull/1703); the
   founder: DEC-2605 row 13, "Yes, add Spaces Keys (Recommended)").
   - `DIGITAL_OCEAN_API_KEY` gains Spaces Keys create, read and delete.
   - `cargo xtask infra` mints `mandate-infra-<env>-<run id>`, with full access, since only that
     grant can create a bucket. It hands the key to OpenTofu only in the child process's
     environment.
   - If `mandate-tofu-state-<env>` is missing, it creates it with a signed S3 request; OpenTofu
     then adopts it (DEC-2012 item 5), and it stays private, versioned and `prevent_destroy`.
   - The key lives in a guard that deletes it when the run ends: on success, on an error and on a
     panic. The Cloudflare half of the preflight runs before the mint, so only the state bucket's
     check can fail while a key exists. The mint's `POST` is sent once, never retried, so a lost
     answer never mints a second key.
   - `infra.yml`'s last step, `if: always()` with only `DIGITAL_OCEAN_API_KEY` and
     `KUBE_API_ALLOWED_CIDRS` (the second for DEC-2028's window), runs `cargo xtask infra close`, which deletes every `mandate-infra-<env>-*` key, so a run whose main step
     was killed or cancelled leaves none. Each run's preflight also deletes any left over; the
     workflow's per-environment concurrency group means no other run of that environment can
     hold one.
   - Every credential reaches `curl` as a config on its standard input, never on its command line.
     Nothing prints a key.
   - The residual, disclosed: a runner that dies outright, so that not even the `always()` step
     runs, leaves its key until the next run of that environment.
   - `SPACES_ACCESS_KEY_ID` and `SPACES_SECRET_ACCESS_KEY` are gone, and with them the founder's
     by-hand bucket. This replaces DEC-2012 item 5's "the founder makes that bucket and the Spaces
     key once, by hand".
5. **One preflight names exactly what is missing, with no values** (lands with
   [#1703](https://github.com/kunwarshivam/mandate/pull/1703)). `cargo xtask infra
   preflight`, and the start of every `plan` and `apply`, prints a checklist and stops before
   `init` if any line is missing:
   - the secrets `TOFU_STATE_PASSPHRASE` (at least 32 characters), `DIGITAL_OCEAN_API_KEY` and
     `CF_INFRA_API_TOKEN`, each by name;
   - production's `KUBE_API_ALLOWED_CIDRS` and `ACCESS_ALLOWED_EMAILS`;
   - DigitalOcean: the token accepted, then Kubernetes, VPC and firewall read, and Spaces Keys
     read and create, each probed by its own call, create by the run's own mint. Spaces Keys
     delete is checked when the run deletes its key (the guard, then `infra close`), and by the
     preflight only when an earlier run left a key behind; the write scopes for Kubernetes, VPC and firewall cannot be probed without making
     something, so the runbook lists them and `apply` names the scope DigitalOcean refuses;
   - Cloudflare: exactly one account; the token's own permission groups, read from its own
     record, against Tunnel, Access apps and policies, Workers R2 Storage, DNS, Zone read and API
     Tokens (DEC-2029); a token that cannot read its own record lacks API Tokens Edit, and is
     named as such;
   - R2 enabled on the account, read from R2's bucket list, which Cloudflare refuses until it is.

   It reminds the founder of the one step it cannot read, the DigitalOcean billing alert at $120
   (DEC-2012 item 12). `TOFU_STATE_PASSPHRASE` stays the one stored secret that is not a cloud
   token, and state encryption is unchanged (DEC-2012 item 5).

6. **The public log carries no value** (the coordinator's security audit; the repository is
   public, so its Actions logs are too).
   - `tofu` runs with its output captured. The log gets each command line, a failure's
     diagnostics (OpenTofu redacts every sensitive value in them), and, after `plan`, one line per
     resource change: its actions and address, read from `tofu show -json` as untyped JSON. No
     diff and no `Outputs:` block is printed, and the plan file is never uploaded.
   - The founder's CIDRs, the run's window, production's Access emails and the Cloudflare
     account id are `sensitive` variables, and the outputs built from them are sensitive outputs.
   - `KUBE_API_ALLOWED_CIDRS` and `ACCESS_ALLOWED_EMAILS` are environment secrets, not variables:
     GitHub prints a step's environment in the public log, masking only secrets. GitHub masks a
     secret only as the whole string it stored, so before any `tofu` call `infra` also masks each
     listed CIDR, its bare address, and each listed email on its own.
   - `cargo xtask ci tofu` reads each module's provider schemas and fails unless every
     string-valued attribute whose name contains `token`, `secret`, `password`, `private_key`,
     `kube_config`, `raw_config`, `access_key` or `api_key`, or is `value`, in every type the tree
     uses, is `sensitive`. Names that end in `_id`, `_url`, `_lifetime` or `_header` say what a
     credential is, not hold one, and are not held to it. The reviewed exceptions are a DNS
     record's `data.value` and a node taint's `value`; a seeded removal of one fails the check.
   - Every value fetched at run time, starting with the run's Spaces key, is masked with
     `::add-mask::` in its raw, base64, base64url, URL-encoded, JSON-escaped and per-line forms
     before anything else touches it. Each command's data is escaped as the runner decodes it
     (`%` to `%25` first, then CR to `%0D` and LF to `%0A`), so a `%` or a line break never
     registers a different mask.
   - The secret step runs `target/debug/xtask infra`, the binary an earlier step built with no
     secret in its environment, so no build script runs beside a secret.
   - The dispatch takes `sha`, the full commit the founder reviewed. The run refuses unless it is
     the run's commit and the checkout, so an approval binds to that commit. Each root records the
     commit it applies (`terraform_data.applied_commit`), and each run lists the files under
     `deploy/tofu/`, `infra.yml`, `xtask/`, `Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml` and
     `.cargo/` changed since the last applied one. That `tofu
     output -raw applied_commit` is the one output the workflow reads; it is not a secret. This
     amends DEC-2012 item 10's "runs no `tofu output`".

### What is left by hand

- Make two tokens and store them in `infra-staging`: `DIGITAL_OCEAN_API_KEY` and
  `CF_INFRA_API_TOKEN`.
- Store one passphrase there: `TOFU_STATE_PASSPHRASE`.
- For production only, store `KUBE_API_ALLOWED_CIDRS` and `ACCESS_ALLOWED_EMAILS` as secrets in
  `infra-production`.
- Two console steps that no API can do: the billing alert, and enabling R2 once.
- Approve each run.

### Alternatives considered

- **A GitHub identity provider for Access:** an OAuth application and a client secret made by hand
  (item 2).
- **An empty control-plane allowlist passed to DigitalOcean:** whether an enabled firewall with no
  address admits everyone or no one is not documented, so it is never sent (item 3).
- **A Spaces key scoped to the state bucket:** it cannot create the bucket, and the run still
  needs the bucket's settings, so it would need a second, broader key anyway.

<a id="dec-2031"></a>

## DEC-2031: The executor's interim image in the local and CI cell, on its fixture paper broker

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176; paper and fixtures only, and nothing loosens a rule) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | `claude-code` (builder of lane X, the executor slice of E21-30), under DEC-79 and DEC-176 |
| **Story or claim** | E21-30, claim [#1498](https://github.com/kunwarshivam/mandate/issues/1498) (lane X, slice X3); infrastructure design §3.1, §3.4; DEC-1013, DEC-1171, DEC-1172, DEC-2003, DEC-2007 |

### Context

DEC-2003 built the executor process and DEC-2007 its binary, `mandate-executord`, whose broker is
the in-process fixture paper broker in every mode. The cell's process list still marked the
executor `built: false`, so the deployment manager had no executor pod template to start from, and
the cell built no executor image. DEC-1013 settled the same question for the runtime with an interim
image.

### Decision

1. **An interim `executor` image.** `deploy/cell/images/executor.Dockerfile` holds the statically
   linked `mandate-executord` on an empty base, non-root, with no shell and no other file, as
   DEC-1013's runtime image does. Lane R2 (E21-35) replaces it with its reproducible, signed build.
2. **`cargo xtask cell up` builds and loads it.**
   - It adds `mandate-executord` to the static build, never with the `live` feature.
   - It builds the image from a context holding only the binary, imports it into k3d, and sets
     `images.executor`.
   - Where the `mandate` image is pulled, the executor is left out, as the runtime is.
3. **`processes.json` marks the executor built.** It names the executor's own probe
   (`mandate-executord probe livez|readyz --probe /tmp/process.sock`).
   - **Its pod template.** The chart already renders the executor's managed-pod template once
     `images.executor` is named:
     - `run --probe /tmp/process.sock`;
     - `MANDATE_JOURNAL_DSN` for the `mandate_executor` role;
     - the deployment manager's `MANDATE_PROCESS`, `MANDATE_WORKSPACE` and `MANDATE_SUBJECT`, which
       the binary's launch reads (DEC-2007 item 3).
   - **Who starts it.** The manager starts an executor once it reads an account's version-2
     deployment (DEC-1171).
4. **No Compose service yet.** The executor is `compose: false` until a Compose adapter names its
   account, a later slice. On one host the executor waits for that adapter, as the deployment
   manager does (DEC-1172).
5. **The demo stays honest.** With the runtime and executor both built, `cargo xtask cell demo`
   still reports the paper agent and research parts as skipped, each with its reason:
   - the runtime proposes no order on the deny-all wiring until the post-R0 hook (DEC-1013);
   - the executor takes no order before slice X1b.

   The paper fill and protective stop stay a named gap until they run; they are never faked.
6. **No broker host is reached.** The pod's `broker` egress stays declared, but the binary compiles
   no host transport and reads no credential (`AGENTS.md` rules 7 and 8).

### Rationale

It follows DEC-1013's path for the runtime: an interim image built from the checkout lets the cell
run the process now, and lane R2's signed build replaces it without changing the chart.

### Alternatives considered

- **Leave the executor unbuilt until X1b.** Rejected: the deployment manager, the probes and the
  executor's journal role would go unexercised in the cell for no gain.
- **Add the Compose service now.** Deferred: it needs an adapter that names the account from the
  operator's environment, which is its own change.

<a id="dec-2032"></a>

## DEC-2032: A leak-guard over CI's workflows and scripts, part of lint

| | |
|---|---|
| **Status** | Accepted; L10 specified by [DEC-2055](infrastructure.md#dec-2055) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | `claude-code` (lane SI) at the coordinator's direction, after the public-repository secrets audit. It is a reversible engineering choice under DEC-79 and DEC-176: it only tightens what CI may run, and it loosens no rule |
| **Story or claim** | The public-repository secrets audit (2026-10-18); companion workflow fixes by lane CT (#1700, #1705, #1707); IAC's tofu rules L4 and L11 and RS's `release.rs` are not this decision's |

### Context

The repository is public, so every CI log and every artifact is public too. A secret can leak
without anyone printing it on purpose:
- by an environment dump;
- by a shell trace;
- by a context pasted into a script;
- through an artifact or a cache;
- by a verbose HTTP client;
- through the runner's files;
- by a build step that runs code from a pull request while holding a secret.

Review alone has not caught each of these: the audit found several on `main`.

### Decision

1. **`cargo xtask leak-guard`, run by `lint`.** It reads every workflow (`.github/workflows/*.yml`
   and `*.yaml`) and every script under `.github/scripts/`, and refuses the lines each rule names.
   It refuses the shapes its rules name. A refused line stays refused until a person lists it in
   `xtask/src/leak.rs`'s `ALLOWED`, by rule, file and trimmed line, byte for byte, in a change a
   reviewer sees. A test fails when an allowed line is no longer in its file. A text match is never
   complete, so L1 to L3 are defence in depth. The closed world is L9, a secret step that runs only
   a prebuilt command, together with no secret in a pull request's jobs.
2. **The rules.** Each rule has a fixture that must fail and one that must pass. A seeded-bug test
   appends each failing fixture to each real file and requires the guard to refuse it there.
   - **L1, environment dumps:** `printenv`, `declare`, `typeset`, a bare `export` or
     `export -p`, a bare `set`, an `env` with no command (only options such as `-u HOME` or
     `-C /tmp`, assignments, or a redirect), `compgen -e` or `-v`, and `/proc/*/environ`.
   - **L2, trace and debug output:**
     - `set` with any flag holding `x` or `v`, or `-o xtrace` or `-o verbose`;
     - a shell run with `-x`, `-v`, `-o xtrace`, `-o verbose`, `--xtrace` or `--verbose`, and a
       shebang with `-x` or `-v`;
     - `PS4=`, `SHELLOPTS`, `BASH_XTRACEFD` and `BASH_ENV`, set inline or in `env:`;
     - `ACTIONS_STEP_DEBUG`, `ACTIONS_RUNNER_DEBUG`, `GH_DEBUG` and `TF_LOG`;
     - `RUST_LOG` at `trace`;
     - `helm --debug`, `kubectl` above `--v=5`, `doctl -v` and `cosign -d`.
   - **L3, contexts:**
     - `toJSON(` whose argument names `secrets`, `env`, `github`, `vars`, `inputs`, `steps`,
       `needs`, `matrix`, `runner`, `job` or `strategy` anywhere, however nested;
     - the whole `secrets` context anywhere, in an `env:` block too: `${{ secrets }}`, `join(`,
       `format(` and `secrets.*`;
     - `secrets[`;
     - `secrets: inherit`, in any case and spacing;
     - `secrets.` anywhere but an `env:` block, so a secret reaches a step only as a variable.
   - **Secret jobs**, which L5, L6 and L8 read: a job is one when it has an `environment:`, reads
     `secrets.` or `secrets[` in any case, holds `id-token: write`, or reads `github.token` while it
     grants a write permission. A read-only `github.token` in a pull request's job gives nothing
     the pull request's own code does not already hold. A job key with a trailing comment is a job
     of its own. A step is read whole, so its `with:` counts before or after its `uses:`.
   - **L5, artifacts:**
     - each `upload-artifact` or `upload-pages-artifact` step names its paths, in block, `|` block
       or flow style, and each path is in a reviewed list, `ARTIFACT_PATHS`, by workflow and path;
     - no path may name `deploy/tofu`, `.terraform`, a plan, state, a key, a secret, a token, a
       kubeconfig, an `.env`, auth or a credential;
     - `include-hidden-files` at any value but `false`, in any case and quoting, is refused;
     - `gh release upload` is refused.

     An upload in a secret job is allowed for a reviewed path, as release.yml's signing needs.
     That is the scope: the path list, not the job, is what a reviewer reads.
   - **L6, caches:** a secret job restores and saves no cache. That means no `actions/cache`,
     `rust-cache`, `sccache-action` or `actions/setup-go` (which caches unless `cache: false`), and
     no `cache`, `cache-dependency-path`, `enable-cache`, `cache-from`, `cache-to` or
     `bundler-cache` input at any value but `false`, in block or flow style. Nor may a run use
     `--cache-from` or `--cache-to`. A cache a pull request wrote must never run beside a secret.
   - **L7, curl and tools:**
     - `curl` refuses `-v`, `--verbose`, `--trace*`, `-i`, `--include`, `-D`, `--dump-header`
       and `--aws-sigv4`, read inside combined short options (`-sSfLv`) and with attached values
       (`-D-`), as well as in long form;
     - a `curl` `-H` or `-u` value must be an `@file`, attached (`-ualice:…`) or not, and in a
       combined option (`-sSu`) too;
     - `docker login -p` is refused;
     - any `--password` other than `--password-stdin` is refused.

     L7 reads every job, not only secret ones, and holds no list of hosts. That is the scope: a
     header or user written inline is refused wherever it goes.
   - **L8, runner files and workflow commands:**
     - a secret job, and every `.github/scripts/` file it runs, writes no `GITHUB_ENV`,
       `GITHUB_PATH`, `GITHUB_OUTPUT`, `GITHUB_STEP_SUMMARY` or `GITHUB_STATE`;
     - it issues no `::` workflow command but `::add-mask::`, in any case, and after an escaped
       `\n` or `\r` inside a `printf` or `echo -e` string too;
     - anywhere, an `::add-mask::`, in any case, is refused unless it is a reviewed line that escapes its value
       first, `%` to `%25` and then CR to `%0D` and LF to `%0A`. An unescaped value lets data
       end the command early.
   - **L9's canonical subset, read first over every workflow:** the line readers above see the
     structure GitHub does only when a workflow is written in one plain shape, so any line outside
     it is refused:
     - the top level is at column 0 and each level exactly two spaces in, with no tab;
     - a `- ` item has one space after its `-`, no more;
     - no file holds a line break the line readers don't split on: a lone CR, U+0085, U+2028
       or U+2029. Scripts are held to this too;
     - every line outside a block scalar is a bare `key:` (`[A-Za-z0-9_-]+`, then `: ` or the
       end of the line, nothing quoted, no space before the colon) or a `- ` item;
     - there is no anchor, alias, tag or merge key;
     - there is no flow collection but `{}` and a flat `[a, b]`;
     - no scalar runs on to the next line;
     - a quoted scalar closes on its own line, and a double-quoted one holds no backslash escape;
     - no `${{` is without its `}}` on the same line.
     - no key holds a capital letter, except in a mapping of names, judged by its position, not
       its key: job ids under the top-level `jobs:`, a job's service ids, and an `env:`, `with:`,
       `inputs:`, `outputs:`, `secrets:` or strategy `matrix:` where each is the keyword at its
       place. GitHub's keywords are lower case, so `Jobs:`, `Uses:` or `Shell:` in any other case
       is refused, whatever GitHub makes of it, and a job whose id is `env` holds keywords;
     - no mapping holds a key twice;
     - `env:`, `with:`, `secrets:` and `defaults:` are literal block mappings (or `{}`), never a
       scalar or an `${{ … }}` expression, so every name and input is read.

     A YAML parser is not used, because its reading can differ from GitHub's; the subset is one
     both read alike.
   - **L9, the closed world for a secret step, and pinned actions:** L9 is an allowlist, not a
     list of shapes.
     - A step is a *secret step* when a `secrets.` or `secrets[` reference, in any case, reaches
       it: from its own `env:`, `with:` or `run:`, from its job's lines outside its steps (the
       job's `env:`, or a reusable workflow's `with:` or `secrets:`), or from the workflow's
       top-level `env:`.
     - A secret step, and every step after the first one in its job, is refused unless:
       - its own `run:` is exactly one line of `xtask/src/leak.rs`'s `SECRET_RUNS` for its
         workflow, never a block;
       - its own `uses:` is one of xtask's reviewed pins, `READ_ACTIONS` (DEC-873);
       - it sets no `shell:` or `working-directory:`;
       - its `env:` sets only the names its `SECRET_RUNS` entry lists, or, for a later step,
         those its workflow's `SECRET_JOB_ENV` lists. So no `PATH`, `LD_PRELOAD`, `SHELL`,
         `NODE_OPTIONS` or `BASH_ENV` reaches it.

       So no `;`, `&&`, `|`, `$(`, wrapper, script or build reaches a secret but a reviewed line.
       A step's own keys are those at its item's key column, which the canonical subset makes
       exact. So a `run:` inside its `env:`, or inside a `name: |` block, is not its own.
     - A job holding a secret step:
       - sets no `container:`, `services:`, `shell:` or `working-directory:` anywhere;
       - sets no `defaults:` but its workflow's one reviewed block in `SECRET_JOB_DEFAULTS`
         (`release.yml`'s `shell: bash`, `web-deploy.yml`'s `working-directory: web`);
       - sets a job-level `env:` of only `SECRET_JOB_ENV`'s names.

       Its `strategy:`, `matrix:` and `outputs:` are literal mappings, never an expression. Its
       workflow sets no top-level `defaults:`, and a top-level `env:` of only those names.
     - Every step before the first secret step of such a job uses one of `READ_ACTIONS`'s pins or
       runs one `PRE_RUNS` entry for its workflow, a block compared whole. The entries are pinned,
       checksum-verified tool installs, the repository's own toolchain, the lockfile's build of
       the binary the secret step runs, and `web-deploy.yml`'s reviewed npm build. So no earlier
       step can replace what the secret step runs. IAC's #1771's kubectl and flux install is
       listed with them.
     - In such a job, every reviewed action's `with:` inputs, before the secret step and after
       it, are `STEP_WITH`'s for that action in that workflow, key and value exactly. So a
       checkout takes no `repository:`, `ref:`, `path:`, `token:` or `submodules:`, and no input
       can point a reviewed action at other code.
     - A step before the secret step sets only `PRE_ENV`'s `env:` names, so no `RUSTC_WRAPPER`
       or `PATH` reaches the build.
     - A folded (`>`) block never matches a reviewed literal block.
     - Files are read with CRLF line ends normalised to LF.
     - A secret job runs no local `./` action, whose steps are not read here.
     - `SECRET_RUNS` holds main's five secret runs, each a binary or script built or checked out
       before any secret is in reach:
       - `infra.yml`'s `target/debug/xtask infra` and `infra close` (#1703);
       - `r2-lockcheck.yml`'s `target/debug/examples/r2_finding`;
       - `release.yml`'s `.github/scripts/release-sign.sh` (DEC-2024);
       - `web-deploy.yml`'s `npx wrangler deploy`, which becomes `node_modules/.bin/wrangler` when
         CT's #1743 lands.

       A test fails when an entry is no longer in its file.
     - A job that is secret only by `id-token: write` or a writable `github.token` has no secret
       step, so L9 does not bind it. `release.yml`'s `publish` jobs build with `id-token: write`
       on a push to `main` only, after the change merged, as DEC-2024 sets out;
     - every `uses:`, in every workflow, is pinned to a 40-character commit, except a local
       `./` action and a `docker://` image. `codeql.yml`'s three tag pins (`@v7`, `@v4`) are
       pinned in the same change: `actions/checkout` to v7.0.1, as every other workflow already
       is, and `github/codeql-action` to v4.39.0, the commit `v4` named on 2026-10-18.
   - **L12, triggers:**
     - `pull_request_target` and `workflow_run` appear only in `merge.yml`;
     - no workflow triggers on `issue_comment`, `issues`, `pull_request_review`,
       `pull_request_review_comment`, `discussion`, `discussion_comment` or `repository_dispatch`,
       events anyone can raise. Trigger keys and the `on:` key are read quoted or not;
     - `merge.yml` names no `head.sha`, `head.ref` or `head_ref`, so it never checks out a pull
       request;
     - a workflow with an `environment:` triggers only on `workflow_dispatch` or a push whose
       branch list is exactly `[main]`, in either list style, with no tags and no ignored
       branches;
     - each job with an `environment:` has a job-level `if:` that holds
       `github.ref == 'refs/heads/main'` alone or joined to other terms by `&&`. An `if:` with
       `||`, `!=` or any negation is refused, so no other term can widen it.
     - in a workflow `SECRET_RUNS` names, each job holding a secret step names an `environment:`,
       so the gate above binds it.

     `release.yml` held the `release-signing` environment and also ran on pull requests. Its
     pull-request run moves to a new `release-check.yml`: the same two builds an arch, `compare`,
     and the image drills, with no environment, no secret and no upload. `release.yml` now runs
     only on a push to `main` and on dispatch. Signing stays in `release.yml`, because its
     `needs: build-a-*` digest comparison (DEC-2024 item 3) cannot reach across workflows. DEC-972
     item 3's pull-request check is unchanged in substance; only its file is new. xtask's
     `only_the_dispatched_signing_job_holds_the_release_key` now also requires that no other
     workflow names the release key or its environment, and that `release-check.yml` holds no
     secret.
   - **Later PR under this decision:** L10, Rust in xtask and examples, with `::add-mask::` written
     only through one escaping helper.
3. **How lines are read.** The guard reads text, not a parse tree, so it holds only what its rules
   name, and a bypass found later is a defect, fixed with a regression test.
   - A line is split into commands at shell separators, braces and `!`, and leading shell keywords
     (`if`, `then`, `do`, …) and variable assignments are dropped.
   - Quotes are dropped rather than parsed, so a refused word inside a string is refused too.
   - A leading YAML key, a leading `\`, and the wrappers `sudo`, `exec`, `command`, `nohup`,
     `time`, `env`, `timeout` and `xargs` are stepped over, with their options and the values those
     options take.
   - A redirect ends a command's arguments, so `env > out.txt` is a bare `env`.
   - A shell's `-c` program is read as a command of its own.
   - GitHub expressions are read whole and without regard to case: each `${{ … }}`, joined
     across lines, a `}}` inside a quoted string not ending it, and each `if:` value.
   - Full-line comments are not read, except a shebang.
   - The #1724 reviews' probes are kept as `BYPASSES`, each refused.
   - The shapes the second review found that the guard does not yet read are in the backlog, under
     E21-52.
4. **What is allowed today.** Each item is a reviewed line in `ALLOWED`, with its reason there.
   - `no-credentials.sh`'s `compgen -e`, which lists names, never values, and prints only those
     that look like a credential's (DEC-964).
   - `merge.yml`'s `toJSON` of the pull request numbers a `workflow_run` names, which the event
     holds in public.
   - `release.yml`'s `images/test-key/cosign.pub` artifact, the public half of the run's
     ephemeral drill key.
   - `merge.yml`'s merge job, whose `github.token` may write, writing the fixed `merged=true` as
     its output.
   - `release.yml`'s two `publish` jobs, which hold `id-token: write`, writing the build's printed
     digest as their output, the public digest `compare` checks.

### Alternatives considered

- **A third-party workflow scanner** (zizmor, actionlint's shellcheck pass): it does not know this
  repository's reviewed exceptions, and it would be a new dependency. actionlint already runs in
  lint and stays.
- **A YAML parser:** most rules are about shell text inside `run:` values, which a YAML parser does
  not split into commands. The `env:` block detection reads indentation, which is enough for the
  workflows' own style.

<a id="dec-2033"></a>

## DEC-2033: The executor process hands its agents' proposed orders to the core, exits first and openings only with their protection

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176; paper and fixtures only, every opening still passes the binding gate, and nothing loosens a rule; item 4 records a hold against rule 13 as a gap for E7-29 to close, not as a reading of the rule; item 4 superseded for exits with no limit by [DEC-2037](DEC-2037.md)) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | `claude-code` (builder of lane X, the executor slice of E21-30), under DEC-79 and DEC-176 |
| **Story or claim** | E21-30, claim [#1498](https://github.com/kunwarshivam/mandate/issues/1498) (lane X, slice X1b); infrastructure design §3.4; journal spec §5.2, §9.1 (`IntentProposed`); trading spec §9; DEC-85, DEC-2003, DEC-2007; `AGENTS.md` rules 3, 5, 13 |

### Context

DEC-2003's executor process applies its agents' kill switches but takes none of their orders. In
one process the runtime hands the core an `IntentHandoff`. Across processes, the runtime journals
each order it proposes as `IntentProposed` on the agent's stream and the executor must take it from
there. The record carries the order's instrument, side, quantity, limit, time in force, and
purpose, but not the protective prices of an opening, which the runtime derives from the mandate.

### Decision

1. **Each proposal is handed once, after the switches.** In every pass, after the kill switches and
   the tick, the process hands each `IntentProposed` of a followed agent's stream that the core's
   intent fold does not yet hold, in each agent's stream order, as the handoff the runtime would
   make. The core's fold is the only record of what was taken, so a restart takes nothing twice.
   - **Exits** (a purpose that does not add risk) go whenever the lifecycle permits a risk exit,
     ready or not (`AGENTS.md` rule 13).
   - **Openings and increases** go only when the lifecycle permits an opening, that is once the
     startup reconciliation has run (infrastructure design §3.4).
2. **A later stop drops a waiting opening.** An opening or an increase that a later
   `KillSwitchActivated`, or an `AgentModeChanged` to anything but `normal`, follows on the same
   stream is never handed. An exit is never dropped this way (rule 3: ambiguity never adds risk).
3. **An opening carries its mandate's protection.** A `Protections` port gives the protective
   prices for an entry at the opening's limit, or none when the mandate turns protection off.
   - A port that cannot answer stops the process, rather than hand an opening unprotected (TI-4).
     The restart asks again.
   - The binary's fixture wiring answers none: its gate holds every opening anyway (item 5).
4. **An owner's exit at the market goes as a limit at its confirmed floor.** The core takes only
   limit orders from a handoff. The process prices the exit at the `floor` of the
   `OwnerExitRequested` on the proposal's causation chain: the worst price the owner confirmed they
   accept, so it sells as reliably as possible within their consent and never below it. The bid
   would sit above it and fill less surely. The floor counts only when that request's `scope` is
   `instrument` and its `subject` is the exit's instrument; any other request confirms no floor
   for it.
   - **A recorded hold until E7-29.** An exit at the market with no limit to give the core, an
     owner's with no confirmed floor (journal spec §9.1 rule 12) or any risk or discretionary exit
     at the market (rule 3 allows them), is held: it is neither dropped nor refused, and not
     silent. The process raises one opaque alert naming the intent, once per run
     (`owner_exit_unpriced` for an owner's exit, `exit_unpriced` for another), asks again every
     pass, and keeps applying switches and handing the other exits.
   - **This hold is a gap against rule 13**, which lets only `paused` or `stopped`, an `Unknown`
     order or the broker hold an exit. It is recorded here, not accepted as a reading of the rule.
     It closes when backlog story E7-29 lands market handoffs in the core, the lane's next story
     after this stack.
5. **A handoff the gate has no input for is held, never a stop.** When the core refuses a handoff
   for a missing binding-gate input, nothing is journaled, the process asks again every pass, and
   later proposals, exits included, still go. It raises one opaque alert per intent per run,
   naming only the intent id (rule 6):
   - **`opening_held`** for an opening or an increase without the gate's facts.
   - **`exit_held`** for an exit, an alarm that must never sound. An exit needs no gate fact, but
     the core's `intent::received` refuses any handoff, an exit included, whose agent the mandate
     port knows no version for, and `gate::binding_checks` refuses a reducing order whose snapshot
     names no or another mandate version. [DEC-2034](infrastructure.md#dec-2034), which lands with this
     decision, closes both: the process answers each followed agent's deployed version from its
     `AgentDeployed` version 2, which every followed agent has, and `binding_checks` keeps the
     local decision for a reducing order (rule 13).
   - Any other input the core refuses for it stops the process as every refusal does. This
     process never hands an opening before the startup reconciliation, the only time the core
     holds one, so no resumed opening reaches that refusal; the classification is a pure function
     tested on constructed inputs.
6. **A proposal that does not read stops the process** rather than be skipped (DEC-85), after the
   readable switches are applied.
7. **The in-process fixture is the only broker the executor reaches.** `xtask/layers.toml` allows
   `mandate-executord` no external crate of its own but `tokio`, forbids every crate that carries
   a transport to a host or a simulator, and gives it no `live` feature, which only the paper
   runner may declare. `mandate-alpaca`, whose client it uses over the fixture, links `reqwest`
   for its paper HTTPS transport; no source of the executor names that transport or the HTTP
   client. Tests pin both (`AGENTS.md` rule 8).

### Slices

- **X1b-1:** items 1, 2, 4, 6 and 7 for exits; an opening waits.
- **X1b-2:** openings, items 3 and 5.

### Rationale

Handing the journaled proposal as the in-process handoff keeps one path into the core: the same
receipt, gate, write-before-acting and protection sequencing as the runtime's own process.
Exits first follows rule 13; an opening needs its protection and the gate's facts, so it waits for
the slice that brings both.

### Alternatives considered

- **Journal the protective prices in `IntentProposed`.** Deferred: a journal record change for the
  runtime's writer, which the post-R0 hook owns.
- **Stop the process on a missing gate fact.** Rejected: one opening without facts would crash-loop
  the executor and hold back every exit behind it (rule 13).
- **Price an owner's market exit at the confirmed bid.** Rejected: the bid fills less surely than
  the floor, and the floor is what the owner accepted.
- **Refuse, or hold in silence, an owner's market exit without a floor.** Rejected: rule 13 never
  refuses an owner exit and lets only the agent's mode, an `Unknown` order or the broker hold one.

<a id="dec-2034"></a>

## DEC-2034: No exit waits on a mandate version: the executor reads each agent's deployed version, and the gate keeps an exit's local decision when the snapshot does not bind

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-176: a tightening of rule 13's reading; it removes refusals of risk reduction and adds no action path; the founder may revert) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | `claude-code` (builder of lane X, the executor slice of E21-30), as the coordinator directed |
| **Story or claim** | E21-30, claim [#1498](https://github.com/kunwarshivam/mandate/issues/1498) (lane X, slice X1b-2b); [trading spec §9.1](../../specs/trading-domain.md#91-evaluation-order-and-reason-codes); journal spec §9.24 (`AgentDeployed` version 2); DEC-2033 item 5; `AGENTS.md` rules 3 and 13 |

### Context

DEC-2033 item 5 found two core paths that can refuse an exit for a missing binding-gate input:

- `intent::received` refuses any handoff whose agent the mandate port knows no version for, since
  `IntentReceived` carries that version as its `man` configuration reference. The binary's fixture
  mandate port knows none, so in the binary every exit sounded `exit_held`.
- `gate::binding_checks` refuses a reducing order whose snapshot names no mandate version or
  another one, or lacks a configuration reference. It already keeps the local decision for a
  reducing order when the snapshot is missing, or when `mandate-risk` cannot evaluate it.

Rule 13 says risk reduction is never denied for want of these inputs, and §9.1 reads none of them
to deny an exit.

### Decision

1. **The executor process supplies each followed agent's deployed version.** It follows only
   agents with a version-2 `AgentDeployed` for its account, and that record carries the agent's
   `mandate_version` (journal spec §9.24, rule 22). The process answers the core's mandate
   version from that record, the last one for the agent, and asks the wiring's port only for an
   agent it does not follow. So `intent::received` always has a version for a followed agent's
   exit, and `exit_held` stays the alarm that must never sound.
   - The executor's fold does not interpret `MandateVersionApplied` yet (E6-4), so no account
     stream this executor reads carries a later version; when it does, that change reads it too.
2. **The gate keeps a reducing order's local decision when the snapshot does not bind.** In
   `gate::binding_checks`, a snapshot whose mandate reference is missing or differs from the
   resolved version, or that lacks a configuration reference, refuses an opening or an increase
   (`BindingGateInputMissing`, as before) and leaves an exit's account-stream decision standing,
   as a missing snapshot already does.

### Rationale

Both changes only stop risk reduction from being refused for inputs §9.1 does not read to deny it.
The deployed version is the one the agent runs under, and an opening still fails closed on any
mismatch.

### Alternatives considered

- **Let `intent::received` write an exit without a `man` reference.** Rejected: the journal
  requires the reference on every gated draft, and the process can always supply the true one.
- **Keep the fixture port's empty answer and rely on `exit_held`.** Rejected: an alarm is not an
  exit; rule 13 needs the exit to go.

<a id="dec-2035"></a>

## DEC-2035: The cell runs the executor's connect check as a one-off Job, chosen by `run.process`

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision; it adds no credential, no egress beyond Postgres, and no broker but the in-process fixture; the founder may revert) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | `claude-code` (builder of lane X, the executor slice of E21-30) |
| **Story or claim** | E21-30, claim [#1498](https://github.com/kunwarshivam/mandate/issues/1498) (lane X, slice X3b-1); [infrastructure §3.1](../../design/infrastructure.md); connections spec §8.1; DEC-993; DEC-2007 |

### Context

The cell's demo step deploys an agent and has the manager start its executor. `connection record`
needs a `ConnectionChecked` on the account stream, which only `mandate-executord connect-check`
writes. The workspace chart's one-off `run` Job could run only the CLI (`control`).

### Decision

1. **A `connect-check` run process.** `processes.json` gains a process of kind `run` on the
   executor image: role `mandate_executor`, egress to Postgres only, no vault, no broker host, and
   not in Compose. Its broker is the in-process fixture paper broker (DEC-2007), so it holds
   nothing secret.
2. **`run.process` picks the run.** A release with `run.id` set renders exactly the one run
   process `run.process` names, `control` by default, so existing runs are unchanged. A name the
   release does not render fails the render rather than run another process.
3. **Files stay on the scenario's path.** A run that needs input files, such as a mandate document,
   gets them as the demo scenario's gestures do (`gestures_job`: a ConfigMap mounted read-only at
   `/in`). This decision adds no second way.

### Rationale

The check is a one-off step of the cell's drill, like the CLI's commands, so it reuses the run Job
and its network policy rather than adding a long-running process.

### Alternatives considered

- **Run the check inside the executor pod at start.** Rejected: the connection is recorded before
  any deployment exists, and the manager starts executors only for deployed agents.
- **A `run.files` value written by an init container.** Rejected: a second path beside the
  scenario's ConfigMap for the same job.

<a id="dec-2036"></a>

## DEC-2036: The demo's second agent gets its own envelope, on QQQ

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that loosens nothing) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane D1), under DEC-79, at the coordinator's request |
| **Story or claim** | E21-54 (the demo's scenario); [claim #1491](https://github.com/kunwarshivam/mandate/issues/1491) |

### Context

#1729 added a second agent, `demo-web`, to the demo so that step 6's web gesture has an agent to
stop that the CLI has not stopped already. Step 2 deployed it on `demo-spy`'s confirmed version.
That version pins SPY on `conn_demo_paper_01`. With `demo-spy` deployed, mandate spec V-006 sees
SPY as claimed by another agent on that connection, so `agent deploy demo-web` is refused as
`version_invalid`. Every gesture after it never runs, and `cell-demo` has been red since.

### Decision

1. **V-006 stays as it is.** This is a fix to the demo, not to the rule.
2. **`demo-web` has its own envelope**, `deploy/cell/demo/web-mandate.json`.
   - It is `demo-spy`'s envelope with three changes: its name, its description, and its one pinned
     instrument, QQQ.
   - QQQ's snapshot is `deploy/cell/demo/web-instrument.json`: a plain, whole-share listed fund like
     SPY, on `nasdaq`, under an asset id used only by the demo.
   - It stays on the same paper connection, so it is a second agent on the same account. A second
     connection on SPY was rejected: V-006 is keyed by connection, so that would bypass the claim
     through the gap DEC-1173 item 6 leaves open.
3. **The second envelope goes through step 6, after step 5's record.**
   - Step 6 registers QQQ's snapshot, creates and confirms the version, then deploys `demo-web`,
     each code shown and then typed. Only after that does `web/` press its kill switch.
   - `agent deploy` and `backtest record` each take only the workspace's latest confirmed version.
     So the second version can be confirmed only after `demo-spy`'s deployment and its backtest
     record.
   - Both versions are journaled and confirmed by the owner (`AGENTS.md` rule 11).
4. **No recorded market data is added for QQQ, since nothing the demo runs reads any for it.**
   - The runtime runs the fixture wiring, which reads no bars or quotes (DEC-1013).
   - The research bundle is read only for an unpinned agent with a research envelope. Its contents
     stay SPY's alone (DEC-2246 item 6).
   - The backtest inputs are read only for `demo-spy`'s version.
   - The change that gives a pinned agent's runtime market data adds QQQ's recorded bars and quote to
     that fixture in the same change.

### Rationale

This is the smallest change that keeps every rule as written. `demo-spy`'s hash, its gestures and
the backtest's fixed payload reference (DEC-1144 item 8) are untouched.

### Alternatives considered

- **Relax or skip V-006 for the demo.** Rejected: that weakens a safety rule.
- **A second connection on SPY.** Rejected, as item 2 says.
- **Add QQQ to the research bundle now.** Rejected: no agent of the demo reads it for QQQ, and it
  would widen the researching agent's data universe beyond DEC-2246 item 6.

<a id="dec-2038"></a>

## DEC-2038: The deployment manager reports a deployment rule 201 refuses

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that adds an alert and loosens nothing) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane D1), under DEC-79 |
| **Story or claim** | E21-33 slice 4; [claim #1491](https://github.com/kunwarshivam/mandate/issues/1491); a minor from #1737's review, left under the freeze rule |

### Context

The deployment manager runs a version-2 `AgentDeployed` only when journal spec §9.24 rule 201
holds: its connection was established on the control stream before it and not revoked since.
Otherwise it starts nothing, and a later establishment does not revive it.

A version-1 deployment that the manager cannot run already raises `deployment_unread` (DEC-1173
item 3). A version-2 deployment that rule 201 refuses raised nothing, so an owner whose agent never
starts had no signal.

### Decision

1. **A new opaque operator alert, `deployment_unconnected`** (runbook RB-14, SEV-3). It carries a
   reason code and a runbook, nothing else.
   - It is raised for each version-2 `AgentDeployed` that rule 201 refuses, of an agent that is
     neither running nor stopped.
   - It is raised once per manager process, before the pass's slot steps, as `deployment_unread`
     is, so a journal append that halts the pass cannot lose it.
   - An `AgentStopped` of the agent ends it. A later deployment of the agent that rule 201 admits
     also ends it.
   - A refused record of an agent that is already running or stopped raises nothing: it is not a
     deployment the manager failed to run.
2. **RB-14 gets its page**, `docs/runbooks/RB-14-crash-loop.md`. It covers `crash_loop`,
   `deployment_unread` and `deployment_unconnected`, and states the gap below.
3. **A known gap, recorded, not closed here.** No process on `main` journals `AgentStopped` yet.
   The owner's stop is a command the agent's runtime carries out. So an agent id whose deployment
   never ran, or whose runtime crash-looped, is never stopped, and its alert repeats on each manager
   start. The owner deploys a new agent id meanwhile. The backlog carries it.

### Rationale

The alert costs one set and one loop in the manager, mirrors `deployment_unread`, and turns a
silent failure to start into a signal. It changes no start, stop or record: the manager journals
nothing new.

### Alternatives considered

- **Reuse `deployment_unread`.** Rejected: the operator's fix differs. A version-1 deployment needs
  a version-2 redeploy; a refused one needs its connection recorded first.
- **Revive the deployment on a later establishment.** Rejected: rule 201 judges a deployment where
  it stands on the stream, and §11's verifier reads it the same way.

<a id="dec-2041"></a>

## DEC-2041: the bootstrap's secrets command also writes the journal's archive endpoint

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79: a correction of an Accepted decision's text to match what it does, at the coordinator's review of #1802. It changes no behaviour and weakens no safety rule |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); DEC-2018 item 4 (superseded in part); DEC-2017 item 6; #1802 |

### Context

DEC-2018 item 4 says `cargo xtask gitops secrets <environment>` "writes the two Secrets alone". Since
#1802, the bootstrap also writes the ConfigMap `flux-system/journal-archive-endpoint`, holding the
archive's R2 endpoint, which names the founder's Cloudflare account and so is never in git
(DEC-2017 item 6). `secrets` writes it too, so the endpoint is set or rotated the same way. DEC-2018
is Accepted, so its text is corrected here rather than edited.

### Decision

1. **DEC-2018 item 4 reads:** `cargo xtask gitops secrets <environment>` writes the two workload
   Secrets and the archive endpoint's ConfigMap, and nothing else, under the same refusals and
   confirmation, to set or rotate them.
2. **A failed write says what it leaves broken.** Each write's error names its object and what the
   cluster cannot do without it: without the archive key, the journal cannot archive; without the
   tunnel token, the tunnel cannot connect; without the endpoint's ConfigMap, the journal and the
   workspace stay uninstalled. No error carries a value.

Every other item of DEC-2018 stands.

### Rationale

The endpoint is rotated with the key whose bucket it names, so one command sets both. A decision
whose text describes less than its command does would mislead the founder at the moment of a
rotation.

### Alternatives considered

- **A separate command for the endpoint:** a second step for the founder, and a window in which the
  key and the endpoint disagree.

<a id="dec-2042"></a>

## DEC-2042: `infra.yml`'s `sha` is optional for `preflight` and `plan`, and still required for `apply`

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79, at the coordinator's request after the founder's preflight failed three times on a moved `main`. It keeps the apply's binding to a reviewed commit, so it weakens no safety rule |
| **Story or claim** | E21-56, E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); DEC-2030 (its `sha` rule, superseded in part); DEC-2014 |

### Context

DEC-2030 has every `infra.yml` dispatch take `sha`, the full commit the founder reviewed, and refuse
unless it is both the run's commit and the checkout. That binds an approval to a reviewed commit.
For `preflight` and `plan`, which change nothing in the cloud, it mostly costs a failed run: `main`
moves between the founder reading a commit and dispatching, and the run refuses with "the dispatch
names commit X, but the run is at Y". The founder hit it three times in a row.

### Decision

1. **`apply` is unchanged.** It still requires `sha`, a full 40-character commit equal to the
   run's commit and the checkout. An empty `sha` is refused with an error naming DEC-2042 and
   saying to dispatch `apply` with the commit the plan printed.
2. **`preflight` and `plan` may leave `sha` empty.** The run then proceeds at the checkout, once
   the checkout is a full commit and the run's own commit (`GITHUB_SHA`), the commit `main` was at
   when the run started. A `sha` that is given is checked exactly as before.
3. **The commit is printed first.** Every run starts its log with `==> infra <action> of
   <environment> at commit <sha>`. One that ran without a named commit says so and names the
   `sha` to dispatch `apply` with. The plan records the commit as `TF_VAR_commit_sha`, and the
   apply's `terraform_data.applied_commit` (DEC-2030) still records the commit applied.
4. **An unknown action never defaults to the checkout.**

### Rationale

The binding that matters is the apply's: it changes the cloud, and the founder approves it. A
preflight or a plan only reads, so running it at the commit `main` is at costs nothing, and
printing that commit lets the founder name it for the apply. The apply still refuses a moved
`main`, so the plan the founder read and the apply they approve are of the same commit.

### Alternatives considered

- **Keep `sha` required everywhere:** the founder's runs keep failing on a moved `main`.
- **Make `sha` optional for `apply` too:** an approval would bind to whatever `main` became while
  the run waited, which DEC-2030 refused.

<a id="dec-2043"></a>

## DEC-2043: the one Cloudflare account is read as the distinct, non-empty ids of a listing capped at two, since provider 5.27.0 lists one account once per page

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (lane IAC), under DEC-79, at the coordinator's direction: it keeps the rule and only changes how the listing is read |
| **Story or claim** | E21-56 (lane IAC), claim [#1634](https://github.com/kunwarshivam/mandate/issues/1634); supersedes in part [DEC-2030](infrastructure.md#dec-2030) item 1 |

### Context

The founder's first staging `plan` (run 39207098260) failed DEC-2030 item 1's postcondition with
`self.result is list of object with 1000 elements`, though the preflight's own API check saw one
account. The pinned provider, cloudflare 5.27.0, asks for the next page of accounts until a page
comes back empty or it holds `max_items`, which defaults to 1000
(`internal/services/account/list_data_source.go`, with cloudflare-go v7.12.0's
`V4PagePaginationArray.GetNextPage`). The accounts API answered every page with the same account,
so the one account was listed 1000 times.

### Decision

1. **The rule stands:** the plan fails unless `CF_INFRA_API_TOKEN` sees exactly one Cloudflare
   account, so a token that reaches two never picks one silently.
2. **It counts distinct, non-empty account ids**, in the postcondition of
   `data "cloudflare_accounts" "visible"` in both roots, and the account id is the one such id.
3. **The listing is capped at `max_items = 2`**, two requests instead of a thousand. A token that
   sees a second account lists both on the first page, which item 2 refuses.
4. **Tests in both roots** cover one account repeated (with an empty entry), which plans and uses
   that id; two accounts repeated, which fails; and no account, which fails. The first fails
   against DEC-2030's `length(self.result) == 1`.

### Alternatives considered

- **Filtering empty ids alone:** the 1000 entries were copies of the account, not empty padding.
- **A different data source, such as `cloudflare_account` by id:** it needs the id, which DEC-2030
  item 1 reads rather than configures.

<a id="dec-2044"></a>

## DEC-2044: the preflight checks that Cloudflare Access is set up, and staging's cluster and VPC move to nyc1 at the same price after nyc3 had no capacity for its nodes

| | |
|---|---|
| **Status** | Accepted; item 2's region superseded by [DEC-2047](infrastructure.md#dec-2047) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (lane IAC), under DEC-79, at the coordinator's direction. It spends nothing more and keeps New York (DEC-995 item 2) |
| **Story or claim** | E21-56 (lane IAC), claim [#1634](https://github.com/kunwarshivam/mandate/issues/1634); with [DEC-2012](infrastructure.md#dec-2012) items 5 and 7, [DEC-2030](infrastructure.md#dec-2030) item 5 |

### Context

The founder's first staging `apply` (run 39209752034) failed on two things the plan could not see.
Cloudflare refused the Access policy with `access.api.error.not_enabled`, because the account had
no Zero Trust team. DigitalOcean refused the cluster with `422 region has insufficient capacity
for requested cluster for slug: s-2vcpu-4gb` in `nyc3`. The VPC, tag, firewall and Cloudflare
resources that came before may already exist in state.

### Decision

1. **The preflight names Access.** It lists the account's Access applications, which the token's
   Access apps permission reads. Cloudflare refuses that list with `access.api.error.not_enabled`
   until a Zero Trust team exists, and the checklist then prints `[MISSING] Cloudflare Access is
   enabled (one-time console step: Zero Trust → set up a team)`. Any other refusal is named with
   its status. The runbook lists it as the third console step, beside R2 and the billing alert.
2. **Staging's cluster and VPC move to `nyc1`.** `nyc1` is New York too, so DEC-995 item 2 holds,
   and `s-2vcpu-4gb` costs the same there. The estimate is unchanged at **57 USD a month** (two
   nodes at 24 USD, 5 USD of Spaces, 40 GB of volumes at 0.10 USD), under staging's 65 USD share
   and DEC-2605's 150 USD cap. Production stays in `nyc3`.
   - The state bucket stays in `nyc3`: the `cell` module takes `state_region` beside `region`,
     and Spaces does not run in `nyc1`. The bucket keeps `prevent_destroy`, and nothing about it
     changes.
   - The next apply replaces only the VPC, since a VPC's region cannot change. It is empty,
     because the cluster was never created. The tag and firewall have no region and stay as they
     are. Nothing else is destroyed.
   - The `budget` module now fails the plan when DigitalOcean does not offer the node size in the
     cluster's region, and names the regions that do. That catches a size missing from a region.
     It cannot catch a momentary capacity shortage like this one, which only the create call
     reports, so the runbook tells the founder to send that error to lane IAC.
   - The bootstrap's kubeconfig context follows: `do-nyc1-mandate-staging`. A test checks it
     against each root's region.

### Alternatives considered

- **A smaller node, `s-2vcpu-2gb` at 18 USD:** cheaper, but the workloads need the memory of
  4 GB nodes.
- **`s-2vcpu-4gb-amd` at 28 USD in `nyc3`:** it costs more (65 USD, the whole share). The
  coordinator asked for equal or lower cost.
- **Another metro, such as `sfo3`:** farther from the markets DEC-995 placed the cell beside.

<a id="dec-2045"></a>

## DEC-2045: the kill-switch admission policies cover every workspace namespace, and refuse a Job relabelled control or a control pod not owned by a Job

| | |
|---|---|
| **Status** | Accepted; items 2 to 4 superseded in part by [DEC-2046](infrastructure.md#dec-2046) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79 and DEC-176, at the coordinator's reviews of #1797 and #1810. It only narrows who may make a pod the signature webhooks skip, and keeps `AGENTS.md` rule 13 whole, so it weakens no safety rule |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); E21-34 (the kill switch); supersedes [DEC-2016](infrastructure.md#dec-2016) item 2 in part (its two admission policies and its drill) |

### Context

DEC-2016 item 2 has the signature webhooks fail closed but skip a pod labelled
`mandate.dev/process: control`, so the kill switch is never refused by a webhook outage, and adds
two `ValidatingAdmissionPolicy` objects so that the label is not a way around the check. As
written, the policies covered `ws-ws1` only; where the controllers share one identity
(`system:kube-controller-manager`), the pod policy was label-only, so a Deployment's pod could carry
the label; a CronJob could own a control Job; and a Job policy matching every update could refuse
the garbage collector while it deletes a finished kill-switch Job. Matching creation only instead
let a service account create a suspended unlabelled Job, give its pod template the label, and
resume it. Item 2 also promised a drill that did not yet exist.

### Decision

1. **Every workspace namespace.** Both policies select namespaces labelled
   `policy.sigstore.dev/include: "true"`, the label that puts a namespace under the signature
   webhooks, so a new workspace's namespace is covered by the same label.
   `deploy/tests/test_gitops.py` refuses a `ws-*` namespace without it.
2. **Pods.** A pod with the label, or one given it, must be made by the Job controller
   (`system:serviceaccount:kube-system:job-controller`, or `system:kube-controller-manager` where the
   controllers share one identity) and be controlled by a `batch/v1` Job, so the owner check refuses
   a Deployment's, ReplicaSet's or StatefulSet's pod under either identity.
3. **Jobs.** No service account may make a Job whose pods carry the label, or give an existing
   Job's pods it, and no CronJob may own such a Job. The founder's CLI creates the kill switch's Job
   as a person (E21-34), so a workload in the namespace, such as the deployment manager, cannot
   mint one. The policy matches creation and update; on update it fires only when the label is newly
   `control`, so the garbage collector's updates while it deletes a finished kill-switch Job are
   never refused. A later kill switch triggered by a service needs its own decision.
4. **Drilled.** The cell's `admission` drill (`cargo xtask ci cell-kill-switch`) applies these
   policies as they are, with a signature webhook that is down, fails closed, and skips what
   `admission.yaml`'s patch exempts. It checks on a real API server that the founder's kill-switch
   Job is admitted and its pod made; that an unlabelled pod is refused; that a person's or a service
   account's labelled pod, a service account's control Job, the control label a service account
   gives its suspended Job, a labelled Deployment's pod, a CronJob's control Job and the label added
   to a pod later are each refused; that a second workspace's namespace is covered; and that a
   finished kill-switch Job is removed by foreground deletion. Its seeded run matches the Job policy
   on creation only and must report the suspended Job's relabel admitted. The drill runs the
   controllers as the cell's k3s runs them; the owner check is what holds where they share one
   identity. It does not run policy-controller itself; that its reconciler keeps the post-render
   `objectSelector` rests on its source (knative.dev/pkg's validation reconciler sets a webhook's
   rules, namespace selector and client config, and leaves its object selector).
5. **Checked in CI.** `deploy/tests/test_gitops.py` also refuses admission policies that miss a
   label given on update, and each such check is shown failing on a seeded bug.

### Consequences

- The rest of DEC-2016 item 2 stands: the exempt pod still runs only the chart's digest-pinned
  image, and the start-up signature check (E21-35, OPS-15) still refuses any binary without the
  release key's signature in `staging` and `production`.
- A change to a control pod's or Job's spec after it is made is not covered here; a later decision
  takes it.

<a id="dec-2046"></a>

## DEC-2046: no one changes a kill switch's pod after it is made, and no service account changes a control Job

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79 and DEC-176, at the coordinator's review of #1810 (round 2, N1). It only narrows who may change a pod the signature webhooks skip, and keeps `AGENTS.md` rule 13 whole, so it weakens no safety rule |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); E21-34 (the kill switch); supersedes [DEC-2045](infrastructure.md#dec-2045) items 2 to 4 in part |

### Context

Under DEC-2045 both kill-switch admission policies fire on update only when the `control` label is
new. The signature webhooks skip a pod with that label, so anything that changes such a pod or its
Job's template afterwards is never checked. On a Kubernetes 1.35 API server, with DEC-2045's
policies, a service account could:

- swap the image of the kill switch's running pod (`kubectl set image`): a pod's container image
  stays mutable;
- add an ephemeral container to that pod (`kubectl debug`), through the `pods/ephemeralcontainers`
  subresource, which the pod policy did not match;
- change a person's suspended control Job, whose template labels and scheduling fields stay mutable
  (its image does not, on 1.35).

### Decision

1. **Pods.** The pod policy also matches `pods/ephemeralcontainers`, and on update it fires when
   the label is newly `control` or when the spec of a pod with the label changes
   (`object.spec != oldObject.spec`). The Job controller never changes a pod's spec, so any change
   to a control pod's spec, by anyone, is refused. The kubelet's status and the scheduler's
   binding go through their own subresources, and the Job controller's and garbage collector's
   finalizer updates change only metadata, so none of them is refused.
2. **Jobs.** The Job policy fires on update whenever a control Job's spec changes
   (`object.spec != oldObject.spec`), which includes a label newly given to its pods. A service
   account's change is refused; a person's is not. The garbage collector's deletion updates change
   only metadata, so foreground deletion still completes.
3. **Exec, attach, port-forward and proxy are held by RBAC.** They are CONNECT requests whose
   admission object is their options, not the pod, so no policy can tell a kill switch's pod from
   another. The cell's design gives access through RBAC: neither the overlays nor this repository's
   charts, as the hosted clusters render them, define a Role or ClusterRole; the one Deployment
   mounts no service-account token; and the workspace chart's deployment manager, where it runs, may
   only create, get, list and delete pods. `deploy/tests/test_gitops.py` refuses any Role or
   ClusterRole there that grants `pods/exec`, `pods/attach`, `pods/portforward`, `pods/proxy`,
   `pods/ephemeralcontainers`, `pods/*` or `*` in the core group, and any binding to
   `cluster-admin`, `admin` or `edit`, each shown failing on a seeded bug. It does not render the
   third-party charts (policy-controller, CloudNativePG), whose controllers' own roles are theirs,
   and it reads a role by its own rules, so a ClusterRole that aggregates others is not followed.
4. **A CronJob of any API group.** The Job policy's owner clause stays `r.kind == 'CronJob'`, with
   no `apiVersion`: it denies, so naming the group would let another group's `CronJob` own a
   control Job.
5. **Drilled.** The `admission` drill adds:
   - a service account's changed image and ephemeral container in the kill switch's pod;
   - a service account's change to a person's suspended control Job (a `nodeSelector`);
   - a person's control Job owned by another API group's `CronJob`.
   
   Each must be refused. Its seeded run restores DEC-2045's Job condition and must report
   `admitted a service account's change to a person's control Job`. The drill now cleans up whether
   or not a step errs, and waits for its namespaces to go. `test_gitops.py` checks both policies'
   update conditions and the subresource, each shown failing on a seeded bug.

### Consequences

- The founder cannot debug a kill switch's pod with an ephemeral container or change its image in
  place. A kill switch that needs a different image runs as a new Job.
- A later Role that grants exec or attach in a workspace namespace fails CI.

<a id="dec-2047"></a>

## DEC-2047: all of the hosted infrastructure lives in the US West: both clusters and their VPCs in sfo2, the archive in R2's western North America, and the state buckets to follow

| | |
|---|---|
| **Status** | Accepted; item 2's VPC replacement superseded by [DEC-2054](infrastructure.md#dec-2054) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | the founder, on 2026-10-19, in the coordinator's session (DEC-79: where the infrastructure runs). Their words, verbatim: "use sfo2. the server has to be in usa", and "and that means all of our infra has to be come from us-west-2 or sfo". The design below is lane IAC's, under DEC-79, and only narrows it |
| **Story or claim** | E21-56 (lane IAC), claim [#1634](https://github.com/kunwarshivam/mandate/issues/1634); supersedes in part [DEC-995](DEC-995.md) item 2, [DEC-2012](infrastructure.md#dec-2012) where it names `nyc3`, and [DEC-2044](infrastructure.md#dec-2044) item 2's region |

### Context

DEC-2044 moved staging's cluster from `nyc3`, which refused it for capacity, to `nyc1`. The next
staging plan (run 39213327884) stopped at DEC-2044's own precondition: `DigitalOcean does not offer
s-2vcpu-4gb in nyc1; it offers it in: ams3, blr1, fra1, lon1, sfo2, sgp1, syd1, tor1`. The
coordinator proposed Toronto. The founder answered in the words quoted above. `us-west-2` is AWS's
name for Oregon, and the platform has no AWS, so DigitalOcean's San Francisco regions cover it.

### Decision

1. **The rule:** every piece of hosted infrastructure lives in the United States, in the US West.
   - DigitalOcean resources go in `sfo2` (or `sfo3`).
   - Cloudflare R2 buckets take the location hint `wnam`, western North America.
2. **Both clusters and their VPCs run in `sfo2`**, which offers `s-2vcpu-4gb` at the same 24 USD.
   Each environment's estimate is unchanged at **57 USD a month** (two nodes at 24 USD, 5 USD of
   Spaces, 40 GB of volumes at 0.10 USD), under its 65 USD share and DEC-2605's 150 USD cap.
   - Staging's next apply replaces only the VPC that its first apply made in `nyc3`, since a VPC's
     region cannot change. It is empty, because the cluster was never created. The tag and
     firewall have no region. Nothing else in DigitalOcean is destroyed.
   - Production has no resources yet, so it starts in `sfo2`.
   - The bootstrap's kubeconfig context is `do-sfo2-mandate-staging`. A test checks it against
     each root's region.
3. **The archive moves to western North America under a new name**,
   `mandate-journal-archive-west-<env>`, with location `wnam` and R2's default jurisdiction.
   - Provider 5.27.0 replaces a bucket whose `location` changes, and R2 honours a location only
     the first time a name is created, so the same name cannot move.
   - The new bucket and its lock sit at new addresses. The eastern bucket and lock that staging's
     first apply may have made are forgotten with `removed { lifecycle { destroy = false } }`,
     never destroyed (`prevent_destroy` stays a safety rule). They are empty, since nothing has
     been archived, and the founder deletes them by hand.
   - Barman's bucket-scoped token is re-scoped to the new bucket. Lane GO's `journal.yaml`
     destination paths follow the name.
4. **Guards:** the `cell` module refuses any cluster region but `sfo2` and `sfo3`, and the
   `cloudflare` module refuses any archive location but `wnam`, in both roots. Tests show each
   refusal.
5. **A size missing from a region names the alternatives.** The `budget` module's error names the
   regions that offer the node size, and the region's available sizes with the same vCPUs and
   memory, with their monthly prices. Momentary capacity still shows only when the cluster is
   created.
6. **The state buckets follow in a separate change, and until it lands they are the one
   temporary exception to item 1.** `mandate-tofu-state-staging` (and production's, if a run
   makes it) stays in `nyc3`, which the `cell` module's `state_region` carries. So the US-West
   rule is not yet met for the state. A bucket cannot change region, so each environment gets a
   new state bucket in `sfo2`. The run copies the encrypted state object to it byte for byte,
   never decrypting it and with the passphrase unchanged, only when the new bucket is empty. It
   verifies the copy before `init`, and leaves the old bucket as a rollback for the founder to
   delete. The runbook lists every bucket the founder deletes by hand, by exact name.

### Alternatives considered

- **Toronto (`tor1`), the coordinator's first proposal:** in DEC-995's answer, but outside the
  United States, which the founder now requires.
- **Keeping the eastern archive's name:** R2 would keep its eastern location.

<a id="dec-2048"></a>

## DEC-2048: production may pin, per key, any digest staging's files held on `main`, so a rollback to one staging ran passes

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79 and DEC-176, at the coordinator's review of #1798. The check only narrows (per key, where it matched any digest anywhere), so it weakens no safety rule |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); supersedes [DEC-2016](infrastructure.md#dec-2016) item 5 in part |

### Context

DEC-2016 item 5 says `cargo xtask gitops promote production` copies staging's two files' digests
as they stand, and that `deploy/tests/test_gitops.py` refuses any digest production pins that
staging's files never held on `main`. Its check matched every `sha256:` digest in production against
every digest in staging's history, whatever it pinned: a chart's digest moved under another chart's
name, or an image reference with another repository, still passed. And the text read as if
production could only ever equal staging's current files, which would leave no way to roll
production back to a release staging ran before.

### Decision

1. **Per key.** What an environment pins is a set of pairs: each `OCIRepository`'s name in
   `charts.yaml` with its digest, and each `images` key in `images.yaml` with its full reference.
   `test_gitops.py` reads staging's two files at every commit on `main` up to the pull request's
   merge base, and refuses any pair production holds that staging never held under the same key.
2. **Rollback.** Production may pin any pair staging held on `main`, not only its current one, so
   rolling production back to a release staging ran is an ordinary pull request that passes the
   check. `promote production` still copies staging as it stands; a rollback edits production's files
   to an earlier staging pin by hand, reviewed like any change.
3. **Writes.** `promote` writes both files beside their targets before replacing either, so a write
   that fails leaves the overlay as it was.

<a id="dec-2049"></a>

## DEC-2049: The signer's refusals are each proven on a run that would otherwise sign, and `BASHOPTS=extdebug` is left as it was

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | The lane RS builder (`claude-code`), under DEC-79 and DEC-176, at the coordinator's review of #1829. It adds a test and a recorded reading, changes no behaviour and weakens no safety rule |
| **Story or claim** | E21-35 (lane RS); [DEC-2024](infrastructure.md#dec-2024) item 4 (superseded in part); #1829, #1852 |

### Decision

This adds to [DEC-2024](infrastructure.md#dec-2024) item 4, whose text stands otherwise.

1. **Each refusal is proven where it matters.** `the_signer_refuses_exported_functions` hands
   every run what a signing needs: a stand-in cosign pinned by its SHA-256, a key and a password.
   Its control, with nothing planted, signs all three files. Each run with a planted exported
   function (`echo`; `set`; `compgen` beside `echo`; `unset` beside both) must stop with its own
   refusal on stderr: the signer's "the environment defines shell functions; nothing is signed",
   or bash's "is a special builtin". It must also never run cosign, never print anything from the
   planted function, and never reach `RUNNER_TEMP`.
2. **`BASHOPTS=extdebug` is left as it was before DEC-2024 item 4.** With it in the job's
   environment, bash tries to load bashdb at start. Runners have no bashdb, so bash warns and
   turns debugging mode off. No trap reaches the signer through the environment, so nothing is
   added for it.

### Rationale

A refusal tested only on a run that would have failed anyway proves nothing: the run must be one
that would sign if the refusal were gone. Item 2 records what was checked so a later reviewer
need not check it again.

### Alternatives considered

- **Clearing `BASHOPTS` in the signer.** Bash reads it before the script's first line, so the
  script cannot clear it in time, and with no bashdb present there is nothing to clear it for.
- **Adding the text to DEC-2024 item 4.** DEC-2024 is Accepted, and the decisions README lets a
  PR change only its status line.

<a id="dec-2050"></a>

## DEC-2050: staging's apply verifies, read-only, that staging is up

| | |
|---|---|
| **Status** | Accepted; item 2 extended by [DEC-2051](infrastructure.md#dec-2051) and [DEC-2052](infrastructure.md#dec-2052); item 3's pending edge ended by [DEC-2056](infrastructure.md#dec-2056) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79, at the coordinator's request toward "staging delivered end to end". It only reads, adds no credential, no access path and no workflow change, so it weakens no safety rule |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); DEC-2018 item 7 (the bootstrap from `infra.yml`), DEC-2028 (the window) |

### Context

Staging's apply ends with lane GO's bootstrap: Flux is installed and synced from `main`. Nothing
then said whether staging came up, and the window closes with the job, so "is staging up?" could
only be answered by someone with cluster access.

### Decision

1. **Where.** `bootstrap_staging` runs `infra_verify::verify` right after the bootstrap, with the
   same kubeconfig (900 seconds, mode 0600, deleted however the bootstrap ends) and inside the same
   window, before `infra` closes it. The window, the Spaces key and their closes are unchanged, and
   `infra.yml` does not change.
2. **What it reads.** Only `kubectl get` and HTTP `GET`:
   - every Flux `GitRepository`, `OCIRepository`, `Kustomization` and `HelmRelease` is `Ready`, and
     there is at least one of each;
   - every pod in `flux-system`, `cnpg-system`, `cosign-system` and each `ws-*` namespace is
     `Running` with every container ready (a finished Job's pod is skipped), and each platform
     namespace runs one;
   - the bindings `mandate-control-pods` and `mandate-control-jobs` deny (DEC-2045, DEC-2046), and
     every webhook of `policy.sigstore.dev` fails closed (DEC-2016 item 1);
   - Cloudflare records the tunnel as `healthy`, read with the step's Cloudflare token from the
     root's `cloudflare_account_id` and `tunnel_id` outputs, each masked as it is read;
   - a request to `https://<api_hostname>/` without credentials is redirected or refused (3xx, 401,
     403). A 2xx means the API answers without Access and is a MISSING line, as is any other status.
3. **How it ends.** It reads staging every five seconds until every line holds, for at most eight
   minutes inside the kubeconfig's fifteen, prints the last reading as `infra verify (staging):`
   with `[ok]` and `[MISSING]` lines, as the preflight prints its own, and fails the run if any line
   is MISSING. Until the API pod's tunnel connector lands (#1539), the tunnel's line and the API's
   when Access cannot reach it are `[pending #1539]` notes that fail nothing, so a good apply and
   bootstrap end green and still show what waits; the PR that lands the connector sets
   `EDGE_PENDING` to `None`, and both fail from then on. An API that answers 2xx without
   credentials is MISSING whatever is pending. A read `kubectl` cannot make is its own MISSING line, never an abort. The window and
   the key are still closed and deleted after a failed verify.
4. **Not covered.**
   - Whether the API's origin itself answers through Access needs an Access service token. That
     is a new credential, so it is the founder's to decide; until then the check proves the edge
     enforces Access and the tunnel is connected.
   - A standalone `verify` dispatch would open the window outside the reviewed plan, by a
     DigitalOcean API call; that is a new access path and needs its own decision.

<a id="dec-2051"></a>

## DEC-2051: staging runs the workspace vault, its seal key typed by the founder

| | |
|---|---|
| **Status** | Accepted; items 3 and 5 superseded in part by [DEC-2052](infrastructure.md#dec-2052) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79, with lane V2 (the chart, DEC-2021) and at the coordinator's direction, which kept the seal key out of CI. It deploys what DEC-2021 decided, adds no credential to any CI job, and touches production not at all, so it weakens no safety rule |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); E21-32 (the vault); DEC-2021 (staging's static seal), DEC-2016 (promotion), DEC-2050 (verify, item 2 extended) |

### Context

Lane V2's vault chart (`mandate-vault-server`) is published and signed with the others, and
`deploy/cell/values/vault-staging.yaml` gives staging a static seal from the Secret
`vault-seal-key` (DEC-2021). DEC-2021 item 3 makes that Secret the founder's local bootstrap's to
create, from a key typed at a terminal, and production's unseal is still open (item 6). Nothing
installed the vault on a hosted cluster.

### Decision

1. **Staging alone.** `deploy/gitops/clusters/staging/` pins `mandate-vault-server` by digest,
   verified as release.yml's keyless signature on `main` like every chart of the repository's
   (DEC-2015 item 3), and installs it with the HelmRelease `vault` in `ws-ws1`, beside the
   workspace whose pods its NetworkPolicy admits. Its values are `vault-staging.yaml` unchanged,
   generated into a ConfigMap. Production pins and installs no vault; `promote production` copies
   only the charts production pins, so it never gains one, and `test_gitops.py` refuses either.
2. **Installed by helm-controller, never pruned, never waited on.** helm-controller runs the chart,
   so its CA, kept by `lookup`, survives each upgrade. Neither install nor upgrade waits for the
   vault to be ready, since it stays sealed until the founder initializes it. Removing it from git
   leaves it and its data in place.
3. **The seal key is the founder's, typed once.** `cargo xtask gitops bootstrap staging`, the
   founder's path at a terminal, asks for the key after Flux, with the terminal's echo off. It
   accepts exactly 64 hex characters (`openssl rand -hex 32`), one trailing line break aside: the
   unambiguous form OpenBao's static seal decodes into a 32-byte key. A refusal never repeats what
   was typed. It creates `ws-ws1/vault-seal-key`, its one entry `key`, with `kubectl create`, and
   only when no such Secret exists, so a later bootstrap keeps the key and never rotates it.
   `bootstrap_from_infra`, the CI path, never touches it, which a test pins. Until the Secret
   exists the vault's pod waits to mount it; it never starts without it.
4. **Initialization is the founder's, once.** `bao operator init -recovery-shares=3
   -recovery-threshold=2` in the vault's pod, its recovery keys and root token kept offline
   (DEC-2021 item 4). `deploy/gitops/BOOTSTRAP.md` gives both steps.
5. **Verify waits on it.** DEC-2050's check reads the vault's pod like any other, but while it is
   not ready its line is `[pending founder init]`, which fails nothing. The check cannot tell a
   vault never initialized from one that fails to unseal; only the vault's own status can.

### Consequences

- The static seal's first boot is staging's (DEC-2021, residuals), and lane V2's cell drill has
  not yet passed end to end in CI.
- Production gets the vault only after the founder decides its unseal (DEC-2021 item 6), by a
  change that adds it to production's overlay and to `test_gitops.py`.

<a id="dec-2052"></a>

## DEC-2052: staging's vault is set up from CI, its recovery keys kept in the cluster

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | the founder, on 2026-10-19 in the coordinator's session: unable to run DEC-2051's two steps from their phone, they chose "Keep them in-cluster (Recommended)" for where staging's vault recovery keys go, with production staying manual by the founder. Lane GO (Claude Code) built it, under DEC-79, at the coordinator's direction |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); E21-32 (the vault); supersedes, for staging only, [DEC-2021](infrastructure.md#dec-2021) items 3 and 4 and [DEC-2051](infrastructure.md#dec-2051) items 3 and 5; extends [DEC-2050](infrastructure.md#dec-2050) item 2 |

### Context

DEC-2021 item 3 has the founder type staging's vault seal key at a terminal, and says no CI job can
create it; item 4 has the founder keep the recovery keys offline. DEC-2051 built that: the founder's
bootstrap asks for the key, and the founder runs `bao operator init` by hand. The founder cannot run
either step from a phone, and chose to keep the recovery keys in the cluster instead.

### Decision

1. **Where.** `xtask/src/vault_init.rs` runs inside staging's apply job, after lane GO's bootstrap
   and before the verify (DEC-2050), with the same short-lived kubeconfig and inside the same
   control-plane window (DEC-2028). Only `bootstrap_staging`, which only staging's apply reaches,
   calls it, and a test pins that; production is never set up this way and stays manual.
2. **The seal key.** When `ws-ws1/vault-seal-key` does not exist, the runner makes 32 random bytes
   from `/dev/urandom` as 64 hex characters, masks them, and creates the Secret from `kubectl`'s
   standard input, as DEC-2051 item 3's check accepts it. An existing key is kept and never
   rotated. The founder's bootstrap still asks for one when none exists, as break-glass.
3. **Init, once.** The step waits for `vault-0` to run, reads `bao status`, and skips init when the
   vault is initialized; a status it cannot read stops it rather than initializing blind. Otherwise
   it runs `bao operator init -format=json -recovery-shares=3 -recovery-threshold=2` in the pod,
   masks every value of the answer, and stores the answer, its root token taken out, as
   `init.json` in the Secret `vault-recovery/vault-init`, in a namespace made for it that binds no
   workload, so only cluster admins can read it.
4. **The root token never outlives the run.** It is revoked with `bao token revoke -self`, the token
   on the command's standard input, whether or not the answer was stored, and by a guard on every
   other path; a revoke that fails stops the run, saying the token may be live. Provisioning
   (DEC-2022) comes with V3 (E21-9), from a short-lived root token generated from the stored
   recovery keys.
5. **Nothing secret leaves memory.** No key, token or recovery key is ever an argument, an
   environment variable, a file or a log line: each reaches `kubectl` on its standard input, and
   `kubectl`'s own error output is not printed. The log carries status words only.
6. **Verify.** With init in the same run, a vault still not ready when DEC-2050's verify ends is a
   failure like any other, no longer a pending note. Staging's first apply showed two more gaps in
   DEC-2050's check, closed here: a Job's pods are judged by the Job, whose `Complete` condition
   alone counts, since a Job that retried leaves its failed pods behind; and a line that is MISSING
   for a pod or a Job says why, in Kubernetes status words alone: the pod's phase and reason, the
   first container reason it waits or ended with and its exit code, and a Job's counts and every
   condition it holds. A failing Job's last twenty log lines are read but never printed: their
   lines may echo a connection string or SQL, so only the fixed class words they match are
   (`auth-failed`, `connection-refused`, `timeout`, `dns`, `missing-object`, `permission-denied`,
   `tls`, `migration-edited`, else `unclassified`). So the next apply's log says what is wrong
   without anyone reaching the cluster. The check runs `kubectl get` and `kubectl logs` alone.

### Consequences

- Anyone with cluster-admin on staging can read the recovery keys, and so make a root token; so can
  Flux's kustomize and helm controllers, which run as cluster-admin and apply only what `main`
  holds. That is the founder's accepted trade for a staging that sets itself up; production does
  not take it.
- If storing the answer fails after init, the recovery keys are lost, though the vault still
  unseals itself. Staging holds no data yet, so the remedy is to remove the vault's volume and let
  the next apply initialize it again.
- Lane V2's caveats stand: the static seal's first boot is staging's, and the cell vault drill has
  not yet passed end to end in CI.

<a id="dec-2054"></a>

## DEC-2054: the cluster's VPC is a new one, named for its region and never destroyed by a plan, since DigitalOcean refuses to delete staging's first VPC, nyc3's default

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (lane IAC), under DEC-79, at the coordinator's direction. It spends nothing and only narrows DEC-2047 |
| **Story or claim** | E21-56 (lane IAC), claim [#1634](https://github.com/kunwarshivam/mandate/issues/1634); supersedes [DEC-2047](infrastructure.md#dec-2047) item 2's "replaces only the VPC" |

### Context

DEC-2047 moved staging's cluster to `sfo2` and expected the next apply to replace the empty VPC
that the first apply had made in `nyc3`. The apply (run 39218360992) failed with `DELETE
/v2/vpcs/…: 403 Can not delete default VPCs`. DigitalOcean makes the first VPC in a region that
region's default VPC, and refuses to delete a default VPC. Creates earlier in that apply may be in
state.

### Decision

1. **The `network` module declares the VPC at a new address, `digitalocean_vpc.west`,** named
   `<name>-<region>`, so it never takes the name of a VPC the account already holds.
   - Staging's range moves to `10.111.0.0/20`, so it overlaps no VPC the account keeps.
   - Production's range, `10.120.0.0/20`, is unchanged.
   - The firewall's rules follow the new range. The tag and firewall have no region and are
     updated in place.
2. **The old address is forgotten with `removed { lifecycle { destroy = false } }`.** No plan
   deletes `nyc3`'s VPC `mandate-staging`. It is free and holds nothing, and the runbook lists it
   under "Deleted by hand, later": the founder may delete it once it is no longer the default, or
   leave it.
3. **No plan destroys a VPC.** The new VPC keeps `prevent_destroy`, so any plan that would
   replace or delete it fails at plan time, before an apply can stop halfway. An xtask test fails
   when a VPC in the tree lacks it.

### Alternatives considered

- **Importing the `nyc3` VPC and keeping it:** the cluster must sit in a VPC of its own region.
- **Making another VPC the default first:** a console step for the founder, with nothing gained
  over forgetting it.

<a id="dec-2055"></a>

## DEC-2055: Leak-guard L10, Rust in xtask and every crate's examples

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | `claude-code` (lane SI), at the coordinator's direction. It is a reversible engineering choice under DEC-79. It only tightens what xtask and the examples may write, and its `env::var` consumer rule tightens L10 as first planned, under DEC-176 |
| **Story or claim** | The public-repository secrets audit (2026-10-18). This specifies the L10 that [DEC-2032](infrastructure.md#dec-2032) left to a later PR |

### Context

[DEC-2032](infrastructure.md#dec-2032)'s leak-guard reads CI's workflows and scripts, and left one rule, L10, to a
later change. L10 covers the Rust that CI runs with secrets in reach: every `.rs` file under
`xtask/src` and under each crate's `examples/`, which `r2-lockcheck.yml` runs.

### Decision

1. **What L10 refuses:**
   - `{command:?}`, `{cmd:?}` or `{child:?}`, and their `:#?` forms, because a command's `Debug`
     form prints its environment;
   - `dbg!`;
   - `env::vars` or `env::vars_os`, called or imported, except on reviewed lines that read names
     only;
   - a `Debug` derive on a struct with a token, secret, password, passphrase, kubeconfig or
     credential field. That covers `#[derive(…)]` and `#[cfg_attr(…, derive(…))]`, on one line or
     several;
   - `::add-mask::` outside `xtask/src/mask.rs` (lane GO, #1723), xtask's one writer of it, whose
     `command_escaped` escapes `%` to `%25` first, then CR to `%0D` and LF to `%0A`.
2. **Every `env::var(…)` and `env::var_os(…)` on a line is read, not only the first:**
   - its argument is a string literal, or a plain identifier or path (`name`, `&name`,
     `module::NAME`), and nothing else, so a computed name (`name_of(1)`, `format!(…)`) is
     refused;
   - an `env::var` result is consumed by `.ok()`, `.is_ok()`, `.is_err()`, `.unwrap_or(…)`,
     `.unwrap_or_default()`, `.unwrap_or_else(|_| …)` or `.map_err(|_| …)`, which never show the
     error, unless the name is a string literal or an `UPPER_CASE` constant that names no secret.
     So `?`, `.unwrap()`, `.expect(…)`, `.map_err(|e| …)` and `.context(…)` are refused on a
     secret's read. The error's `NotUnicode` case holds the value, and each of those can print
     it. DEC-2032 planned to refuse only `.context`; refusing the rest only tightens, which
     DEC-176 lets an agent take;
   - an `env::var_os` result is an `Option`, which holds no error, so only its argument is read;
   - `env::var` and `env::var_os` are called where they are named. Importing them (`use
     std::env::var`, `use std::env::{self, var}`, `as` renames) or passing them as a path
     (`let f = env::var`, `.map(env::var)`) is refused, as `env::vars` is, so every call is read.
3. **What is not read, and what is reviewed:**
   - The guard's own file, `xtask/src/leak.rs`, is not read: its fixtures and messages name what
     the rules refuse.
   - Lines on `main` that broke the rules are fixed in the same change: `release.rs`'s `GH_TOKEN`
     read and `gitops.rs`'s workload-secret reads take `.ok()` before `.context`.
   - Reviewed lines in `ALLOWED`, each with its reason there:
     - `main.rs`'s removal of every `PROPTEST_*` variable by name;
     - `release.rs`'s DEC-2027 refusal, which reads the environment's names only;
     - the two fields of `gitops.rs`'s `WorkloadSecrets`, which derives `Debug` over its
       redacting `Secret` type.
   - What L10 does not yet read is in the backlog under E21-52: a positional `{:?}` of a command,
     and an `::add-mask::` built from parts.

### Alternatives considered

- **A Rust parser (`syn`):** it would read the token tree, but it is a new dependency for a text
  rule. The rules here are narrow enough to read from lines, and the review's probes are kept as
  regression rows.

<a id="dec-2056"></a>

## DEC-2056: the tunnel runs in the API's pod, and staging's verify fails on the edge

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79, at the coordinator's direction ("#1539's later change"). The API still listens on its pod's loopback only, and verify's edge lines now fail instead of noting, so it weakens no safety rule |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); DEC-967 item 6 (the cell's one ingress), [DEC-2050](infrastructure.md#dec-2050) item 3 |

### Context

The workspace API server listens on its pod's loopback only (DEC-978 item 1). Lane IAC's tunnel
sends the API's host to `http://127.0.0.1:8080` (`deploy/tofu`, `api_origin`, which accepts only a
loopback or in-cluster Service address). The chart ran cloudflared as its own Deployment, so that
origin was cloudflared's own loopback, where nothing listens. DEC-2050 item 3 therefore made the
tunnel's line and the API's line under staging's verify pending notes, "until the API pod's tunnel
connector lands", citing #1539.

### Decision

1. **A sidecar, not a Service.** Where the API runs and `ingress.cloudflared.enabled`, cloudflared
   runs as a second container of the API's pod, with the same image, arguments, token file and
   limits as before. It shares the pod's loopback, so the API keeps no non-loopback listener and the
   cell still has no Service, NodePort, LoadBalancer or Ingress.
2. **One place per tunnel.** Cloudflare spreads a tunnel's requests over all its connectors, and a
   connector without the API answers 502. So with the API, no standalone cloudflared Deployment
   renders. Without the API, it renders as before, so the tunnel stays connected.
3. **Only cloudflared reads the token.** The tunnel's Secret is a volume of the API's pod mounted
   only by the cloudflared container. That container mounts only `/tmp` and the token, nothing of
   the API's: not its session key, not the CA bundle, not the subject links.
4. **Egress.** `egress-cloudflared` keeps its rules (DNS, the edge's CIDRs on 7844) and selects
   whichever pod runs cloudflared. A pod's containers share one network, so the API's container
   can also reach the edge on 7844. That reaches only Cloudflare's tunnel edge, which takes nothing
   without a tunnel token, and the API's container cannot read the token.
5. **Verify.** `EDGE_PENDING` is `None`: an unhealthy tunnel, or an API Access cannot reach, is a
   MISSING line on staging's apply. The pending mechanism stays, and its test now names a change
   of its own. An API that answers 2xx without credentials fails, as before.
6. **Tests.** `deploy/tests/test_cell.py::test_the_tunnel_runs_beside_the_api` catches five seeded
   bugs:
   - a second connector;
   - no sidecar;
   - the policy selecting another pod;
   - the API container mounting the token;
   - cloudflared mounting the API's session key.

   `xtask` `the_edge_fails_unless_a_change_is_pending_but_never_on_an_open_api` covers item 5.

### Consequences

- Restarting the tunnel, for a rotated token, restarts the API too once it runs, and the API's
  sessions with it (a restart already loses them, DEC-978 item 6).
- Hosted cell #1 (lane H, `deploy/hosted/cell1.py`) routes its app tunnel to
  `api.ws-ws1.svc.cluster.local:8080`, and runs no API. If it ever runs one, its route becomes
  `http://127.0.0.1:8080`.
- If staging's tunnel is not healthy, its next apply fails verify where it used to pass with a
  note.

<a id="dec-2057"></a>

## DEC-2057: staging's API signs in through the site's existing Supabase project

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | the founder, [DEC-2605](process.md#dec-2605) row 15 ("no new project"); the values that follow from it written by an agent (Claude Code, lane GO) under DEC-79 |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); [DEC-640](DEC-640.md) (the managed issuer), [DEC-2010](infrastructure.md#dec-2010) (the demo's fixed workspace) |

### Context

The staging overlay's `api.*` values were placeholders that `deploy/tests/test_gitops.py` refuses,
waiting on staging's identity provider and web app origin. The founder chose not to make a new
Supabase project for staging: staging reuses the site's existing one (DEC-640).

### Decision

1. **Issuer.** `https://wyyxngmblqqokcvraflx.supabase.co/auth/v1`, the site's project. The key
   set is the issuer's own `/.well-known/jwks.json`, and the audience is Supabase's
   `authenticated`, as the identity spec's item 15 names for the managed issuer.
2. **Workspace and organization.** The demo's fixed workspace `01KDEM0000000000000000000W` and its
   organization `01KDEM0000000000000000000R` (DEC-2010), the ones the local cell hosts.
3. **App origin.** `https://app.owlhead.ai`. `web-deploy.yml` deploys one web app, production's,
   and there is no staging web app, so that is the only origin a browser could call staging's API
   from.
4. **Public values only.** No Supabase key, service-role or anon, is in the repository. The API
   verifies tokens against the published key set and needs none.
5. **Tested.** `test_api_settings` refuses a placeholder, a non-ULID id, a non-https URL, an empty
   audience, and now a key set that is not under the issuer. Each is shown failing on a seeded bug.

### Tenant separation

Staging's users share the site's sign-in pool, so anyone who can sign in to the site can present
a token staging's API accepts. That token proves only who they are. Every read and command is
still authorized by membership of the hosted organization and workspace (DEC-978 item 8,
`authorize`), and a subject is linked to a member only by the operator's subject links (DEC-1023).
A site user with no link to the staging workspace gets 401 or 403, never data. Staging stays paper
only.

### Consequences

- A browser on `app.owlhead.ai` reaches staging's API only once a web build points at
  `api-staging.owlhead.ai`. This setting only admits that origin.
- Separating staging's sign-ins later means a new project and a new decision. Only these values
  change.

<a id="dec-2058"></a>

## DEC-2058: hosted processes reach their external hosts by exact name, through Cilium

| | |
|---|---|
| **Status** | Accepted; item 3 superseded in part by [DEC-2059](infrastructure.md#dec-2059): Alpaca's live host is the token-exchange process's alone, in one policy |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79, at the coordinator's direction for lanes LV, D1 and I2. It opens only the hosts infrastructure §3.1 already gives each process, paper only, and narrows each to exact names, so it weakens no safety rule |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); infrastructure design §3.1; DEC-967 item 6 (a portable chart); DEC-2057 (staging's identity provider) |

### Context

The workspace chart gives each process its §3.1 egress. In-cluster targets are selected by label,
and external ones by the CIDRs values give each slot: `broker`, `market-data`, `model-providers`,
`identity-provider`, `token-host` and others. Every external slot is empty on staging, so no process
there can reach an outside host. That includes the API, which fetches the identity provider's key
set. The hosts staging needs have no stable address list. OpenRouter and Supabase are Cloudflare
anycast, and Alpaca publishes none. A CIDR list wide enough to reach them would reach everything
else on those networks.

### Decision

1. **Names, not addresses.** On a hosted cluster, each process with an external §3.1 destination
   gets a `CiliumNetworkPolicy` (DOKS runs Cilium). The policy selects that process's pods only, by
   `mandate.dev/workspace` and `mandate.dev/process`. It allows exactly two things: DNS to the
   cluster's kube-dns through Cilium's DNS proxy, which learns each name's addresses as the pod
   resolves it, and TCP 443 to its hosts by exact `matchName`, with no pattern and no other port. It
   takes no ingress. Cilium adds these allows to the chart's own NetworkPolicies, which stay
   unchanged.
2. **Staging's hosts.**

   | Process | Hosts |
   |---|---|
   | api, control | `wyyxngmblqqokcvraflx.supabase.co` (DEC-2057) |
   | executor | `paper-api.alpaca.markets` |
   | data | `data.alpaca.markets` (*amended by [DEC-2082](infrastructure.md#dec-2082): the IEX source reaches only this host, so `stream.data.alpaca.markets` is dropped*) |
   | model-gateway | `openrouter.ai` |

   The runtime gets no model egress: it reaches models only through the gateway. The token-exchange
   process's OAuth host joins when lane X names it.
3. **Never live.** `api.alpaca.markets`, Alpaca's live trading host, appears nowhere in a hosted
   overlay (AGENTS.md rule 8).
4. **The chart stays portable.** The policies live in the hosted overlay
   (`deploy/gitops/clusters/staging/egress.yaml`), not in the workspace chart, which keeps to core
   Kubernetes APIs (DEC-967 item 6). The local cell keeps no external egress.
5. **Checked.**
   - `deploy/tests/test_gitops.py::test_fqdn_egress` compares each policy with an independent
     table of item 2. `test_each_fqdn_egress_bug_is_caught` shows nine seeded bugs failing it: the
     live host, a wildcard, a host on the wrong process, port 80, a CIDR, a wrong selector, no
     workspace label, an ingress, and a missing policy.
   - Staging's verify adds a read-only line: Cilium's config must not turn `enable-l7-proxy` off,
     since without the DNS proxy no FQDN policy is enforced. Cilium's default is on.

### Consequences

- If DOKS ever runs Cilium without its DNS proxy, verify fails on that line and these hosts are
  unreachable. The way out is then an egress proxy, which needs its own decision.
- A new external host is a change to this table, the test's table and the overlay together.
- Production gets its own overlay's policies when it runs these processes.

<a id="dec-2059"></a>

## DEC-2059: the token-exchange process alone reaches Alpaca's live host, for its OAuth call

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79, at the coordinator's direction (staging plan item 6). It implements [DEC-821](DEC-821.md) items 2 and 4, which the founder accepted, on staging's network, and adds no reach that decision does not already allow; independent review before merge |
| **Story or claim** | E21-58, claim [#1635](https://github.com/kunwarshivam/mandate/issues/1635); [DEC-821](DEC-821.md) items 2 and 4; [DEC-2627](DEC-2627.md) (the token exchange runs once per connect); connections spec §5.2; supersedes [DEC-2058](infrastructure.md#dec-2058) item 3 in part |

### Context

DEC-2058 gave staging's processes their external hosts by exact name through Cilium. Its item 3
refused Alpaca's live host, `api.alpaca.markets`, in every hosted overlay. The paper OAuth connect
needs one call there: `POST https://api.alpaca.markets/oauth/token`, made by the token-exchange
process (DEC-821 item 2). `processes.json` lists that process as a service; DEC-2627 has the
deployment manager start it once per connect. DEC-821 item 4 states the limit: network allow
lists work on names and addresses, not paths, so a process that can reach the token endpoint can
reach the whole live API at the network level, and the token exchange's dedicated client type is
the only barrier to the rest.

### Decision

1. **One pod reaches the host.** Staging's `egress.yaml` adds `egress-fqdn-token-exchange`, a
   CiliumNetworkPolicy that selects only pods labelled `mandate.dev/workspace: ws1` and
   `mandate.dev/process: token-exchange`. It allows two things: DNS through Cilium's proxy, and TCP
   443 to `api.alpaca.markets` by exact name. Nothing else.
2. **Every other pod still refuses it.** The executor reaches only `paper-api.alpaca.markets`, as
   DEC-821 item 4 requires, and no other process is given the live host. DEC-2058 item 3 now reads:
   the live host appears in a hosted overlay only in this one policy.
3. **Method and path are held in the process, not on the wire.** HTTPS hides the method and path
   from the network. Enforcing them there would need Cilium's TLS interception: a certificate
   authority inside the cluster, trusted by the pod, re-originating TLS to Alpaca. That is a new key
   and a new attack surface, for a check DEC-821 item 2 already makes in code.
   - **On `main` today** (lane X, `crates/mandate-connections/src/tests/`):
     - `hosts.rs::the_live_token_request_is_post_oauth_token`: the only live request type is that one
       call;
     - `hosts.rs::the_live_host_admits_only_post_oauth_token`: every other method, path, query,
       fragment, port, userinfo or look-alike host is refused before anything is sent;
     - `exchange.rs`'s tests: exactly one call, with only the exchange's members.
   - **Not yet, arriving with the HTTP transport** (lane X's slice 5b, `exchange_http.rs`, tests
     first): the exact form body on the wire, a redirect from the token endpoint never followed, and
     only an admitted outbound reaching the socket.
4. **Checked.** `deploy/tests/test_gitops.py` adds the host to its independent table for
   `token-exchange` alone. It requires the live host to appear exactly once in a hosted overlay, as
   an exact name in the one policy whose selector is exactly the workspace's token exchange.
   It refuses any other object that can grant egress in the overlay: a `NetworkPolicy`, a
   `CiliumClusterwideNetworkPolicy`, or a `CiliumNetworkPolicy` other than the expected ones. It
   matches the host in any letter case. `test_each_fqdn_egress_bug_is_caught` shows each of these
   failing it:
   - the host on the executor or on the data process;
   - a paper host added to the token exchange;
   - port 80;
   - a wildcard;
   - a CIDR;
   - the selector without its workspace label;
   - a policy granting the host to another workspace's token exchange;
   - a second copy of the policy;
   - the host named in any other object, in lower or mixed case;
   - a clusterwide policy to `world`, to `0.0.0.0/0`, or to every name;
   - a NetworkPolicy to `0.0.0.0/0`.

### Residual risk

- **Any pod with the label reaches the host.** Once DEC-2627 lets the deployment manager start the
  token-exchange process, any pod the manager makes with `mandate.dev/process: token-exchange` in
  this workspace reaches the live host. The admission policy's image and Secret allowlists are today
  a union over all the manager's templates. So a compromised manager could start a pod with that
  label running another allowed image.
- **What bounds it.** That needs the manager itself compromised. Even then, no build outside
  production compiles a live trading host but this one token endpoint (DEC-821 item 4), and no
  process holds an order client for it.
- **What closes it.** DEC-2627's implementation must make the image, Secret and command allowlists
  per process for `token-exchange`: only its own image, its own Secrets and its own command may run
  under that label. The coordinator has given lane D1 that requirement.

### Consequences

- The first real exchange stays the founder's own act (DEC-821 item 2). Agents never call the live
  host; their tests use fixtures.
- If the token exchange ever runs under another process label, it loses the host until this policy
  and its test change together. Its pods carry the chart's workspace slug (`ws1`), not the workspace
  ULID, as every chart pod does.
- Production gets the same policy in its own overlay when it runs the token exchange.

<a id="dec-2061"></a>

## DEC-2061: the token exchange may overwrite its own connection's code, so taking the code is single use

| | |
|---|---|
| **Status** | Accepted; item 2's grant and item 4's take amended by [DEC-2321](trading-core.md#dec-2321) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent (Claude Code, lane V2), under DEC-79, accepted by the coordinator on 2026-10-19 at lane X's request: it grants one process one more capability on its own connection's entry only, in service of single use, and adds no reach |
| **Story or claim** | E21-32, claim [#1442](https://github.com/kunwarshivam/mandate/issues/1442); builds on [DEC-2060](DEC-2060.md); lane X's atomic `take_code` (#2020) |

### Context

The token exchange reads the authorization code the API stored, redeems it once, and deletes it
(connections spec §5.2). Lane X's review made taking the code atomic: of any number of overlapping
exchange runs for one connection, exactly one gets the code. OpenBao 2.7.1's KV v2 has no
compare-and-delete. Its soft delete and its `destroy` of a version are both idempotent, so neither
tells the first taker from the second. A check-and-set write does: a write with `cas` set to the
version the taker read succeeds once, and every other taker's fails. That write needs `update`
on the code, which DEC-2060 item 4's exchange policy does not grant.

### Decision

1. **A new access, `Access::Take`.** It overwrites an entry that is there, and never creates one.
   `policy::allows` grants it to a connection's token exchange on that connection's code alone:
   no other identity, no other connection's code, and no token or secret.
2. **Only the Kubernetes-auth policy grants it.** The exchange's templated policy (DEC-2060 item 4)
   gives its code's data path `read` and `update`, and never `create`, so it cannot write a code
   where none is. The AppRole policies, retired with that exchange (DEC-2060 item 8), never grant
   it, and a test pins that.
3. **The executor gains nothing.** Its policy still reads its own token alone.
4. **How a take works.** The taker reads the code and its version `v`, writes a tombstone with
   `cas = v`, then deletes the code's metadata. A taker whose write loses gets no code. A read
   that finds the tombstone is no code. Neither ever leaves the code readable. These two
   properties are pinned by tests where the take is built.

### Rationale

Single use of the code is what makes a replayed or duplicated exchange harmless. The one
capability it costs is narrow: the exchange can already create and update its own connection's
token, which is the more sensitive entry, and the new grant reaches only that connection's code.

### Alternatives considered

- **A compare-and-delete.** OpenBao has none.
- **Destroying the version read.** Rejected: it is idempotent, so two takers both succeed.
- **Generic `Write` on the code.** Rejected: it would also grant `create`, and widen the AppRole
  table that is being retired.

<a id="dec-2062"></a>

## DEC-2062: the vault reviews connection processes' tokens as its own ServiceAccount, bound to `system:auth-delegator`

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | an agent (Claude Code, lane V2), under DEC-79, the number assigned by the coordinator, with lane GO's chart conditions; it grants one ServiceAccount the one cluster role Kubernetes auth needs, and only where that auth is on |
| **Story or claim** | E21-32, claim [#1442](https://github.com/kunwarshivam/mandate/issues/1442); completes [DEC-2060](DEC-2060.md) item 7 |

### Context

DEC-2060 has the vault check each connection process's projected token with the cluster's
TokenReview API on every login, and lane GO's #2012 lets the vault reach the API server by
address. But the vault had nothing to call TokenReview with. Its pod mounts no ServiceAccount
token (`automountServiceAccountToken: false`), its Kubernetes auth mounts are configured with
only the host and the CA, and no account holds the right to review tokens. So every in-cluster
Kubernetes login would fail closed. The coordinator's review of #2079 found this.

### Decision

1. **Its own account.** The vault runs as the ServiceAccount `vault` in its namespace. The
   account and the pod both keep `automountServiceAccountToken: false`, and no Secret holds a
   token of it.
2. **Only with Kubernetes auth on.** The chart renders items 3 and 4 only when `kubeApi.enabled`
   is set, the same value that opens the vault's egress to the API server, so the two cannot
   drift apart.
3. **One projected token.** One projected volume, mode 0440, mounted read-only at
   `/var/run/secrets/kubernetes.io/serviceaccount`, the path OpenBao reads in a cluster. It holds:
   - the account's token, for the API server's default audience, lasting at most 3600 s and
     rotated by the kubelet. The default audience is deliberate: the API server's TokenReview
     accepts the vault's own token under it, and this token reaches nothing but the API server;
     the connection processes' tokens, which reach the vault, carry the audience `mandate-vault`
     instead (DEC-2060 item 7);
   - `ca.crt`, from the namespace's `kube-root-ca.crt` ConfigMap.

   The mode is the one the chart's other mounts use. The kubelet gives the projected files the
   pod's fsGroup, 1000, so the vault's user (100, in that group) reads them. The chart pins that
   the pod names both `runAsUser` and `fsGroup` and that no container overrides its user, since a
   mismatch would leave the files unreadable and every login refused, with nothing at render to
   show it.

   The mounts' configuration sets no `token_reviewer_jwt`, so OpenBao rereads the rotating local
   token instead of storing a long-lived one. Their `kubernetes_host` is
   `https://$KUBERNETES_SERVICE_HOST:$KUBERNETES_SERVICE_PORT`, the ClusterIP, which the API
   server's certificate names and the existing `/32` egress already covers. No CA is configured,
   so OpenBao reads the projected `ca.crt`.
4. **One binding.** One ClusterRoleBinding, `vault-token-review-<namespace>`, of the ClusterRole
   `system:auth-delegator` to that account in its namespace alone. No other Role, ClusterRole or
   binding names the account.

`deploy/tests/test_vault.py` pins each item, and a seeded bug for each shows the check catches it.

### Residuals, disclosed

- `system:auth-delegator` is cluster-scoped. It lets the account create TokenReviews and
  SubjectAccessReviews for any token in the cluster, not only the workspace's. A reviewer learns
  whether a token it already holds is valid, and whose it is; it can neither mint a token nor act
  with one. Only the vault pod holds this account's token.
- The binding names the ServiceAccount `vault` in the release namespace, so any pod running as
  `vault` there would hold the review right. Only the vault's StatefulSet names that account. The
  deployment manager's admission policy refuses any pod it creates that names an account other
  than `default`; the per-connection account names it is to admit are lowercase hexadecimal, after
  an optional `x` (DEC-2060 item 2), and can never be `vault`.
- The binding's name carries the namespace, so two workspaces' vaults in one cluster get two
  bindings, each to its own account.

### Rationale

This is the shape Kubernetes and OpenBao document for in-cluster review. A projected,
short-lived local token, reread on each review, is better than a long-lived reviewer JWT stored
in the vault's configuration. Gating on the same value as the egress means the vault never holds
the right without the reach, nor the reach without the right.

### Alternatives considered

- **A reviewer JWT from a dedicated ServiceAccount, stored in the mounts' configuration.**
  Rejected: the token would be long-lived and kept in the vault's storage.
- **Each login's own token as its reviewer** (`use_jwt_as_reviewer`, no binding). Rejected: every
  executor's and exchange's ServiceAccount would then need `system:auth-delegator`, which is many
  bindings instead of one.

<a id="dec-2063"></a>

## DEC-2063: The token-exchange cell drill runs the whole paper connect, stops at the token entry, and is the CLI and API's contract test

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79; the split and the contract test agreed with the coordinator on 2026-10-25) |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | an agent (Claude Code, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)), with lane D1 |
| **Story or claim** | E10-22; [DEC-2330](DEC-2330.md) item 5; [DEC-2125](DEC-2125.md); [DEC-2633](process.md#dec-2633); [DEC-2622](DEC-2622.md); [DEC-464](earlier-303-599.md#dec-464) |

### Context

E10-22 asks for the real token-exchange pod, as the deployment manager starts it, proven against
the cell's fixture provider and vault before the founder's staging connect. The pod runs only
after a whole connect: a signed-in owner's step-up, the API's start, the provider's authorize
page, and the API's callback writing the code. Since DEC-2125 the step-up is the CLI's
`mandate step-up confirm-connect`, and the API's verifier must accept the record it writes; a unit
test that joins the two is impossible, since `mandate-cli` and `mandate-api-server` share layer 9
and `xtask/layers.toml` forbids a dependency between them.

The executor the manager starts after the exchange checks its connection against Alpaca's paper
host only under `--broker alpaca-paper`, which the cell does not run: the cell's executor stays on
the fixture broker, so no `ConnectionChecked` is written and the connection never establishes there.

### Decision

1. **The drill.** `cargo xtask cell drill exchange`, and the CI job `cell-exchange` on a cell of its
   own (DEC-464's ten minutes), plain and seeded. It opens and founds a fresh paper workspace,
   signs in as the demo owner through the stub issuer, and asks the API for a `connection`
   challenge bound to the connect's digest, which the drill computes itself from the workspace
   and the accepted disclosure (DEC-2622 v1). It runs `confirm-connect` with the code the challenge
   shows, in a one-off Job of the control process, and checks the CLI confirmed that same digest.
   It starts the connect with that `cli_confirm` evidence, follows the start's authorize page to
   the `oauth-fixture` pod through a port-forward (the page the API names under DEC-2633), and
   calls the API's callback with the code and state the fixture redirects with.
2. **What it holds.** The manager's token-exchange pod for the connection reaches `Succeeded`, the
   connection's code entry is the take's tombstone or gone, and its token entry is format 1: a
   bearer token, both scopes, nothing else. The drill stops there; it asserts no
   `ConnectionChecked` and no establishment.
3. **The token never leaves the vault's pod.** The drill reads the entries with the cell's fake
   root token on a script's stdin, never an argument, and the token entry's access token is
   replaced by `present` inside the pod before anything is read out.
4. **The seeded bug.** The CLI confirmation is skipped, so the start must refuse its `cli_confirm`
   evidence, `step_up_missing`; a seeded run counts only with that code.
5. **The CLI and API's contract test.** Because the start must succeed on the record the real CLI
   wrote, this drill is the end-to-end check that the CLI's `StepUpConfirmed` and the API's
   `CliConfirm` agree on kind, digest, principal and code (DEC-2125 item 3). Both sides also take
   the kind and the digest from one source each (`StepUpActionKind::code()`,
   `oauth_connect_digest`).
6. **Establishment end to end is a separate story.** It needs the cell's executor to read the
   account from the fixture: an account reader swapped in at the executor's `Start.account`,
   debug builds and the dev and ci tiers only, behind DEC-2633's fences, with a test that a release
   build cannot select it, and the fixture answering `GET /v2/account`. It is never a paper-host
   override: `mandate-alpaca` compiles in one host and `mandate-shell` pins that no `--host` exists.

7. **The late owner.** `cargo xtask cell drill exchange-late`, in the same `cell-exchange` job,
   runs the same connect but follows the authorize page and calls the callback only once the
   manager's exchange pod for the connection is `Running`, and 5 s after, past its first takes. A
   real owner at Alpaca authorizes after the role provisioner and the manager have started the
   exchange, so this is the order production runs in, and it holds only because the exchange waits
   for its code until the request's deadline ([DEC-2363](trading-core.md#dec-2363) item 3). It holds on the
   same evidence as item 2, and its seeded run plants item 4's bug.

8. **The exchange image carries a CA bundle.** The drill found every exchange pod ending
   `endpoint_failed` within its first second, before any code existed: its HTTP client, built on
   reqwest's platform verifier, finds no roots on the image's empty base and cannot be built, so
   nothing was ever sent and the fixture saw no request. Reproduced outside the cell with every
   CA path hidden. `cargo xtask cell up` copies the host's bundle into the `token-exchange` image as
   it does for the `api` image, and the image's tag covers its Dockerfile so a cell rebuilds it.
   Any other image of `mandate-token-exchange`, staging's included, must carry roots too.

### Rationale

The drill exercises every process in the connect's path together, in the order production runs
them, with the CLI and API exactly as built, so the founder's first staging connect is not the
first time they meet. Stopping at the token entry keeps the drill on the fixture broker, which
never dials a broker.

### Alternatives considered

- **A unit test from the CLI into the API's verifier.** Not buildable under the layer rules; a new
  tool-layer test crate would cost a crate entry for what this drill already covers.
- **Assert establishment in this drill.** Needs the account reader of item 6, a broker-connector
  change with its own review, so it is that story's.
- **Point the executor's paper host at the fixture.** Rejected under item 6.

<a id="dec-2065"></a>

## DEC-2065: Self-hosted, ephemeral GitHub Actions runners on DigitalOcean run CI's cell jobs, under a $150 monthly cap of their own

| | |
|---|---|
| **Status** | Accepted (founder); item 6's team of the runners' own amended by [DEC-2074](infrastructure.md#dec-2074); item 6's "nothing inbound" given an opt-in debug window by [DEC-2077](DEC-2077.md), Proposed |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | The founder, on 2026-10-26, in the coordinator session: "Okay use digital ocean, just unblock us", approving the spend for self-hosted runners with a cap of 150 USD a month, kept separate from DEC-2011's hosting budget (as relayed in lane RN's dispatch). Items 2 to 7 are lane RN's engineering readings under DEC-79; each only tightens what the founder decided |
| **Story or claim** | E21-64 (lane RN), [#2509](https://github.com/kunwarshivam/mandate/pull/2509); [DEC-2011](infrastructure.md#dec-2011), [DEC-2012](infrastructure.md#dec-2012), [DEC-610](earlier-600-899.md#dec-610), [DEC-464](earlier-303-599.md#dec-464), [DEC-2609](process.md#dec-2609) |

### Context

CI on GitHub-hosted runners was starved: 19 `ci` runs queued with none in progress, `main` without
a finished run for over an hour, and the cell jobs timing out at ten minutes on contended runners.
The founder chose self-hosted runners on DigitalOcean and approved the spend.

### Decision

1. **The founder's decision.** CI gets self-hosted GitHub Actions runners on DigitalOcean. Their
   spend is capped at **150 USD a month**, apart from DEC-2011's 150 USD hosting cap.
2. **The fleet: three `s-4vcpu-8gb` droplets in `sfo2`, 144 USD a month.** DigitalOcean's droplet
   pricing page (<https://www.digitalocean.com/pricing/droplets>, read 2026-10-26) lists the Basic
   `s-4vcpu-8gb` (4 vCPUs, 8 GiB, 160 GiB SSD, 5,000 GiB transfer) at 48.00 USD a month
   (0.07143 USD an hour, capped at the monthly price). 3 × 48 = **144 USD**, 6 USD under the cap.
   Inbound transfer is free and each droplet's outbound allowance is far above what CI pulls.
   Alternatives at the same page: four of them would be 192 USD; `s-8vcpu-16gb` is 96 USD, so one
   plus one `s-4vcpu-8gb` is two runners for the same 144 USD; `c-4` is 84 USD and
   `g-4vcpu-16gb` 126 USD, one runner each; six `s-2vcpu-4gb` at 24 USD fit, but 4 GiB holds a k3s
   cluster and a Rust build poorly. `modules/budget` refuses a plan whose estimate, from
   DigitalOcean's own size list, passes the cap, and the module refuses a cap above 150 USD. The
   founder sets a billing alert at 150 USD in the console (no API sets one), preferably in a
   DigitalOcean team of its own so the alert and the token cover the runners alone.
3. **Only the eleven cell jobs move.** Three runners take three jobs at a time, while GitHub gives
   the repository up to twenty, so moving all 28 Linux jobs would slow CI. The cell jobs are the
   ones timing out, the heaviest, and the ones needing Docker; moving them also frees GitHub's
   slots for the rest. `select` computes one output, `runner`, and the cell jobs run on
   `${{ needs.select.outputs.runner }}`. Lane DR, which owns CI's job budgets, may widen the set.
4. **Forks never reach them.** Three fences, in this order of strength:
   - **The repository setting** "Require approval for all outside collaborators" (`GET
     /repos/{owner}/{repo}/actions/permissions/fork-pr-contributor-approval` reads
     `all_external_contributors`). A `pull_request` run uses the workflow files of the pull
     request's own merge commit, so a fork can rewrite `ci.yml` to ask for `mandate-ci-do` directly
     and never pass the guard below. Until someone approves its run, nothing of it runs anywhere.
     The founder confirms the setting before the first apply, and each host reads it again with
     the App's token before registering every runner, registering none unless it reads
     `all_external_contributors` (it fails closed: an unreadable setting registers nothing).
     A workflow cannot check it: `GITHUB_TOKEN` cannot be granted Administration read. A runner
     already registered and idle when the setting is turned off stays until its next job, which
     the hook below still screens. **No one, agent or founder, approves
     a fork's workflow run while `CI_RUNNER` is set or the runners are up.** To run one, first
     unset `CI_RUNNER` and power the droplets off.
   - **The runners' job-started hook** (`ACTIONS_RUNNER_HOOK_JOB_STARTED`), baked into the image,
     so no workflow can change it. Before any step it fails every job that is not this
     repository's: a `pull_request` whose head repository is not this one, a `push` or
     `workflow_dispatch` off `refs/heads/main`, any other event, or one it cannot read. An
     approved fork's job that names the label directly is refused there. It reads `GITHUB_EVENT_NAME`,
     `GITHUB_EVENT_PATH`, `GITHUB_REPOSITORY` and `GITHUB_REF`, which no agent could test on a
     real runner, so the founder's first run after the switch is its smoke test; a missing value
     fails every job, never admits one.
   - **ci.yml's guard.** `runner` is `vars.CI_RUNNER` only for a `pull_request` whose head
     repository is this repository, or another event on `refs/heads/main`; anything else, and any
     run while the variable is unset, is `ubuntu-24.04`. It keeps the repository's own workflow
     from sending a fork there, and makes the change do nothing until the founder sets
     `CI_RUNNER`; unsetting it is the rollback.

   GitHub advises against self-hosted runners on public repositories; these fences, the fresh
   container per job (item 5) and the host's network fence are the compensating controls. The
   runners carry a label only they use (`mandate-ci-do`) and register on this repository alone; a
   repository of a personal account has no runner groups.
   They coexist with the runner [DEC-2614](https://github.com/kunwarshivam/mandate/pull/1873)
   (Proposed) reserves for `infra.yml`, label `mandate-infra`: GitHub gives a job only to a runner
   holding every label it asks for, so the cell jobs (asking `mandate-ci-do`) never reach that
   runner and `infra.yml` (asking `mandate-infra`) never reaches these. Their registration follows
   `deploy/FOUNDER-STEPS.md` §I option (a), a root-only supervisor that re-registers after every
   job, with a GitHub App's one-hour tokens in place of a standing personal access token. The
   founder's 150 USD covers these CI runners alone, not DEC-2614's infra machine.
5. **Every job on a fresh runner, without the host's Docker socket.** On each droplet a root-owned
   systemd loop signs a JSON web token with a GitHub App's key, mints a one-hour installation token
   narrowed to this repository and `administration: write`, asks for a just-in-time runner
   configuration (single-use by GitHub's design, so `--ephemeral` in effect), revokes the token,
   and starts a new Sysbox container from a baked image with that configuration. The container
   runs its own Docker daemon, so k3d and the jobs' `sudo` work as on GitHub's runners, while root
   inside it is an unprivileged user on the host. When the job ends the runner exits and the
   container is removed with everything it wrote, and the loop starts the next. The image holds
   the runner (`2.338.1`, SHA-256 pinned), Ubuntu 24.04 (pinned by digest), Docker, and the
   repository's toolchain from `main`'s `.cursor/install.sh`; it is rebuilt once it is a day old, before the next job, so
   jobs start with the pins already installed. The host's root password is locked, and an
   idle droplet rebuilds within a day of its next job.
6. **What it does not guard, said plainly.**
   - The App's key is on each host, root only, and in the droplet's metadata. Its blast radius is
     `Administration: write` on this repository, which can also change the repository's
     settings. GitHub has no narrower permission for a personal account's repository: a
     just-in-time configuration (`generate-jitconfig`) and a registration token both need
     `Administration: write`, and only an organization offers "Self-hosted runners" alone. A job that escaped Sysbox's user namespace (a kernel bug) could read
     the key. Only branches of this repository run there, so such a job's author can already push
     code. Moving the repository to an organization would allow the narrower "Self-hosted runners"
     permission (E21-64c).
   - The key is also readable, in the droplets' user data, by anyone holding Droplet read on the
     DigitalOcean team, through its console and API. So the runners get a team of their own, and
     the key is rotated every 90 days, which replaces the droplets.
   - Egress is limited by port, not by host: DNS, NTP and ports 80 and 443 to any public address,
     nothing inbound. Jobs are refused the host, the metadata service and every private range
     (`10/8`, `172.16/12`, `192.168/16`, `100.64/10`, `169.254/16`), so they reach no other
     droplet or cluster in the account. A job could still send what it holds to any public host,
     and it holds no secret: `ci.yml` passes no secret to any job, and its `GITHUB_TOKEN` reads
     contents only. Limiting egress to named hosts needs a proxy (E21-64a).
   - The image runs `main`'s `.cursor/install.sh` with `sudo` inside the image's build, daily.
     `main` is trusted, as it is for every runner.
   - The droplets pull from Docker Hub anonymously, so its per-address limit applies to each
     droplet, as it applied to GitHub's shared addresses before (E21-64b).
   - The GitHub Actions cache is shared with the repository's other runs, as on GitHub's runners.
7. **Secrets.** Agents never see the DigitalOcean token, the App's key or the state passphrase, and
   never run `plan` or `apply`. The founder applies the root from his own machine; the token comes
   from `DIGITALOCEAN_TOKEN` and the key and passphrase from `TF_VAR_*` variables marked
   `sensitive`. No variable is stored in state. The DigitalOcean provider stores a SHA-1 digest of
   the boot script rather than its text, and the local state is encrypted as DEC-2012 item 5
   encrypts the hosted environments'. The runbook is
   [docs/runbooks/ci-runners.md](../../runbooks/ci-runners.md).

### Rationale

The founder's words decide the platform and the cap. Within them, the fleet buys the most CPU
and memory per runner the cap allows for the jobs that need it most, and the design keeps the two
properties a public repository needs from self-hosted runners: no code from outside the repository
runs on them, and no job inherits anything from the one before.

### Alternatives considered

- **A runner directly on each droplet, re-registered after each job.** Jobs need `sudo` (package
  installs, `/usr/local/bin`) and Docker, so they would hold root on the host, the App's key
  included, and a job could leave files for the next. Rejected.
- **A fresh droplet per job.** The cleanest isolation, but it needs a controller holding a
  DigitalOcean token and a minute or more of boot per job. Kept for later if Sysbox falls short.
- **Rootless Docker per job user.** No `sudo` for jobs, so ci.yml's install steps would need
  rewriting, and k3d under rootless Docker needs cgroup delegation it does not reliably get.
- **A fine-grained personal access token.** The same `Administration` permission, but long-lived
  and tied to the founder's account, sitting on every droplet as a bearer token.
- **Moving every Linux job.** Fewer concurrent jobs than GitHub gives, so slower CI.

<a id="dec-2070"></a>

## DEC-2070: The local cell runs a second deployment manager for the research agent's workspace, with a connection of its own

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 for the local and CI cell alone; the coordinator approved option (A) on 2026-10-26) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | An agent (Claude Code, lane GW), under DEC-79 |
| **Story or claim** | E21-54 step 3; flow map step 2, "Research agent's runtime in the cell"; DEC-2247 item 2; DEC-1170; DEC-1172; DEC-1177; DEC-2060 |

### Context

DEC-2247 item 2 deploys the demo's research agent alone in a second workspace, `<workspace>r`, so
that its research is the only agent on its control stream. A deployment manager serves exactly one
workspace: `mandate-deploy-manager` reads the control stream of the chart's `api.workspace`
(DEC-1172), which the demo points at the run's own workspace. Recording a connection in
`<workspace>r` would write its deployment at version 2, but no manager reads that stream, so
nothing would start its runtime.

### Decision

1. **A second manager, for one more workspace.** When `deployManager.researchWorkspace` names a
   workspace, the chart renders a second manager Deployment, `deploy-manager-research`, for that
   workspace alone.
   - It runs the same image and the same launch.
   - It has its own ServiceAccount and its own Role, with the first manager's verbs on pods and
     ServiceAccounts in this namespace and nothing more.
   - It has its own admission policy and its own accounts policy, each with its own binding.
     Each is named for the second manager and matches only its ServiceAccount, with exactly the
     first manager's rules, so every pod and ServiceAccount it makes is bounded as DEC-1177 bounds
     the first's. Neither manager's policies match the other's account, so neither can make a pod
     that no policy checks. A chart invariant test renders both managers and requires two disjoint
     pairs of policies whose rules are the same.
   - Its pods' names hash its own workspace with the subject (`Kubernetes::pod_name`), so they never
     meet the first manager's.
   - Empty, nothing renders. Only the local and CI cell's values name it; `cargo xtask cell demo`
     points it at `<workspace>r`, as it points the first at the run's workspace (DEC-2010).
2. **A connection of its own.** The research workspace's paper connection is `demo-research-paper`,
   distinct from the demo's `demo-paper`. So it gets:
   - its own executor ServiceAccount, the connection's hex (DEC-2336);
   - its own vault role and credentials, provisioned beside the demo connection's (DEC-2060);
   - its own account stream, a fresh account per run, as the demo's has (DEC-2379).

   Step 3 runs the executor's connect check for it and records it, as step 2 does for the demo's,
   before the research agent is deployed. The deployment is then written at version 2, and the
   second manager starts its runtime and executor.
3. **Least privilege holds per manager.** Neither manager's Role names the other's pods or accounts
   by any rule but the shared namespace's, which is the residual DEC-1177 already records for one
   manager. Each process's pod remains bounded by the admission policy.
4. **The production path is a manager serving a list of workspaces.** That change is to a
   safety-critical crate, and it is recorded as a backlog row (E21-54g). This decision is the
   local cell's interim: the chart refuses `deployManager.researchWorkspace` at any tier but `dev`
   or `ci`, as it refuses the fixture OAuth provider (DEC-1176), and a chart test renders every
   hosted values file and finds no second manager. *Amended by [DEC-2080](infrastructure.md#dec-2080): staging may also render it,
   only beside the model gateway; production never.*
5. **The model gateway follows the research workspace.** `cargo xtask cell demo` points
   `modelGateway.workspace` at `<workspace>r` with the second manager, since its agent is the one
   that researches (DEC-2397 item 5).

### Rationale

A second instance of the reviewed manager adds no code to a safety-critical crate and keeps every
existing bound. A separate connection keeps the executor's identity, its vault role and its account
stream one per connection, which is what DEC-2060 and DEC-2336 assume.

### Alternatives considered

- **(B) One manager over several workspaces.** It is the right production shape, but it is a change
  to a safety-critical crate and its admission story. Recorded as the follow-up.
- **(C) The research agent back in the demo's workspace.** Refused: it reverses DEC-2247 item 2,
  and the cell's research source assumes one agent on its control stream (DEC-2214 item 1).

<a id="dec-2071"></a>

## DEC-2071: The local cell's research workspace has a clock of its own

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 for the local and CI cell alone, completing DEC-2070) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | An agent (Claude Code, lane GW), under DEC-79 |
| **Story or claim** | E21-54 step 3; E21-29; DEC-1010; DEC-2070; DEC-2247 item 2 |

### Context

DEC-2070 gives the research workspace, `<workspace>r`, a deployment manager and a connection of its
own, so the manager starts that workspace's runtime and executor. Each of them reads its ticks from
its own workspace's clock stream, `clock:<workspace>r` (DEC-1010). The cell's scheduler is the single
writer of the hosted workspace's clock stream (`api.workspace`) and of no other. So nothing writes
the research workspace's clock: its runtime never passes a tick, so it never researches on its
cadence, sizes or asks; and its executor never reconciles.

### Decision

1. **A second scheduler, for the research workspace.** When `deployManager.researchWorkspace` names
   the research workspace, the chart renders a second scheduler Deployment, `scheduler-research`.
   - It runs the first scheduler's image and launch, but its `--workspace` is the research
     workspace, so it writes `clock:<workspace>r` and no other stream.
   - It keeps the first's process entry: its journal role (`mandate_scheduler`), its egress
     (Postgres and the time sources, asking none until `egress.time` names one), its resources and
     its guaranteed class. It has no ServiceAccount token and no ingress.
   - It renders only beside the second manager. That manager's guards already refuse it at any tier
     but `dev` or `ci`, and refuse any research workspace but the hosted one's ULID with `r`
     (DEC-2070 items 1 and 4).
2. **The demo waits on it.** `cargo xtask cell demo` already points the research workspace at
   `<workspace>r` (DEC-2070 item 5). It also waits for both schedulers to roll out, as it waits for
   both managers, so no earlier run's scheduler still writes another workspace's clock.
3. **The chart's invariants cover it.** A chart test renders the second manager and scheduler and
   requires every pod invariant to hold for both: hardening, default deny, exact egress, no
   ingress, guaranteed resources, colocation, and secrets as files. It also requires that the second
   scheduler differs from the first only in its name and its workspace.

### Rationale

One scheduler per workspace is the shape infrastructure design §3.1 already gives every workspace.
A second instance of the reviewed scheduler adds no code to any crate. The production path is the
same as DEC-2070's: a workspace deployment per workspace (E21-54g).

### Consequences

- The research workspace's runtime and executor tick in the local and CI cell, so what they do next
  depends only on the runtime's own wiring. That wiring is still a gap in step 3, named in the run.
- A second writer of `clock:` streams runs in the cell. Each stream still has exactly one writer,
  because each scheduler takes ownership of its own workspace's stream alone.

<a id="dec-2072"></a>

## DEC-2072: CI's required aggregates `fast` and `full` run on `select`'s runner, falling back to GitHub's

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent (Claude Code, lane IAC), under DEC-79: a reversible CI change, asked for by the coordinator. It changes nothing until the founder sets `CI_RUNNER`, and weakens no check |
| **Story or claim** | No story; CI. [DEC-2610](process.md#dec-2610) item 3, [DEC-2065](infrastructure.md#dec-2065) item 4, [#2509](https://github.com/kunwarshivam/mandate/pull/2509) |

### Context

On 2026-10-26 no post-merge `ci` run on `main` completed after 08:35 UTC. Run 40307542804, on
`a7f52ba9e77ad8d70a8b620b335a64413487746d`, held `main`'s group `ci-refs/heads/main` for six hours. 47 of its 50 jobs had finished
by 14:35 UTC, but its aggregates `fast` and `full` sat queued for an `ubuntu-24.04` runner behind
the account's concurrent-job cap. So every later push run waited in the group and was replaced
by the next one (DEC-2610 item 3). Each aggregate is one short job on GitHub's pool, but it queues
behind every pull request's mutants shards. DEC-2065 already sends the cell jobs to
`select`'s `runner` output.

### Decision

1. `fast` and `full` run on `${{ needs.select.outputs.runner || 'ubuntu-24.04' }}`.
2. `runner` is `vars.CI_RUNNER` only for `main`'s own runs and for pull requests from this
   repository's branches (DEC-2065 item 4). Otherwise it is `ubuntu-24.04`. `runner` depends on
   no step, so a `select` that failed in a step still names it. The fallback covers a `select`
   that never produced outputs, because it was skipped or cancelled first. Either way the
   aggregate starts and fails on `SELECT_RESULT`, as today.
3. Until the founder sets `CI_RUNNER`, both run on `ubuntu-24.04` exactly as before. Nothing
   changes, and unsetting the variable remains the rollback.

### Rationale

The aggregates hold the required verdicts. On the self-hosted pool, a run on `main` that has
finished its parts gets its verdict without waiting for a GitHub slot, and releases the group. Each
aggregate is a one-minute script with `permissions: {}`, no checkout and no secret, so the fork
fences of DEC-2065 item 4 and its job-started hook apply to it as they do to the cell jobs.

Once `CI_RUNNER` is set, the aggregates share the three-runner pool with the cell jobs, so a
pull request's cell jobs can queue ahead of `main`'s one-minute aggregate. If the pool is offline,
a verdict waits instead of failing; GitHub keeps a queued job for up to 24 hours. Unsetting
`CI_RUNNER` returns every job to GitHub's pool at the next run.

### Alternatives considered

- **`cancel-in-progress: true` on `main`:** DEC-2610 item 3 rejects it, since no run on `main`
  would finish through a burst of merges.
- **Cancelling a stuck run by hand:** frees the group once. The next run starves the same way.

<a id="dec-2073"></a>

## DEC-2073: the cell's demo snapshot is classified as of the run's start

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision for the paper-only cell under DEC-79, at the coordinator's direction on 2026-10-26; the rule set's 7-day `etp_classification_max_age_s` and the eligibility floor are unchanged; flagged for the founder's look after the fact) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | lane DR (`claude-code`), at the coordinator's direction |
| **Story or claim** | E21-54, the cell's demo. Lane GW's repro on 2026-10-26: the binding gate denied the demo's SPY opening `leveraged_etp_not_enabled`, because `deploy/cell/demo/instrument.json` carried `etp_classified_at` 2026-10-05, older than the rule set allows from 2026-10-12 on |

### Decision

1. **A run stamps the classification instant.** `cargo xtask cell demo`, and the executor drill,
   register `deploy/cell/demo/instrument.json` with `etp_classified_at` replaced by the instant the
   run starts (UTC, to the second). Every other byte stays as committed, so the snapshot is still
   canonical JSON. The committed value is a template.
2. **What it asserts.** The fixture's classification (`etp` `plain`, from the Nasdaq Trader symbol
   directory) is asserted as of the run's start. No source is read: the cell has no classifier, and
   Alpaca's asset attributes do not say whether a fund is leveraged or inverse.
3. **Nothing else moves.** The rule set's 7-day `etp_classification_max_age_s`, the eligibility
   floor's fail-closed reading of an over-age or missing classification, and the snapshot's other
   members are unchanged. A run registers its snapshot within the run, so in the cell the floor's
   age check is met by this assertion, not by a source.
4. **Disclosed.** The demo guide says so under "Not yet real". A real classifier is E2-8's, and the
   cell's use of it is the backlog row `E2-8-cell-etp-classification`.

### Rationale

The floor's rule is right: a classification older than a week is not trusted, and an unclassified
fund is treated as leveraged. A dated fixture made every demo run after 2026-10-12 fail that rule,
for the fixture's age, not the fund's. Re-dating the file by hand would go stale again in a week.

**What this claims, plainly.** The fixture claimed "plain as of 2026-10-05". The stamp claims
"plain as of this run" with no source read, so in the cell the freshness check is satisfied by
assertion. That is a new claim, not a re-statement. It is acceptable here only because the cell is
paper-only, the instrument is SPY, a plain fund, and `mandate config register` accepts no `etp` but
`plain` and no source but the symbol directory, so the stamp can never admit a leveraged or
unclassified fund. It is not a pattern for any environment that trades: there the classification
comes from a source (E2-8). The founder may revert it after the fact (DEC-79).

### Alternatives considered

- **Raise the max age, or skip the check in the cell.** Rejected: it weakens a safety rule (DEC-79).
- **Classify from the broker.** Rejected: Alpaca's asset record carries no leverage attribute.
- **Re-date the committed file.** Rejected: it would fail again seven days later.

### Tests

`mandate-executord`'s `the_demo_opening_passes_the_floor_only_on_a_snapshot_classified_within_its_age`
runs the demo's opening through the real binding gate on the demo's own rule set: 22 days old, as
the checked-in instant reads on 2026-10-27, it is denied `leveraged_etp_not_enabled`; classified at
the run's start it is allowed; exactly seven days passes and a second more is denied. xtask's
`a_run_registers_the_snapshot_classified_at_its_start` checks the stamp moves only that member.

<a id="dec-2074"></a>

## DEC-2074: The CI runners may live in the founder's only DigitalOcean team

| | |
|---|---|
| **Status** | Accepted (founder) |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | The founder, on 2026-10-27, as relayed by the coordinator: they can have only one DigitalOcean team, so they applied the runners in their existing team rather than a separate `mandate-ci` team, with the billing alert covering the whole team bill (existing spend plus 150 USD) and the token revoked after the apply. Lane RN records the trade-offs |
| **Story or claim** | E21-64; amends [DEC-2065](infrastructure.md#dec-2065) item 6 and `docs/runbooks/ci-runners.md` §2, which asked for a team of the runners' own |

### Decision

1. **The runners may share the founder's one DigitalOcean team.** DEC-2065 item 6's "the runners
   get a team of their own" becomes a preference, not a requirement. Everything else in DEC-2065
   stands, including the 150 USD cap, which `modules/budget` still enforces on every plan.
2. **What sharing gives up, and what replaces it.**
   - **The billing alert covers the whole team's bill.** It is set at the team's other spend plus
     150 USD, so it can no longer show the runners alone. The plan's own cap check still refuses
     a fleet estimated over 150 USD.
   - **The token can delete or change any droplet, firewall or tag in the team**, staging's and
     production's cluster nodes and `modules/network`'s node firewall included. Its scopes are
     per resource type, not per project. So it is revoked as soon as each apply ends, and a new
     one is made for the next apply. This module's own firewall and tag apply only to the
     `mandate-ci-runner` tag, so the runners' plan touches nothing else in the team.
   - **Any team member with Droplet read can read the runners' user data**, which holds the
     GitHub App's private key. So every current member must be trusted with that key, and it is
     rotated whenever a member joins, as well as every 90 days (DEC-2065 item 6). Rotating it
     replaces the droplets.
   - **The runners share the region's default VPC.** They set no VPC, so in `sfo2` they likely
     land in the VPC of the staging and production clusters, whose node firewall admits that
     VPC's whole range. Jobs are still refused every private range by the host's fence (DEC-2065
     item 6), but a job that escaped Sysbox, or a compromised host, would sit on the clusters'
     network. A separate team kept them apart. Until the runners get a VPC of their own
     (E21-64e), this is a residual risk.
   - **The alert cannot tell the runners' spend from the rest**, so growth elsewhere can mask an
     overspend; the plan's cap check is what holds the runners to 150 USD.
3. **If a second team becomes possible**, moving the runners there restores DEC-2065 item 6 as
   first written. The move is a `tofu destroy` in this team and an apply in the new one.

### Rationale

A second DigitalOcean team is not available to the founder. Each lost property has a procedural
replacement (revoking the token, rotating the key on membership change), and the cap is still
enforced in code.

### Alternatives considered

- **No self-hosted runners until a second team exists.** CI stays starved, which DEC-2065 exists
  to end.
- **A DigitalOcean project instead of a team.** Projects group resources but scope neither
  billing alerts nor tokens, so they buy none of these properties back.

<a id="dec-2075"></a>

## DEC-2075: CI's `select` runs on the fork-guarded runner expression, so no cell job needs a hosted runner

| | |
|---|---|
| **Status** | Accepted; item 1 (`select` on the guard) reversed by [DEC-2076](infrastructure.md#dec-2076), items 2 to 4 stand |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | An agent (Claude Code, lane RN), under DEC-79: a reversible CI change the coordinator approved. It changes nothing while `CI_RUNNER` is unset and weakens no check; it tightens the guard |
| **Story or claim** | E21-64; [DEC-2065](infrastructure.md#dec-2065) item 4, [DEC-2072](infrastructure.md#dec-2072) |

### Context

On 2026-10-27, after the founder applied the self-hosted runners and set `CI_RUNNER`, no job
reached them. Every cell job, and since DEC-2072 the `fast` and `full` aggregates, `needs: select`,
and `select` ran on GitHub's `ubuntu-24.04`. GitHub's runners started nothing for over four hours:
on pull request run 40425751885 (00:20 UTC), `select`, `fast-lint`, `mutants-plan` and `hosted-k3d`
were all still queued at 04:35, and `main`'s run 40425767178 had no jobs.

### Decision

1. **`select` runs on the guard itself.** Its `runs-on` is the guard expression, written out in
   full because a job cannot read its own output, and its `runner` output is the same expression,
   which the cell jobs and the aggregates use. A run of this repository's own branches or of
   `main` reaches the self-hosted pool without any hosted runner.
2. **The guard is tightened.** It admitted any event other than `pull_request` on
   `refs/heads/main`. A `pull_request_target` run reports the base ref, `refs/heads/main`, so had
   that trigger ever been added, a fork's pull request would have reached the runners. The guard
   now admits only:

   | Event | Condition | Runner |
   |---|---|---|
   | `pull_request` | head repository is this repository | `CI_RUNNER` |
   | `pull_request` | head repository is a fork, or deleted | `ubuntu-24.04` |
   | `push`, `workflow_dispatch` | ref is `refs/heads/main` | `CI_RUNNER` |
   | `push`, `workflow_dispatch` | any other ref | `ubuntu-24.04` |
   | `pull_request_target`, `schedule`, any other | any | `ubuntu-24.04` |
   | any | `CI_RUNNER` unset or empty | `ubuntu-24.04` |

   The runners' job-started hook still refuses every job but this repository's own (DEC-2065
   item 4), and the repository's approval setting is still the fence against fork workflows.
3. **One guard.** `only_the_fork_guard_reaches_the_self_hosted_runners` pins `select`'s
   `runs-on` and `runner` to the exact expression, refuses `CI_RUNNER` on any other line of any
   workflow, and lets a job's `runs-on` read only `select`'s output. Seeded bug: the old,
   looser guard fails it.
4. **What stays on GitHub's runners, and still needs their capacity:** `fast-lint`, the
   `fast-test` shards, `fast-pending`, `fast-python`, `fast-portable` (macOS), `fast-rh-sim-live`,
   the `full-checks` jobs, the mutation plan, its shards and their costs, `schema-mutants`,
   `hosted-k3d` and `report-main`. While GitHub starts nothing, `fast` and `full` still cannot
   pass, because they need those jobs' results. Only the cell jobs get a verdict from the
   self-hosted pool alone.

### Rationale

`select` is one small job, and three self-hosted runners take it as readily as a cell job. Leaving
it hosted made the whole self-hosted pool depend on GitHub's queue, which is the dependency
DEC-2065 exists to remove.

### Alternatives considered

- **Move every Linux job.** Three runners take three jobs at a time against GitHub's twenty, so
  CI would slow whenever GitHub's runners are healthy. Lane DR owns that budget.
- **Let the cell jobs skip `select`.** They would lose DEC-1190's scoping and run on every diff.

<a id="dec-2076"></a>

## DEC-2076: `select` goes back to GitHub's runners

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | An agent (Claude Code, lane RN), at the coordinator's direction, under DEC-79: a reversible CI change |
| **Story or claim** | E21-64; reverses [DEC-2075](infrastructure.md#dec-2075) item 1, keeps its items 2 to 4 |

### Context

DEC-2075 moved `select` onto the self-hosted runners because GitHub's runners had started nothing
for hours. The coordinator found the cause: about 200 stale queued runs, and 15 in progress, from
pull requests merged the day before, held every slot. Once they were cancelled, GitHub's runners
moved again.

### Decision

1. **`select` runs on `ubuntu-24.04` again.** Every job of `ci` needs `select`. On the
   self-hosted pool, three droplets that were down or still building their image would block
   every job of `main` and of every pull request from this repository, lint and tests included,
   which need no self-hosted runner.
2. **DEC-2075's other items stand.**
   - The guard still admits only `push` and `workflow_dispatch` on `main`, and pull requests
     from this repository; `pull_request_target`, `schedule` and the rest take `ubuntu-24.04`.
   - The guard is written out once, as `select`'s `runner` output, which the cell jobs and the
     aggregates read.
   - `only_the_fork_guard_reaches_the_self_hosted_runners` now pins `select` to
     `ubuntu-24.04` and the guard to that one output, and still holds every `runs-on` in every
     workflow to GitHub's runners or `select`'s output.
3. **If GitHub's runners stall again with a clean queue**, DEC-2075 item 1 can be applied again.

### Rationale

The self-hosted pool exists for the cell jobs. Making every job wait on it trades one single point
of failure for another.

### Alternatives considered

- **Keep `select` on the pool.** Lint and tests would stop whenever the droplets do.

<a id="dec-2078"></a>

## DEC-2078: CI runners prove their isolated runtime before registration and clean up failed listeners

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | an agent (Cursor), under DEC-79: reversible runner engineering that tightens failure handling and leaves the founder's platform, isolation and 150 USD cap unchanged |
| **Story or claim** | E21-64f, [#2780](https://github.com/kunwarshivam/mandate/issues/2780); E21-64l, [#2867](https://github.com/kunwarshivam/mandate/issues/2867); [DEC-2065](infrastructure.md#dec-2065), [DEC-2072](infrastructure.md#dec-2072) |

### Context

The founder's first apply of DEC-2065 created the expected three droplets, firewall and tag at
144 USD. Each host authenticated its GitHub App, read the fork policy and called
`generate-jitconfig`, but no listener connected. In thirty minutes the hosts created thirty
offline registrations. The manager created a registration before its first real Sysbox run,
discarded the runner id, ran the container with `--rm`, and systemd retried every thirty seconds.
The fleet exposed neither a readiness result nor bounded failure behavior.

Runner `2.338.1`, pinned by the first change, was a GitHub prerelease. GitHub published stable
`2.338.2` before the founder's apply. The symptom proves only that failure was after JIT creation
and before listener connection; it does not distinguish a Sysbox launch failure, nested Docker
failure, container egress failure or runner defect.

The hardened one-runner canary then exposed an earlier host prerequisite. Ubuntu 24.04 did not
have `/run/sshd` when cloud-init ran the user-data script. `sshd -t` therefore failed with
`Missing privilege separation directory: /run/sshd`; `set -e` stopped bootstrap before Docker,
Sysbox or the runner service existed. E21-64l records and fixes that observed cause.

### Decision

1. **Only a stable GitHub Actions runner release may be pinned.** E21-64f moves to `2.338.2`,
   checksum-verified as before. A prerelease needs its own accepted decision and a canary that
   demonstrates why the stable release cannot be used.
2. **Readiness precedes registration.** Once per host boot and built job-image digest, each host
   starts that image through `sysbox-runc`, requires the nested Docker daemon and runner binary
   to work as the job user, resolves GitHub's API and broker, and reaches both over HTTPS. A failed
   preflight creates no `mandate-ci-do` registration.
3. **A registration has one owned lifecycle.** The manager keeps the JIT response's runner id.
   The runner wrapper propagates listener failures. When the container returns, the manager
   deletes that id (a completed ephemeral runner may already have removed itself), then retries
   failures with a bounded backoff. A restart first removes the prior attempt recorded by that
   host, so a crash cannot start a registration flood.
4. **Bring-up is one host, then three.** The root accepts a runner count from one through three,
   default three. The founder first applies one host, waits for one idle listener and runs the
   secret-free smoke workflow. Only a green smoke may scale the root to three; only three idle
   listeners and a green three-job smoke may set `CI_RUNNER`.
5. **Failure stays on hosted CI.** `CI_RUNNER` remains absent throughout bring-up. A failed
   canary is destroyed before another implementation is tried. Agents never apply or receive the
   App key, DigitalOcean token or state passphrase.
6. **The smoke has one narrow runner-selector exception.** Before `CI_RUNNER` exists,
   `runner-smoke.yml` may select `mandate-ci-do` only when its event is `workflow_dispatch` and
   its ref is `main`; every other event or ref resolves to `ubuntu-24.04`, where the smoke refuses
   that it is not self-hosted. This amends DEC-2075 item 3 only for the founder-dispatched canary.
   The checked closed world permits the exact expression once and still refuses the label
   everywhere else.
7. **A volatile runtime prerequisite precedes its validator.** The host creates root-owned
   `/run/sshd` at mode 0755 before `sshd -t`. The plan test proves that order so a future image
   cannot silently move SSH validation ahead of the directory that OpenSSH requires.

### Rationale

The first live run found the exact boundary the original tests could not exercise. Proving the
runtime before registration removes false capacity from GitHub. Owning the returned id closes the
orphan lifecycle. A stable runner removes an unnecessary prerelease variable without claiming it
was the sole cause. One host buys the missing evidence for 48 USD rather than recreating the
entire 144 USD fleet, while the existing rollback keeps required CI on GitHub's runners.

### Alternatives considered

- **Reapply the same fleet:** rejected; three identical hosts reproduced the failure.
- **Change Ubuntu or replace Sysbox immediately:** rejected without evidence. Sysbox lists the
  current host as supported, and weakening the fresh-container isolation is not a diagnostic.
- **Leave failed registrations for inspection:** rejected. Their offline state adds no cause and
  can strand jobs if the routing variable is set.
- **Open SSH or put raw logs in GitHub:** rejected. SSH changes DEC-2065's host boundary, and raw
  bootstrap output can include infrastructure detail. The preflight reports only its stage and
  exit status through the host journal; the remotely visible contract is no registration before
  readiness.

<a id="dec-2079"></a>

## DEC-2079: Every process the workspace chart builds is released and signed; the scheduler is promoted, and each process held back names the overlay value it lacks

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | an agent (Claude Code, lane IAC), under DEC-79: a reversible release change, asked for by the coordinator. It adds checks and weakens none |
| **Story or claim** | E21-58 (lanes GO and RS inactive); [DEC-971](earlier-900-1999.md#dec-971), [DEC-2015](infrastructure.md#dec-2015), [DEC-2016](infrastructure.md#dec-2016), [DEC-2048](infrastructure.md#dec-2048) |

### Context

Staging was applied on 2026-10-19. Its cluster, Flux, journal and vault came up, but its
workspace never did. `deploy/gitops/clusters/staging/images.yaml` holds only `api` and `mandate`,
because `cargo xtask gitops promote` maps only the binaries `release.yml` publishes. That map did
not include the scheduler, the dispatcher, the model gateway or the data process: the local cell
builds those four from the checkout, but no release built them. Without the scheduler no agent
ticks, so a hosted cell could not run an agent. Nothing checked that the two lists agree.

### Decision

1. `release::BINARIES` ships `mandate-scheduler`, `mandate-dispatcher`, `mandate-datad` and
   `mandate-gatewayd`. Each is built, compared for reproducibility, signed, published as its own
   image and indexed like the other binaries.
2. `promote`'s `CHART_IMAGE_FOR_BINARY` adds `mandate-scheduler` as `scheduler`. Rendered with
   staging's values, the chart takes it with no new value, and it is the process without which no
   agent ticks.
3. The other three are released but not promoted. They are listed in `RELEASED_NOT_PROMOTED`,
   each with the overlay value the chart refuses to render without. Promoting the image alone
   would stop the whole workspace release from reconciling.
   - `data` needs `data.instruments`, which today is a recording of closes that only a dev or CI
     cell lists. A hosted cell needs a market-data source first.
   - `dispatcher` needs `dispatcher.recipient`, the founder's mail address, which no committed
     overlay may hold (DEC-2511). *Amended by [DEC-2080](infrastructure.md#dec-2080): the recipient is an opaque
     id, not an address, so staging's overlay names `founder` and the dispatcher is promoted.*
   - `model-gateway` needs `modelGateway.workspace` and `modelGateway.deployment` to render. Its
     two monthly budgets are a separate matter: without them it refuses to start, and they are the
     founder's spending values (DEC-2282). *Amended by [DEC-2081](infrastructure.md#dec-2081): staging's overlay names
     them and the gateway's registry; it is promoted once the provider key is in the vault.*

   Each moves to the promote map in the change that gives the overlays its value.
4. Two tests hold this:
   - `every_built_chart_process_is_promoted` reads `processes.json` and fails when a built
     process (other than the third-party `postgres`) is neither promoted nor held.
   - `every_held_process_is_released_and_not_promoted` requires every held process to be
     released, and not also promoted.

   With `every_charted_binary_is_released`, the gap of 2026-10-19 cannot recur silently.
5. The released gateway image holds no recorded research bundle; only the local cell's interim
   image carries it, as for the runtime (DEC-2246).

### Rationale

Promotion is the only route an image takes into a hosted cell (DEC-2016). So every process the
chart runs must be released, and none may be promoted before its overlay can render it. A test
over the chart's own process list, with a named reason for each process held back, keeps both true
as processes are added.

### Alternatives considered

- **Build those four images inside the cluster or from a branch:** this breaks DEC-2015's rule
  of signed digests from `main` only.
- **Leave the processes unrendered in hosted cells:** without the scheduler no agent runs.

<a id="dec-2080"></a>

## DEC-2080: Staging runs the dispatcher with an opaque recipient, and may run the research workspace once the model gateway is in

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | an agent (Claude Code, lane IAC), under DEC-79, at the coordinator's direction. These are reversible infrastructure choices for a paper-only environment. No safety rule is weakened: the broker is paper, the model meter's caps bind, and production stays refused |
| **Story or claim** | E21-58, always-on staging; amends [DEC-2070](infrastructure.md#dec-2070) item 4 for staging only; [DEC-2079](infrastructure.md#dec-2079), [DEC-2511](notifications.md#dec-2511), [DEC-2516](notifications.md#dec-2516), [DEC-2282](DEC-2282.md) |

### Decision

1. **The dispatcher in staging.** `dispatcher.recipient` is an opaque recipient id (`[a-z0-9_]`,
   1 to 64 characters), never an address. Staging's overlay names `founder`. The dispatcher mails
   only through the recorded transport, which sends nothing while the general email footer is
   unresolved (DEC-2511), so no secret is needed and none is committed.
   - `promote` now pins the dispatcher, moving it from DEC-2079's held list.
   - Production's overlay names no API values yet. `promote production` copies staging's digests,
     so the change that first promotes production also names a dispatcher recipient in its overlay.
2. **The research workspace in staging.** The chart renders the second deployment manager, and
   with it the second scheduler (DEC-2071), in a dev or ci cell, as before. It now also renders
   them in staging, but only when `images.model-gateway` is named: every research call is then
   metered under the workspace and deployment caps DEC-2282 sets.
   - Production stays refused.
   - No overlay names a research workspace yet. Staging's overlay names it in the change that
     promotes the gateway, which stays held until GW's model registry lands.
3. **Tests.**
   - `test_the_second_manager_runs_only_in_a_dev_or_ci_cell_or_staging` renders dev, ci and
     staging, and refuses production.
   - `test_staging_runs_the_second_manager_only_beside_the_model_gateway` refuses staging without
     the gateway.
   - The promote tests pin the dispatcher.

### Rationale

The founder wants agents running continuously on staging, on paper. The dispatcher carries
alerts, which an always-on agent must raise. The research agent spends only through the gateway's
meter, so allowing it where the gateway runs adds no unmetered spend and no live route.

### Alternatives considered

- **A recipient address as a secret:** the configuration names a recipient id, not an address,
  and the transport mails no one. A secret would protect nothing.
- **Allowing the second manager in staging unconditionally:** a research agent without the
  gateway has no metered model route.

<a id="dec-2081"></a>

## DEC-2081: A hosted model gateway starts live, from a registry file the chart renders, in staging alone

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | An agent (Claude Code, lane IAC), under DEC-79, at the coordinator's direction. It is a reversible infrastructure choice, and it only tightens: the budgets are the ones the founder accepted in DEC-2282, and the gateway offers nothing until its gate passes (DEC-2216 item 4) |
| **Story or claim** | E21-62, E21-58 (always-on staging); [DEC-2216](DEC-2216.md) item 4, [DEC-2282](DEC-2282.md), [DEC-2326](trading-core.md#dec-2326), [DEC-2265](intelligence.md#dec-2265), [DEC-2020](infrastructure.md#dec-2020), [DEC-2079](infrastructure.md#dec-2079) item 5 |

### Context

#2841 gave `mandate-gatewayd` its live start: `run --registry <file> --listen <addr> --probe
<socket> --vault-address <url> --vault-dir <dir>`. The live start:
- reads the founder's vetted entries from the registry file;
- logs in to the vault as the model gateway;
- reads `platform/model-provider-key` once;
- serves over HTTPS.

Every entry is `evaluating`, so it offers nothing until a gate passes and answers research
`policy_denied`.

The chart ran only `--recorded`, over a bundle the released image does not carry (DEC-2079 item
5). It also refused any budget outside the local cell, because a hosted cell's budgets were the
founder's to choose. The founder has now chosen them: $50 a month for the workspace and $50 for the
deployment (DEC-2282).

### Decision

1. **`modelGateway.registry`.** This new value holds the live registry file, as strict JSON. The
   chart renders it into the ConfigMap `model-gateway-registry`, and the gateway mounts it
   read-only at `/run/mandate/model-gateway`. It holds no key or secret.
2. **Named, the gateway starts live**, and never with `--recorded`:
   - its arguments are those of #2841's live start, against the cell's vault;
   - `mandate.vaultCopy` copies the `vault-approle-model-gateway` AppRole and the vault's CA into
     its memory-backed vault directory, which it mounts read-only, as the API and the provisioner
     do;
   - egress is unchanged: Postgres, the vault, and staging's `openrouter.ai:443` exact-name policy
     (DEC-2058, DEC-2265).
3. **The chart refuses three renders:**
   - a hosted cell's gateway without a registry, since its image has no recorded bundle;
   - a registry in the local cell, which serves the recorded bundle alone and reaches no provider;
   - a hosted cell's gateway at any tier but `staging`, since production's model budgets are the
     founder's to approve with its spend.

   The old refusal of every budget outside the local cell becomes the tier rule. A hosted gateway
   renders its budgets only where values name them; staging's overlay names DEC-2282's $50 and
   $50 (item 4).
4. **Staging's overlay names the gateway** (the change after this one's chart):
   - its registry is `deploy/cell/model-gateway/registry.json` (#2844) byte for byte, and a test
     holds that;
   - it serves the hosted workspace;
   - its meter stream's deployment is the cell's own `staging`, which never changes, since its
     month of spend lives under that name (DEC-2207);
   - its budgets are DEC-2282's $50 and $50.

5. **The gateway is promoted only after the provider key is in the vault.** The live start stops
   at once without `platform/model-provider-key` (`KeyError::Missing`). Flux waits for every
   Deployment in the workspace release, so a gateway that cannot start would leave the whole
   release unready: the API, the managers and the rest. So `mandate-gatewayd` stays in DEC-2079's
   held list until the founder's staging `apply` prints `model provider key: written`
   (FOUNDER-STEPS K). The change after that apply moves it into `promote`'s map, and the next
   promotion pins it.

### Rationale

- The live start is the only one a released image can run.
- Rendering the file from values keeps the registry a reviewed, committed artifact, read once at
  start and pinned by the overlay's content.
- Restricting the registry to staging keeps production's spend behind the founder's approval.

### Alternatives considered

- **Baking the registry into the image:** a change of entries would need a release, not a reviewed
  overlay change.
- **Allowing a registry in the local cell:** the local cell would reach a paid provider; the
  recorded bundle tests the same paths for nothing.

<a id="dec-2082"></a>

## DEC-2082: Staging's data process stores IEX bars, as bars-reader and its own journal role

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | An agent (Claude Code, lane IAC), under DEC-79, at the coordinator's direction. It is a reversible infrastructure choice that only tightens: paper data only, one host, and no new spend |
| **Story or claim** | E2-20, E21-58 (always-on staging); [DEC-2283](DEC-2283.md) (data-plane spec §3.8), #2842 (`Identity::BarsReader`), #2848 (the alpaca-iex source); amends [DEC-2058](infrastructure.md#dec-2058)'s data row; builds on [DEC-2083](infrastructure.md#dec-2083) |

### Context

Staging had no bars source: the data process could only store the local cell's recording, and the
chart refused that outside dev and CI. DEC-2283 specified the alpaca-iex source. EX built its vault
identity (#2842) and its binary (#2848):
`mandate-datad run --source alpaca-iex --workspace <id> --store <datasets>/alpaca-iex --artifacts
<root> --vault-address <url> --vault-dir <dir> --probe <socket>`.

It reads the hosted workspace's control stream, `ctl:<workspace>`, and appends to no stream. It
logs in to the vault as `bars-reader`, reads `platform/news-data-key` once, and calls
`data.alpaca.markets:443` alone.

### Decision

1. **`data.source`**, either `recorded` (the default) or `alpaca-iex`.
   - `recorded` keeps the local cell's launch, and the data process now holds no journal role,
     since it reads no stream.
   - `alpaca-iex` runs EX's command line for the hosted workspace, with its store at
     `datasets/alpaca-iex` under the artifact store.
   - The runtime's and executor's `MANDATE_BARS` follow the source.
2. **For alpaca-iex, the data process gets three things:**
   - its own journal role, `mandate_data`, a member of a new read-only group,
     `mandate_journal_reader`, never of `mandate_journal_app`. The binary does not itself refuse a
     write, so the role is the guard. The group may connect, use the `journal` schema, and select
     `event_ids`, `events` and `stream_heads`, and nothing else.
     - On a fresh journal, `roles.sql` makes the group, and `processes.json`'s new
       `db_access: read` puts the role in it.
     - On a journal bootstrapped before the group, such as staging's, CloudNativePG's managed
       roles make it.
     - The workspace chart's `migrate.sh` grants the group its connect and select on every
       upgrade, after the migrations, as the tables' owner. It grants nothing while the group
       does not exist, so the Job never fails on it.
       On a journal bootstrapped before the group, the grant can therefore lag one upgrade. That
       upgrade is at the latest the one that promotes the data process (item 4), which comes after
       the journal chart has made the group.
     - The tables are named, so a journal table added by a later migration stays unreadable to
       the group until it is listed.

     The role is generated in every cell, but only an alpaca-iex data pod mounts its credential.
   - the `vault-approle-bars-reader` AppRole and the vault's CA, copied into its vault directory
     by `mandate.vaultCopy`;
   - egress to Postgres, the vault, and its data host. Staging's exact-name policy now names
     `data.alpaca.markets` alone. `stream.data.alpaca.markets` is dropped, since the source never
     calls it.
3. **The chart refuses two renders:**
   - alpaca-iex anywhere but staging, since its key is the platform's paper data key (DEC-2283);
   - alpaca-iex beside a recorded instrument list.
4. **Staging's overlay names `alpaca-iex`, but `promote` still holds `mandate-datad`.** The source
   stops at start without `platform/news-data-key` or its AppRole, and Flux waits for every
   Deployment in the workspace release, so a data process that cannot start would leave the whole
   release unready. It is promoted in the change after the founder's staging `apply` prints
   `news data key: written` (FOUNDER-STEPS M), as the gateway is after its key (DEC-2081 item 5).

### Rationale

The source reads paper data with the platform's key and nothing else, so staging is the only tier
that may name it. Holding the image until the key is in the vault keeps one missing secret from
taking down the whole staging release.

### Alternatives considered

- **The application group, as every other process has:** it may insert into the journal, and
  the data process's binary would be the only thing stopping it.
- **Listing instruments for staging:** the control stream already registers them, and a second
  list could disagree with it.

<a id="dec-2083"></a>

## DEC-2083: Every upgrade grants the journal's application group connect, and every DSN names the journal schema

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | An agent (Claude Code, lane IAC), under DEC-79, at the coordinator's direction (the journal-db lanes are inactive). It is a reversible infrastructure fix. Least privilege is unchanged: the group gets only what roles.sql already gave each of its roles |
| **Story or claim** | E21-58 (always-on staging); journal spec §6.1, infrastructure §3.5 and §7.2; [DEC-2081](infrastructure.md#dec-2081), [DEC-2082](infrastructure.md#dec-2082) |

### Context

The journal chart's `roles.sql` gives each role `CONNECT` on the `mandate` database and the
`journal` schema as its `search_path`. CloudNativePG runs it once, when the cluster is
bootstrapped. Every later process's role is a managed role: a `LOGIN` member of
`mandate_journal_app`, with none of those per-role grants. `roles.sql` also revokes everything on
the database from `PUBLIC`. The group itself is granted `USAGE` on the `journal` schema, so a
member needs only `CONNECT` and the search path.

So a role added after bootstrap cannot connect. Staging's journal was bootstrapped by 10-19, and
these roles were added after it:
- `mandate_connect_finish` and `mandate_role_provisioner`, on 10-23;
- `mandate_dispatcher`, on 10-26;
- `mandate_data` (DEC-2082), next.

Production would meet the same thing at its first role added after bootstrap. Clients name the
journal's tables without the `journal.` prefix.

### Decision

1. **The migration Job grants the group.** On every install and upgrade, before its migrations,
   the workspace chart's `migrate.sh` runs as `mandate_migrator`, the database's owner. It grants
   `CONNECT` on the current database to `mandate_journal_app`. The grant is idempotent.
2. **Every DSN the workspace chart renders sets the search path** in its connection options:
   `options=-c search_path=journal`, through one helper, `mandate.journalDsn`. That covers the
   processes, the API server, connect-finish, and the migrator. This is how the Postgres tests
   already connect, and sqlx and libpq both read it.
3. **Least privilege is unchanged.** The group gets connect, which every role already had. Its table grants stay the migrations'. No role gains another database, `public`,
   or any write the migrations did not give.
4. **Tests:**
   - `deploy/tests/test_journal_roles_pg.py` replays the upgrade on a throwaway PostgreSQL 18:
     bootstrap with `roles.sql`, then a role added the managed way, which cannot connect. After
     `migrate.sh` runs, the role connects and reads, a second run is a no-op, and the role still
     cannot create tables, delete, update, or reach another database. Without the grant, the
     test fails.
   - A chart test holds that every rendered DSN names the schema.

### Rationale

The workspace chart's migration hook already runs as the database's owner on every upgrade, before
any process rolls. Granting there reaches every role added later, with no superuser Job and no
change to CloudNativePG's bootstrap.

### Alternatives considered

- **Re-running roles.sql on each upgrade as the superuser:** a Job holding the superuser's
  credential for a grant the owner can make.
- **Relying on the cluster replacement DEC-2047 brings to staging:** it fixes staging once, not
  production's first new role.

<a id="dec-2084"></a>

## DEC-2084: The news reader is a staging-only workspace process, held until its key and follows

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-28 (UTC) |
| **Decided by** | An agent (Claude Code, lane SC), under DEC-79, at the coordinator's direction, which approved the shape below. It is a reversible infrastructure choice that only tightens: paper data only, one host, a role that may write only its own stream, and no new spend |
| **Story or claim** | E2-10; [DEC-2277](DEC-2277.md) (the paper data key), [DEC-2278](DEC-2278.md) (the reader), [DEC-2280](DEC-2280.md), [DEC-2281](intelligence.md#dec-2281) (the followed agents); builds on [DEC-2082](infrastructure.md#dec-2082) and [DEC-2083](infrastructure.md#dec-2083) |

### Context

The news reader's crate is on main in parts: `mandate-news-reader run --workspace <id> --probe
<socket> --allowlist <file> --artifacts <dir> --user-agent <text> --follow <agent>=<account>...
--vault-address <url> --vault-dir <dir>`. Over Postgres it is the single writer of
`news:{workspace_id}` (journal spec §9.37). It logs in to the vault by AppRole as `news-reader`,
which `cargo xtask vault-provision` already issues as `vault-approle-news-reader`, reads
`platform/news-data-key` once, and calls `data.alpaca.markets:443` alone. The hot store refuses
its role, `mandate_news_reader`, every stream but `news:` (migration 0002).

With no key it never becomes ready, by design (DEC-2280 item 6). With no `--follow` it refuses
`no_follows`. Infrastructure §3.1 lists no news reader, and the chart's tests hold its process list
to that table.

### Decision

1. **A `news-reader` process in infrastructure §3.1 and the chart's `processes.json`**, one per
   workspace deployment, which writes the news stream alone. Its egress is Postgres, the vault, and
   the market-data host, `data.alpaca.markets` on 443, and nothing else.
2. **It renders only with `images.news-reader` and `images.api`, and only in staging.** The API
   server's pod lays out the artifact store, whose claim renders only with it, and without the
   store the reader would refuse `artifact_store_unavailable` and never be ready. Its key is the
   platform's paper data key (DEC-2277), so the chart refuses it at any other `deploymentTier`, as it refuses
   the data process's IEX source (DEC-2082 item 3). Neither the local nor the CI cell runs it: with
   no key it is never ready, and `helm --wait` would wait on it.
3. **Its journal role is `mandate_news_reader`**, a member of `mandate_journal_app`, generated into
   the journal's roles file. Migration 0002 limits it to `news:` streams.
4. **Its vault access is its AppRole:** `vault-approle-news-reader` and the vault's CA, copied
   owner-only into a memory-backed directory by `mandate.vaultCopy`, as the data process's are.
   No ServiceAccount token is mounted.
5. **`news.follows` names the followed agents**, each `{agent, account}`, rendered as one
   `--follow <agent>=<account>` each. The chart refuses an empty list, since the reader would
   refuse `no_follows` and never be ready. They are owner choices, so only the founder's overlay
   names them.
6. **Its allowlist is the repository's `config/sources/allowlist-v3.toml`**, rendered byte for
   byte into a ConfigMap and mounted read-only, so the chart and the crate's tests read one file.
   `news.userAgent` is the contact the vendor sees. It has a default and carries no secret.
7. **The artifact store is read-write for it**, for the typed records it stores before journaling
   them (DEC-2278).
8. **`mandate-news-reader` is released, and promotion holds it.** It joins `release.rs`'s
   `BINARIES` and the promote tests' `RELEASED_NOT_PROMOTED`. It moves to `CHART_IMAGE_FOR_BINARY`
   in the change that gives staging's overlay `news.follows`, after two things:
   - the founder's staging `apply` has written `platform/news-data-key` and
     `vault-approle-news-reader` (FOUNDER-STEPS M);
   - the cell has run the reader against a recorded news fixture (the backlog row E2-10c), so
     the first real run of the chart and the binary together is not staging's.

   Flux waits for every Deployment in the workspace release, so a reader that cannot become ready
   would hold the whole release unready.
9. **Staging's exact-name egress policy opens `data.alpaca.markets` to the news-reader pod.** It
   selects no pod until the image is promoted.

### Rationale

Each choice is the conservative one:
- The reader reads paper data with the platform's key, so only staging may name it.
- Holding the image until its key and follows are set keeps one missing value from taking down
  the staging release.
- Its role and egress are its own, and no wider than its single stream and one host.

### Alternatives considered

- **Running it in the local cell with no key:** it would never be ready, so `helm --wait` would
  fail. Running it there against a recorded news fixture is a backlog row
  ([E2-10-news-reader-in-the-cell](../backlog/E2-10-news-reader-in-the-cell.md)).
- **Kubernetes auth with a ServiceAccount token:** the vault offers it only to a connection's
  executor and token exchange (DEC-2060), and the news reader is neither.
- **Deriving the followed agents from `AgentDeployed`:** the reader reads only snapshot rows of the
  control stream (DEC-2280 item 1), so the deployment names them.
- **A cell Dockerfile now:** the release builds the published image from the binary, and no cell
  runs the reader yet. The backlog row adds the cell's image with the cell's run.

<a id="dec-2085"></a>

## DEC-2085: The news reader runs in the cell on a recording, at dev and ci alone

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-29 (UTC) |
| **Decided by** | An agent (Claude Code, lane SC), under DEC-79, at the coordinator's direction (E2-10c). It is a reversible test-environment choice that only narrows: a recording that reaches no host, a fixture key that names no account, and a start refused outside dev and ci |
| **Story or claim** | E2-10c (#2915); a named precondition of the news reader's promotion ([DEC-2084](infrastructure.md#dec-2084) item 8); narrows [DEC-2084](infrastructure.md#dec-2084) item 2 for the local and CI cell alone |

### Context

DEC-2084 renders the news reader in staging alone. It has no key anywhere else, and with no key it
is never ready. So the first time the chart and the binary run together would be staging's. E2-10c
asks the cell to run the reader first, so that "the agent acts on news" can be proven in CI.

### Decision

1. **`mandate-news-reader run --recorded <file>`** runs the reader over the journal that
   `MANDATE_JOURNAL_DSN` names, with two substitutions:
   - its transport is `recorded::RecordedNews`, read from `<file>`, in place of the HTTPS
     transport;
   - its key is `recorded::recorded_key()`, a fixed paper fixture key, in place of the vault's.

   It reads no vault and reaches no host.
2. **The recording answers every request with one page**, the recorded items whose `symbols` meet
   the request's, whatever its `start`. A request without the key is refused, as the vendor refuses
   it. Since every round is answered again, the reader's own dedupe (journal rule 284) is what keeps
   each item journaled once, so the cell exercises it.
3. **A recorded start is refused outside the `dev` and `ci` tiers**, as `MANDATE_DEPLOYMENT_TIER`
   names them through `mandate_release::startup::Tier`. No tier set reads as production. The
   refusal code is `recorded_outside_dev_and_ci`. An unreadable recording refuses
   `recorded_news_unreadable`, and a malformed one `recorded_news_malformed`.
4. **The chart runs the reader recorded only in the local cell (`localImages`)**, from a recording
   its image carries. The chart change, the image and the cell's drill are the next part of
   E2-10c. Outside the local cell, DEC-2084 item 2 stands unchanged.

### Rationale

The model gateway's `--recorded` start is the precedent: the cell proves a process end to end on a
recording, and the live path waits for its key. Refusing the recorded start by tier, in the binary
as well as the chart, means that no values file can give staging or production a reader that
journals recorded news as if it were real.

### Alternatives considered

- **A recording that honours `start`:** its items' times are fixed while the cell's clock moves,
  so a round would soon ask past every item and store nothing.
- **The in-memory journal of `--fixture`:** the cell's drill must read what the reader journaled,
  and migration 0002's refusal must hold for it, so the reader runs on the cell's Postgres.

<a id="dec-2086"></a>

## DEC-2086: The demo scenario reads step 4's fill and stop from the journal, after web/'s grant

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-29 (UTC) |
| **Decided by** | An agent (Claude Code, lane SC, taking over lane E2E's E2E_FILL slice on claim [#1528](https://github.com/kunwarshivam/mandate/issues/1528)), under DEC-79, at the coordinator's direction. It changes only what the host-side demo scenario reads and prints. It places no order, reads no credential, and weakens no check |
| **Story or claim** | E21-54 (claim #1528); [DEC-2002](infrastructure.md#dec-2002); [DEC-2071](infrastructure.md#dec-2071); [DEC-2378](DEC-2378.md) |

### Context

Step 4 of `cargo xtask cell demo --scenario demo` printed two gaps around web/'s trade loop:

- "the runtime sizes the opening and the gate and autonomy rules ask", waiting on lane D1's
  post-R0 hook (P5_HOOK);
- "the paper fill and the protective stop on a fixture broker", waiting on lane E2E (E2E_FILL).

Both are now stale:

- DEC-2378 replaced the hook with production ports built from the journal, and
  `mandate-runtime-models` wires every paper deployment to them (`production::wired`).
- The executor fills, books and protects a deployed opening on the fixture broker (#2624).

What the scenario lacked was a check of the fill and the stop in the cell.

The research loop of step 3 already reads its links from the journal. But all of its links shared
one `Looped` state. So a research loop that stopped early would also have stopped any read that
came after it.

### Decision

1. **Step 4 reads the granted opening from the journal.** After web/'s trade loop, step 4 polls
   the demo account's stream, `acct:{workspace}:{account}`, for two events in order:
   - `FillApplied` holding `"side":"buy"`;
   - after that fill, `ProtectionChanged` holding `"action":"placed"`.

   Each read is a `LoopCheck`. Until a run of the cell shows it, it stands for its gap, which waits
   on lane E2E for a run in the regular session whose trade loop grants the opening.
2. **Each trade loop has its own chain.** `LoopCheck` names its `Chain`, and each chain keeps its
   own `Looped` state:
   - `Research` is step 3's loop and its CLI grant. It is still read only where `loop_read_here`
     says.
   - `Granted` is step 4's pair. It is read in the job that ran web/'s trade loop, and only once
     that loop reached `approved` (`granted_unread`). Otherwise both reads print `not run` with
     the reason and spend none of their waits.
3. **The ask is no longer a gap.** The step-4 gap for the runtime's ask is removed: the trade
   loop's own approval step shows the ask. The research loop's ask, grant and record wait on lane
   GW's research run (E21-62) instead of P5_HOOK, and so do its fill, stop and exit. P5_HOOK is
   removed.
4. **The dry run names its connection.** A dry run with no connection still names the fill's
   gap, now waiting on DR_CONNECT, since it has no account to read.
5. **Step 4's reads start at the account's head before the trade loop** (added after #2952's review).
   Before the step loop, and so before web/'s trade loop can grant anything, the scenario reads the
   head of each stream step 4 follows (`granted_streams`, `head_query`) and starts its reads after
   it. A buy fill already on the account, from an earlier run on a reused cell or on staging, is
   never read as this run's. A head that cannot be read leaves the stream unanchored: its reads are
   not read from the stream's start, but print why, and fail when the cell is required. Which chain
   a run reads is one pure function, `unread`.
6. **The landed-slice test reads every gap.** `no_wait_names_a_slice_already_on_main` now reads
   the gaps of checks as well as plain gaps, so a check cannot keep a stale wait.

### Rationale

The journal is what Postgres holds, not what a process reports. A fill that web/ shows but the
account stream lacks would be caught here.

Separate chains keep each loop's stop local to it. Gating the step-4 reads on web/'s grant
spends no wait in a job where no opening could have been granted.

### Alternatives considered

- **Keep the gaps until a run shows the fill.** Rejected: the waits named work that has landed
  (DEC-2378, #2624), and the coordinator directed that a landed wait be dropped.
- **Read the fill under the research loop's single state.** Rejected: a stalled research run
  would hide a working fill.
- **Fail when web/ reports `order_filled` but no `FillApplied` is journaled.** Deferred: a read
  that finds nothing already prints as a gap with its wait, and failing it would need the trade
  loop's own result to be trusted over the journal's.
