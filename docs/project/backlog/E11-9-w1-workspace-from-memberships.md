- **E11-9 W-1 (Should, M9)** As the owner, I want the web app to read my workspace from my own
  memberships when its deployment names none, so that one build serves any single-workspace
  deployment without a per-deployment variable (DEC-975, DEC-2113). *Accepted when:* with
  `OWLHEAD_WORKSPACE_ID` unset, the browser opens its session, reads `GET /v1/me/workspaces`, and
  reads the one workspace named; none or several, a 401, or an unreadable answer shows that state,
  reads nothing, and records no command; a malformed value is still misconfigured.
<!-- section: E11 Web app: dashboard and controls -->
