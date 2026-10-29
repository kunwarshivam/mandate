- **E9-4 (debt, Proposed, M8; SC) Remove staging's passkey cool-off waiver after the demo.**
  [DEC-2150](../decisions/DEC-2150.md) lets a staging cell waive a first passkey's 24-hour cool-off
  for the founder's demo, until an expiry no later than the compiled ceiling 2026-11-05T00:00:00Z.
  The ceiling and the expiry end it on their own. But the code, the chart and staging's values keep
  the staging tier, and that path should not outlive the demo. *Accepted when:* staging's values no
  longer set `api.stepUpCoolOffWaiver` or `api.stepUpCoolOffWaiverUntil`; the API server refuses the
  waiver in `staging` at start again, and drops `STAGING_WAIVER_CEILING` and
  `MANDATE_API_STEP_UP_COOL_OFF_WAIVER_UNTIL`; and the chart's schema and render refuse it in
  `staging` again; and `web/` drops [DEC-2412](../decisions/DEC-2412.md)'s
  `NEXT_PUBLIC_OWLHEAD_STAGING_COOL_OFF_UNTIL` (its line in `web-deploy.yml`, `STAGING_WAIVER_CEILING` in
  `src/lib/cool-off-build.ts`, and its names in xtask's workflow env lists). A decision records the removal, superseding DEC-2150 item 1, and journal spec
  §9.38 keeps `staging` as a valid `tier` for the waivers already journaled.
<!-- section: E9 Identity, tenancy, and policy -->
