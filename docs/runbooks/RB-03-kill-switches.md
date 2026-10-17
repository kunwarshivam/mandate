# RB-03: kill switches at every scope, and recovering after one

The alert this answers: `KillSwitchActivated` at agent, connection or workspace scope
([infrastructure design §8.2](../design/infrastructure.md)). It is informational when the owner
pulled it, and SEV-1 when it was automated at workspace scope. This runbook also covers pulling one
when nothing else works.

## What a kill switch does

| Scope | What the stream owners do | Spec |
|---|---|---|
| Agent | The final mode first (`stopped` for an owner kill switch, `paused` for an automated one), then only the agent's own orders cancelled by `client_order_id`, then exactly its sub-ledger quantity sold. Never `cancel-all` or `close-position`, which would reach other agents' and the owner's shares | [trading spec §5.5](../specs/trading-domain.md#55-kill-switch) |
| Connection (the account) | The broker's cancel-all endpoint, `Unknown` orders included, then close-position per instrument | trading spec §5.5 |
| Workspace | The same on every account of the workspace | trading spec §5.5 |

A kill switch is never refused, needs no approval, and does not depend on the API, the model
gateway or any model state (`AGENTS.md` rule 13).

## Pull one: `mandate agent kill`

Needs only the workspace journal: no API, deployment manager, metrics exporter, model gateway or
control plane. The scope is exactly one of `--agent <ID>`, `--connection <ID>` or
`--workspace-wide`. Pausing takes the same flags (`mandate agent pause`).

```bash
mandate agent kill --agent <AGENT> \
  --workspace <WORKSPACE> --user <OWNER> --journal <DSN> --store <DIR>
mandate agent kill --connection <CONNECTION> --workspace <WORKSPACE> --user <OWNER> --journal <DSN> --store <DIR>
mandate agent kill --workspace-wide --workspace <WORKSPACE> --user <OWNER> --journal <DSN> --store <DIR>
```

It prints `kill_switch <scope> <subject> committed as event <id> at seq <n>`. Running it again
commits again; nothing is lost by repeating it.

**In the cell (Kubernetes)**, as a one-off Job under the control role, with nothing but the
journal's Postgres reachable. This is the chart's `control` run, which `cargo xtask cell drill
kill-switch` uses:

```bash
helm template cell deploy/cell/charts/workspace -n ws-<WORKSPACE> -f <values> \
  --show-only templates/run.yaml \
  --set run.id="kill1" \
  --set-json 'run.args=["agent","kill","--workspace-wide","--workspace","<WORKSPACE>","--user","<OWNER>","--journal","$(MANDATE_JOURNAL_DSN)","--store","/work/store"]' \
  | kubectl apply -n ws-<WORKSPACE> -f -
kubectl logs -n ws-<WORKSPACE> job/control-kill1
```

**On one host (Compose)**:

```bash
docker compose -f deploy/cell/compose/compose.json run --rm -v <dir>:/work control \
  agent kill --workspace-wide --workspace <WORKSPACE> --user <OWNER> \
  --journal 'postgres://mandate_control@postgres:5432/mandate?sslmode=disable' --store /work/store
```

**On the single VM** (`deploy/`), over SSH: [deploy/README.md](../../deploy/README.md), *The kill
switch without Cloudflare*.

The CLI's kill switch carries no step-up evidence (DEC-2000 item 4). The stream owners still stop
and flatten. Equities are then sold in the regular session; selling them outside it needs step-up
(mandate spec MI-23). To close one position outside the session, use the web app's owner exit.

## Check it held

1. The control stream has the `OwnerCommandIssued` with `command` `kill_switch` and the scope and
   subject you gave.
2. Every stream owner in the scope recorded `KillSwitchActivated`. Each agent's runtime records it
   on its agent stream, and the executor records it on its account stream. For an agent kill, its
   `cancels` lists only that agent's own orders.
3. Positions in the scope go to flat as the session allows. The `deferred` list names what waits
   for the regular session.

Neither `KillSwitchActivated` within a minute, with the journal reachable: the runtime or executor
is down. Treat it as RB-07 or RB-14. The command is already journaled, and the stream owner applies
it first when it starts.

## Recovering after one

- The agent stays in its final mode. After an owner kill switch it is `stopped`, and it trades again
  only once redeployed. An automated kill switch (`flatten_and_pause`, the daily-loss flatten, the
  lifetime floor) leaves it `paused` until the owner acknowledges it once flat (mandate spec §5.8)
  and resumes it with the code (mandate spec §6.1).
- Before resuming, find why it was pulled. For an automated workspace kill, that is a SEV-1
  postmortem ([quality and release](../project/07-quality-and-release.md)).
- A connection or workspace kill used the account-wide endpoints. Reconcile each account before any
  agent resumes (RB-01).

## Not yet

- Today `mandate-executor` applies only the agent scope. The connection and workspace scopes are
  committed and journaled, and their executor slice is E7-4's next one (DEC-485 item 1).
- The local cell does not run the runtime and executor yet. Its drill shows the command journaled
  with the API, deployment manager, exporter, model gateway and control plane all down, but not yet
  each stream owner's `KillSwitchActivated` (DEC-2000 item 6).
