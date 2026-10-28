- **E2-23 (Proposed, M8; lanes IAC and journal)** As the operator, I want the data process's
  database role to read only its workspace's control stream, so that a process that needs
  `ctl:<ws>` alone cannot read any other stream's orders, positions or mandate content. Today the
  read-only group `mandate_journal_reader` (lane IAC, for `mandate-datad`'s IEX source, DEC-2283)
  holds `SELECT` on `event_ids`, `events` and `stream_heads`, every row: it cannot write, but it
  can read every stream in the journal. That is accepted while each journal holds one owner's
  data, as the founder workspace's cell does; it is not once a journal holds more than one
  owner's (F1, customers on the central fetcher). *Accepted when:* the role reads only rows of
  `ctl:` streams it is granted, by row-level security in the journal's migrations or a view
  limited to them, with the journal's Postgres tests showing another stream's rows refused or
  absent and `ctl:<ws>`'s read in order with its hashes; and this lands before any journal holds
  more than one owner's data.
<!-- section: E2 Market data -->
