# E21-29 follow-up: a time source in the local cell, and the scheduler in Compose

- **E21-29a (Proposed, M8; SC; lane P2)** As an operator, I want the local cell's scheduler to
  measure its clock against a time server inside the cell, so that its measurements are in
  tolerance there and the executor's opening gate can be exercised end to end (journal spec §5.4,
  design §3.4). Today the cell's scheduler asks no server (`values.egress.time` is empty), so every
  measurement is journaled `ClockToleranceExceeded`, the reading that adds no risk (`AGENTS.md`
  rule 3). Nothing reads tolerance yet. Once E21-30 holds openings while the last measurement is
  out of tolerance, the cell's trade loop takes no opening until this lands. *Accepted when:* a
  fixture SNTP server runs in the cell, reachable only from the scheduler over UDP 123 (the `time`
  egress target, in-cluster); `cell up` names it in `--servers`; the executor drill shows a
  `ClockOffsetRecorded` with `within_tolerance` `true`; a seeded run that stops the server shows
  `ClockToleranceExceeded`; and no other process can reach it.
- **E21-29b (Proposed, M8; SC; lane P2)** As an operator, I want the scheduler in the Compose
  cell too, so that the Compose runtime gets ticks as the k3d cell's does. *Accepted when:* the
  Compose drill builds the scheduler's image and passes it as `MANDATE_IMAGE_SCHEDULER`, and
  `processes.json` drops the scheduler's `"compose": false`.
