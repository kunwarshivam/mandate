- **E11-pf2 (Proposed; W)** As the founder, I want `GET /dashboard/history` (DEC-2413, #3028) to
  read only the snapshots its range needs, so that the `1y` and `all` ranges stay fast as an
  account's journal grows. Today the read folds every `AccountSnapshotRecorded` on the account's
  stream and buckets them in memory, and reconciliation writes one about every 10 seconds.
  *Accepted when:* the history read asks the journal for the range's window only (or a kept
  per-bucket summary), a test with a year of snapshots answers every range within the read's
  budget, and the served points are byte-identical to today's for the same journal.
<!-- section: E11 Web app: dashboard and controls -->
