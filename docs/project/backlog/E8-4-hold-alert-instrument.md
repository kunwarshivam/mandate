# E8-4 follow-up: show the held instrument behind a hold alert

- **E8-4h (Should)** As an owner, I want an `attribution_deficit` or `protection_unattributed`
  alert to take me to the instrument whose openings are held, so that I know what to buy back,
  release or cancel. DEC-2855 makes the alert's subject the `GateDecided` that held the opening,
  and the push names nothing (`AGENTS.md` rule 6), but the alerts read carries no instrument and
  the web's `subjectHref` has no case for a `GateDecided` subject. *Accepted when:* the alert's
  screen shows the held instrument from the subject, read inside the workspace app only, with a
  test that no push or notice payload gains it.
<!-- section: E8 Escalation and approvals -->
