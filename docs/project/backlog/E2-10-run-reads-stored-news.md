# E2-10 follow-up: the run's own news tools read the stored news stream

- **E2-10g (Proposed, M5; lane SC; waits on the licence and a spec PR)** As the founder, I want a
  research run's `news.search` and `news.get` to read the items the news reader stored on
  `news:{workspace_id}`, so that the agent reads the news it is triggered by, not only the bundle's
  recorded feed. *Waits on:*
  - **The licence.** Every stored Alpaca item is licensed (allowlist v3: `licensed = true`,
    `counsel_confirmed = false`). The run drops every licensed item (`DropReason::Licensed`;
    `news.get` answers nothing, DEC-2213 item 7), and the reader keeps no licensed text (DEC-2280
    item 4). So the tools would serve nothing until counsel and the founder clear the licence
    (DEC-433 item 20, HI-23) and the raw-response store row lands. Serving licensed items as
    metadata only is the founder's call under HI-23, not an agent's.
  - **A spec PR for `data_ref`.** Journal spec §9.37 (v0.115) says a run's trimmed read journals an
    `ObservationRecorded` naming that read's record. Naming the stored `item_ref` instead, the
    untrimmed record the platform knew, changes that sentence, so its spec PR lands first.

  *Accepted when:* a `NewsReader` port over the news stream and the artifact store answers the
  run's reads in place of the bundle feed where the stream exists. An unlicensed item is served
  with its observation naming `item_ref` verbatim. A licensed item is dropped as today, and no
  licensed text is ever read or logged. An unreadable or mismatched record is skipped and alerted
  as DEC-2288 item 3 says, never served. Tests come first: the unlicensed fixture is served, and
  the licensed fixture never is.
<!-- section: E2 Market data -->
