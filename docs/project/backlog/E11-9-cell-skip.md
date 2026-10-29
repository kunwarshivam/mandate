- **E11-9s (Proposed, M9; W)** As the founder, I want the cell to prove an approval's skip from
  `web/`, so that the owner's "skip" is shown working over the real API as approve already is.
  Today the cell's runtime raises one request per run, and the trade loop must approve it to reach
  the fill, the position, the exit and the P&L, so a skip there would empty every later step; the
  coordinator judged a second full cell run (about ten minutes of CI) not worth it while skip uses
  the same responses route approve proves.
  *Accepted when:* the runtime raises a second request in one cell run, the trade loop skips it
  from `/approvals/{id}`, the API answers the skip on `POST /approvals/{id}/responses`, the request
  reads skipped with no order placed for it, and the step is one of `TRADE_LOOP_STEPS`.
<!-- section: E11 Web app: dashboard and controls -->
