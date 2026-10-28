- **E11-9w (Proposed, M9; W2)** As the founder rehearsing the demo, I want `cell demo` to warm
  `web/`'s `next dev` while the gestures' Job runs, so that a slow first compile never stops the
  trade loop at Playwright's 60 s `webServer` start (lane DR's rehearsal of 2026-10-27: the first
  run on a freshly started machine timed out, and later runs on the same machine did not).
  *Accepted when:* the driver starts `next dev` once and requests the pages the cell's specs open,
  concurrently with the gestures' Job and before the first Playwright run, then stops it; the
  `webServer` timeouts in `web/playwright.cell.config.ts` and the `NEXT_DEV_START_MS` test constant
  in `xtask/src/cell/web.rs` stay as they are, so the `cell-demo-web` job stays under ten minutes
  (`AGENTS.md`, DEC-498); and the scenario report records the warm-up's elapsed time beside the
  Job's.
<!-- section: E11 Web app: dashboard and controls -->
