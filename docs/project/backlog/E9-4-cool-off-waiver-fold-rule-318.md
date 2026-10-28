- **E9-4 (debt, Proposed, M8; SC) The credential fold judges rule 318, failing closed.** Journal
  spec §9.38 makes stream rule 318 the writer's and has the verifier's fold fail closed on it. The
  API's writer keeps it by construction ([DEC-2146](../decisions/DEC-2146.md) item 4): it waives
  only the principal's own enrolled credential whose `cool_off_ends` is after the instant it judged
  at, which is the records' `event_time`, and a challenge counts once. But
  `mandate_passkey::credentials::CredentialFold` still skips `StepUpCoolOffWaived` and carries no
  `event_time` on an assertion, so a record another writer appended against rule 318 does not yet
  break step-up for the member and the credential's holder. *Accepted when:* `credential_record`
  maps `StepUpCoolOffWaived` and the assertion's `event_time`. Then, as for rule 254, step-up's view
  discounts the member and the holder from the first record that breaks a clause of rule 318 or its
  converse: a `StepUpAsserted` inside its cool-off not immediately followed by its waiver. Route
  2's `held` view never refuses. The fold is tested first against `cool_off_waivers.streams` (DEC-77).
<!-- section: E9 Identity, tenancy, and policy -->
