- **E21-30 X1b-3 (Must, M8, before the founder's demo; SC)** As the founder, I want a paper opening
  the runtime proposes in the cell to be placed by the executor, filled on the fixture broker, its
  protective stop placed and the fill booked by reconciliation, so that the end-to-end trade loop
  runs in the local cell with only the broker a fixture ([DEC-2380](../decisions/DEC-2380.md)).
  *Accepted when:* `MarkUpdated` version 2 is registered and the executor journals it; the
  executor's mandate view, instrument snapshot (with fill references), binding-gate source,
  protections and quotes are read from the journal, the deployment's mandate document and
  `mandate-gate-context` for the fixture and the paper broker; the fixture broker fills a placed
  limit or bracket at its limit and keeps its stop leg resting, with activities, positions and
  account that reconciliation books; and a cell drill holds on `FillApplied` and the protective
  stop, plain and seeded.
