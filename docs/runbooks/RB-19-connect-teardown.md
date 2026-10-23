# RB-19: a connect's teardown has stalled

The alert this answers comes from the workspace API's connect finish ([DEC-2348](../project/decisions/DEC-2348.md)).
It is an opaque operator alert: a reason code and this runbook, and nothing else (no connection,
account, workspace or user).

| Code | Severity | What it means |
|---|---|---|
| `connect_teardown_stalled` | SEV-2 | A broker connect reached its deadline, and the API still cannot delete its authorization code and token from the vault. Either the refusal that closes it is being held until the delete succeeds ([DEC-2342](../project/decisions/DEC-2342.md) item 3), or the connect is already closed and its delete is still owed |

It is raised once per request per API process. A restarted API reads the control stream again and
raises it again while the delete still fails.

## What holds while it is raised

- The API retries the delete every five seconds, and appends the held refusal as soon as the first
  delete succeeds, then deletes again.
- A connect held this way stays open on the control stream. Nothing can establish it past its
  deadline, and no executor uses its code.
- The pass appends nothing else for that connect. Other connects are finished as usual.

## What to do

1. Check the vault: is it reachable from the API, unsealed, and serving the API's identity? This
   alert is usually a vault outage (RB-12).
2. Check the API's vault login: the mounted `api.role-id` and `api.secret-id`, and the policy that
   lets the API delete codes and tokens.
3. Once the vault answers, wait one pass. The held refusal is journaled, and the alert is not raised
   again for that connect.
4. If the vault answers and the alert persists after a restart of the API, open an incident: the
   delete itself is failing.

Do not delete the entries by hand, and do not append a refusal by hand. The pass deletes before it
journals (journal spec §9.8), and a hand-written close would skip that order.
