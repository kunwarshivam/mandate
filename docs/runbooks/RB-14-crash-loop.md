# RB-14: crash loop, and deployments the manager cannot run

The alerts this answers come from the deployment manager ([infrastructure design
§8.2](../design/infrastructure.md)). Each is an opaque operator alert: a reason code and this
runbook, and nothing else (no agent, account, instrument or mandate content).

| Code | Severity | What it means |
|---|---|---|
| `crash_loop` | SEV-2 | A runtime or executor restarted five times within ten minutes (design §3.4, Proposed bound). The manager journaled `ProcessCrashLooped` and left it down |
| `kill_switch_unapplied` | SEV-1 | An executor no agent of its account wants any more was started for a kill switch that covers it, and crash-looped before it ended: the switch is not applied ([DEC-2344](../project/decisions/DEC-2344.md) item 1). Raised in place of `crash_loop` |
| `deployment_unread` | SEV-3 | A version-1 `AgentDeployed` of an agent not stopped. It names no connection, so the manager starts nothing for it ([DEC-1173](../project/decisions/DEC-1173.md) item 3) |
| `deployment_unconnected` | SEV-3 | A version-2 `AgentDeployed` that journal spec §9.24 rule 201 refuses, of an agent not stopped: its connection was not established on the control stream before it, or was revoked since. The manager starts nothing for it ([DEC-2038](../project/decisions/DEC-2038.md)) |

Each is raised once per manager process: a restarted manager reads the control stream again and
raises it again while the condition holds. None of them stops, drains or replaces a running
process, and none holds back a kill switch (design §3.4, §3.6).

## `kill_switch_unapplied`: a kill switch that did not reach its account

The account's agents are all retired, so nothing else runs on it, but a kill switch still covers
what the owner holds there (an account- or workspace-scoped switch reaches released holdings). The
manager started the account's executor for that switch, and it crashed until its crash loop. Treat
it as a kill switch not yet applied.

- Escalate at once, as for a kill switch not applied. Today only the operator is told: the owner's
  alert for this condition, with generic text and no ids, lands with E8's notification row
  ([DEC-2344](../project/decisions/DEC-2344.md) items 1 and 2) and is needed before staging. The
  operator never trades on an owner's account; the owner can act in the broker's own console, or
  issue the switch again once the executor can run.
- Find why the executor could not run: its pod's logs and last exit, as below. A revoked connection
  whose token is destroyed cannot log in to the broker; that executor cannot apply anything, so the
  manual step above is the only one.
- A later covering kill switch still gets its one start; a restart of the manager raises this alert
  again, once, while the condition holds.

## `crash_loop`: a process left down

What holds while it is down:

- The manager starts the process again only for a kill switch whose scope covers it, once per
  kill switch, so a kill switch still reaches it (journal spec §9.21, "Which fact is the pause").
- A restart of the manager does not bring it back.
- An executor stays down while any agent of its account is still deployed.

What ends it, at this version:

- **A runtime:** the agent's `AgentStopped`. A stop is final, so the agent runs again only as a new
  agent id, deployed on a confirmed version (`mandate agent deploy`).
- **An executor:** every agent of its account stopped.

**A known gap.** No process on `main` journals `AgentStopped` yet. The owner's `mandate agent stop`
is an owner command that the agent's runtime carries out, and a crash-looped runtime is down. So
today a crash-looped agent stays down. Deploy its replacement as a new agent id. The owner's resume
of a crash-looped agent waits on the runtime copying the pause (E21-30).

To find out why it crashed, read the pod's logs and its last exit
(`kubectl logs <pod> --previous`, `kubectl describe pod <pod>`) in the workspace's namespace. Fix
the cause before deploying again; a new deployment of the same broken build loops again.

## `deployment_unread`: a version-1 deployment

The deployment was written before the manager read version 2, or by a writer that wrote version 1
because rule 201 did not hold when it deployed. It will never run: version 1 names no connection,
and the manager never guesses one.

1. Make sure the connection is recorded. `mandate connection record` writes the version-2
   `ConnectionEstablished` that rule 201 needs, once the connect sequence has checked the account
   (its `ConnectionChecked`).
2. Deploy a new agent id on the confirmed version. The CLI and the workspace API write version 2
   exactly when rule 201 holds.

The old agent id is never run. Its alert ends only with its `AgentStopped`, which nothing journals
yet for an agent with no runtime (see the gap above), so each manager start raises it again. That
is expected, and it names no agent.

## `deployment_unconnected`: a version-2 deployment rule 201 refuses

The deployment names a connection the control stream had not established, or had revoked, when it
was written. A later establishment does not revive it: rule 201 judges a deployment where it
stands on the stream.

1. Find out whether the connection was revoked on purpose (RB-12). If it was, deploy nothing.
2. Otherwise record the connection again (`mandate connection record`, as above), and deploy a new
   agent id on the confirmed version.

As for `deployment_unread`, the old agent id is never run, and its alert repeats on each manager
start until an `AgentStopped` names it.

`mandate journal verify` on the control stream reports the refused record as
`connection_lifecycle_mismatch` (journal spec §11), which confirms the reading.
