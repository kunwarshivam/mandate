- **E8-9k (Proposed, M7; EX or L1)** As the owner, I want my own kill switch recorded as one
  `OwnerAlertSent` (`kill_switch`, `owner_command` the command) in the `OwnerCommandIssued`'s batch
  on the control stream, so that the dispatcher gives every user kill switch exactly one notice
  (notifications spec §3.2, §3.4). Until then the executor alerts every switch it applies, with
  `owner_command` null (DEC-2514 item 2), and the workspace services write none. The dispatcher
  de-duplicates only on `owner_command`, so this change must, in the same change, stop the
  executor alerting a user's and an operator's switch, or a user's switch gets two notices.
  *Accepted when:* the API's kill switch batch holds the alert, the journal accepts it, the
  executor alerts only an automated switch, and a test pins one notice per command.
