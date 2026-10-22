# RB-19: route 2's failed-assertion limit reached

The alert this answers comes from the workspace API server ([infrastructure design
§8.2](../design/infrastructure.md)). It is an opaque operator alert: the code
`reduction_failures_over_limit` on the server's standard error, and this runbook, and nothing else.
It names no address, device, member, passkey, or workspace content (`AGENTS.md` rule 6), so the
alert alone tells you that someone failed many times, never who.

| Code | Severity | What it means |
|---|---|---|
| `reduction_failures_over_limit` | SEV-3 | One client of route 2's opening route (`POST /v1/reduction-sessions`, [workspace API spec](../specs/workspace-api.md) §4.10 route 2 item 3) reached its failure limit: 10 failed presentations from one device, or 100 from one address, within 300 s ([DEC-653](../project/decisions/DEC-653.md) item 3, [DEC-2119](../project/decisions/DEC-2119.md)) |

## What it does and does not do

- It **only reports**. No answer of either route 2 route depends on it: every refusal was already
  the one 401, and an assertion that verifies is never refused for a limit (DEC-834). Nothing is
  blocked, slowed, or locked out because it fired.
- It is raised **once** per client per process when that client first reaches its limit, and
  again only after that client's failures lapse below the limit and it reaches it again. A flood
  therefore writes one line, not one per request.
- The count lives in the server's memory and nowhere else. A restart forgets it, and the alert is
  never journaled.
- An address is the TCP peer, or, behind the ingress, the `X-Forwarded-For` entry the server trusts
  (`MANDATE_API_INGRESS_PEERS`); a native IPv6 address counts as its /64. A device is the
  `__Host-mandate_device` cookie. A client without a device cookie shares one device budget with
  every other such client.

## What to check

1. **Is sign-in down?** Route 2 exists for when the identity provider is unreachable. A burst of
   member retries during a provider outage, with a stale passkey or a mistyped one, can reach the
   device limit. Check the issuer's status and route 1's refresh outcomes. If sign-in is down,
   expect alerts from real members and do nothing on route 2.
2. **How many, from how wide?** Count the alert lines on the API server's standard error over the
   last hour. One or two is a member struggling; a steady stream is a flood across many devices or
   addresses. The alert never says which, by design.
3. **Is the ingress forwarding what the server expects?** If `MANDATE_API_INGRESS_PEERS` is unset
   behind an ingress, every client shares one address budget, and the address limit fires for
   everyone at once. Compare the deployment's values (`ingressPeers` in the workspace chart) with
   the ingress's actual peer addresses.
4. **Is the challenge route saturated?** A flood that also holds the challenge slots makes members
   get the one 401 when they ask for a challenge (§4.10 item 3, the residual). Members then use
   another network, the host CLI (route 3) where there is one, or route 1 once the provider
   answers.

## What to do

- For a flood, rate-limit or block at the ingress or the network edge, by the source the ingress
  sees. The application keeps no per-client data to block by, and nothing here should add one.
- Do not restart the API server to clear the alert: a restart also ends every browser session and
  every route-2 session, and forgets every sign-in, so each member must sign in again before route 2
  serves them ([DEC-2319](../project/decisions/DEC-2319.md) item 2).
- If a member reports being unable to pause or kill switch, point them to the host CLI (route 3) or
  route 1, and check steps 1 and 4.
