- **E8-9k (Proposed, M7; EX or L1)** As the owner, I want my own kill switch recorded as one
  `OwnerAlertSent` (`kill_switch`, `owner_command` the command) in the `OwnerCommandIssued`'s batch
  on the control stream, so that the dispatcher gives every user kill switch exactly one notice
  (notifications spec §3.2, §3.4). The executor raises `kill_switch` only for an automated switch
  (DEC-2514), and the workspace services write none yet. *Accepted when:* the API's kill switch
  batch holds the alert, the journal accepts it, and a test pins one alert per command.
