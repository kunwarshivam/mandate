- **E11-9w (Proposed, M9; W2)** As the founder rehearsing the demo, I want `cell demo` to warm
  `web/`'s `next dev` while the gestures' Job runs, so that a slow first compile never stops the
  trade loop at Playwright's 60 s `webServer` start (lane DR's rehearsal of 2026-10-27: the first
  run on a freshly started machine timed out, and every later run's start fit the bound).
  *Accepted when:* the driver starts `next dev` once and requests the pages the cell's specs open,
  in parallel with the gestures' Job and before the first Playwright run, then stops it; the
  `webServer` bounds in `xtask/src/cell/web.rs` and `web/playwright.cell.config.ts` stay as they
  are (DEC-464); and a test shows the warm-up adds no time to the `cell-demo-web` job's path.
<!-- section: E11 Web app: dashboard and controls -->
