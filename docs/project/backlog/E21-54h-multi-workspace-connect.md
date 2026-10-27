- **E21-54h (Proposed, M8; SC; lane IAC)** As an owner, I want to connect a broker paper account to
  a second workspace in a hosted cell, such as the research workspace `<W>r` (DEC-2070), so that a
  hosted cell can run the research agent in its own workspace as the local cell does. Today:
  - the API server serves one workspace, `MANDATE_API_WORKSPACE`: its memberships, its OAuth
    connect start and its re-check name the hosted workspace alone;
  - connect-finish reads only `ctl:<api.workspace>`;
  - the role provisioner watches only that workspace's control stream, so a second workspace's
    connection would get no vault role for its executor.

  The change is to the safety-critical `mandate-api-server`, connect-finish and
  `mandate-provisionerd` (authentication, tenant isolation, the vault), with tests first, beside
  E21-54g's multi-workspace manager. Until it lands, staging's research agent runs in the hosted
  workspace (DEC-2081).
<!-- section: E21 Operations and infrastructure -->
