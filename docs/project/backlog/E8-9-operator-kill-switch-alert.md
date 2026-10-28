- **E8-9l (Proposed, M7)** As a workspace admin, I want an operator's kill switch alerted once, from
  its `PlatformOperatorAction`, as notifications spec §3.2's `kill_switch` row names its cause, so
  that the operator's switch is one notice to owners and every workspace admin. Until then the
  executor alerts every operator switch it applies, as it did before E8-9k, because removing an
  owner-facing alert with nothing in its place is the founder's call (DEC-2514 item 2). The
  dispatcher de-duplicates only on `owner_command`, so the change that writes the operator's alert
  must also stop the executor's, in the same change.
  *Accepted when:* the operator's switch writes one `OwnerAlertSent` from its action, the executor
  alerts only a risk limit's switch, and a test pins one notice per operator switch.
