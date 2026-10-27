- **E2-20 (Proposed; founder; compliance)** As the operator, I want each workspace's stored bars
  fetched under a recorded data license or with the workspace's own data key, so that serving IEX
  bars is not redistribution under Alpaca's market-data terms. DEC-2283 item 9: the founder
  accepted that risk on 2026-10-27 so customers can use the platform key's bars (data-plane spec
  §3.8). *Accepted when:* either a redistribution license is recorded beside the source
  (DP-11's configuration test then passes for it), or each workspace's data process fetches with a
  key from that workspace's own Alpaca account and the platform key serves only the founder's
  workspace, and data-plane spec item 3's exception is removed.
<!-- section: E2 Market data -->
