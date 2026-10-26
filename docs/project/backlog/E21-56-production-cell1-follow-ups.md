- **E21-56 production cell 1 follow-ups (Should, after the founder approves DEC-2064's spend; IAC,
  GO, W)** Once the founder approves a monthly amount for customer production
  ([DEC-2064](decisions/DEC-2064.md), [design](../design/production-cell.md) §9):
  - IAC adds `production-cell1` to `infra.yml`, `cargo xtask infra`, and their preflight, with
    the runbook's `infra-production-cell1` rows; it sets `approved_monthly_usd`, retires the small
    `production` root, and moves the `cloudflare` module's archive onto `modules/archive`.
  - GO adds the cell's GitOps overlay, with one HelmRelease per workspace, and cloudflared as a
    Deployment, and workspace pods' `nodeSelector` on `mandate.dev/pool`. It adds the three-instance journal with a database per workspace, OpenBao HA and
    its snapshot job, and the monitoring stack, plus `promote production-cell1`.
  - W shows each workspace's API host in the web app.

  *Accepted when:* a plan of `production-cell1` under the approved amount lists exactly design §3's
  resources, and staging's drills pass on the overlay before the first customer.
<!-- section: E21 Operations and infrastructure -->
