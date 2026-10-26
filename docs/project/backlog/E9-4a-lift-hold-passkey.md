- **E9-4a (Proposed, M8; SC)** As an owner, I want to lift a hold on new openings from the web
  app under passkey step-up, as I resume or stop an agent, so that no owner command the web app
  serves is recorded without the evidence the runtime judges. The API serves no lift-hold route
  yet (`LiftHoldRequest` exists in `mandate-api` and its schema, nothing routes it), so it lacks
  step-up only because it is not served. *Accepted when:* the route is served and, beside
  `passkey` evidence, takes an optional `step_up_assertion` and commits its `StepUpAsserted` after
  the command in one batch, as [DEC-2140](../decisions/DEC-2140.md) does for a resume and a Stop.
<!-- section: E9 Identity, tenancy, and policy -->
