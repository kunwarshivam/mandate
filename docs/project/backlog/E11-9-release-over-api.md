# E11-9 follow-up: stop and release an agent's positions from the web

- **E11-9r (Proposed, after the demo; lane WB)** As the owner, I want "Stop and release positions to
  me" in the web Stop sheet over the workspace API, so that I can end an agent and take its
  positions without selling them, from the web as from the CLI. Today the API sheet offers pause,
  resume, Stop, and the kill switch (#2655), but not release: the release record (D10) must list the
  positions handed over and the protection canceled, and the API serves no per-agent list the sheet
  builds that record from, so `buildApiRecord` refuses a release. *Accepted when:* the record lists
  the agent's own positions from `GET /agents/{id}/positions` (DEC-2111, DEC-2138) and says they
  become unprotected; the Stop sends `release: true` with `warning_shown` the content reference of
  the platform's release warning text (§3.11 item 12, never a digest of the lines shown) and a
  passkey step-up bound to `stop_release` (DEC-2140); and a refusal or unconfirmed outcome is shown
  in words, never as released.
<!-- section: E11 Web app: dashboard and controls -->
