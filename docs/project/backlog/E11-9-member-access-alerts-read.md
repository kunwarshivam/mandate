- **E11-9z (Proposed, M9; EX)** As a member, I want the alerts about my own access (a credential
  added, a new device, a recovery used, an address changed) on my inbox, and as a workspace admin
  the alerts about the workspace's members and access (a role granted, a member deactivated or
  deprovisioned, break-glass, a client connected), so that each reaches the inbox of exactly the
  people notifications spec §3.2 sends it to. `GET /alerts` never serves those nine kinds
  (DEC-2141 item 2). *Accepted when:* a read serves each of them to exactly §3.2's recipients, with
  a test per kind that every other member is served none.
