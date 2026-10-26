# E11-9 follow-up: the owner confirms the displayed bid for an equity exit outside the session

- **E11-9x (Proposed, after the demo; lane WB)** As the owner, I want to confirm the bid my exit's
  screen displays, so that an equity owner exit sells outside the regular session (AGENTS.md rule
  13, mandate spec §6.1, MI-1) instead of waiting for it. Today the positions screen's exit (#2645)
  sends `POST /agents/{id}/exits` with every bid member `null`, since no read serves a quote with
  its `bid_size` and `quoted_at`, so an equity sell outside the session waits for it, as the screen
  says. *Accepted when:* a read serves the instrument's current quote (bid, bid size, quoted at)
  and the floor the exit would use, specified first in its own spec slice; the exit's confirmation
  shows them and sends them all or none (the schema's rule), refusing a stale quote; and the
  trade loop exits outside the session with a confirmed bid.
<!-- section: E11 Web app: dashboard and controls -->
