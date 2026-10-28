- **E2-20b (Proposed, before F1; SC; lane IAC)** As an operator, I want the journal's read-only group
  (`mandate_journal_reader`, DEC-2082) to see only the rows its member needs, so that the data process,
  which reads only `ctl:<workspace>`, cannot read another workspace's or customer's streams once
  customers' data shares a journal (F1). Today the group may select every row of `event_ids`,
  `events` and `stream_heads`. This is acceptable while the cell's journal is the founder
  workspace's alone, and the group can write nothing. The change is to the journal's migrations, a
  safety-critical path: row-level security on the three tables, or a view over `ctl:` rows that the
  group selects instead, with tests first that a member reads its control stream and no other
  stream.
<!-- section: E2 Market data -->
