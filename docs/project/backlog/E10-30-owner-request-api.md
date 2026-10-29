# E10-30: an owner's request over the workspace API

- **E10-30 (Must, M8, demo path; lane L1; SC)** As an owner, I want to ask my deployed agent to buy
  or sell its instrument through the workspace API, so that the web app's "Ask an agent" (E11-11,
  [DEC-528](../decisions/DEC-528.md) item 2) reaches the agent's order builder, gate and autonomy
  rules as owner input (DEC-141). *Accepted when:* `POST /agents/{id}/requests` is served as
  workspace API §3.11 item 13 states it ([DEC-2151](../decisions/DEC-2151.md)), by a DEC-77 pair:
  users only, the agent deployed here and not stopped, `paper`, a required positive size, and the
  agent's own pinned instrument, else refused with nothing committed; otherwise one
  `OwnerRequestSubmitted` with `requested_by` `owner`, answered `202` and replayed by key. The
  runtime's copy and the builder's clip of the request are lane X's slice, not this row.
<!-- section: E10 Workspace services -->
