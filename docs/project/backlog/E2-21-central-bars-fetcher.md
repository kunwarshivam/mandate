- **E2-21 (Proposed, before customers get bars; EX)** As the operator, I want one platform-side
  fetcher to write every paper workspace's stored IEX bars, so that the platform data key never
  enters a customer cell (DEC-2284, the founder's decision on DEC-2283 item 9.2). Until it lands only
  the founder's workspace has stored bars. *Accepted when:* one fetcher, holding the key in one
  platform namespace, writes each workspace's daily and minute generations into that workspace's
  store as data-plane spec §3.8 gives them. It must keep §3.8's cap, batching, pacing and DP-15
  to DP-17. Its write identity must reach only that workspace's bars root, and a two-workspace test
  must show it carries nothing of one workspace into another (DP-10). No customer namespace may hold
  the `bars-reader` role.
<!-- section: E2 Market data -->
