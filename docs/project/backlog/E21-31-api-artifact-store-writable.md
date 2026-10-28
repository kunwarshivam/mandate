- **E21-31-api-artifact-store-writable (Proposed; debt; lanes DR and IAC; after the 2026-10-27 demo)**
  As the owner in the local cell, I want the web's passkey enrolment and version create to record,
  rather than answer `503 journal_unavailable`. The cell chart mounts the API server's artifact store
  read-only: `deploy/cell/charts/workspace/templates/_helpers.tpl`'s `artifactsRole` gives `api` the
  role `seed`, and the mount renders `readOnly: {{ ne . "rw" }}`. The cell README says the same: "the
  API server only reads it". Workspace API spec §3.11 item 2 has the API's routes add
  content-addressed objects to that store, so the chart has drifted from the spec. When a put fails,
  the handler answers `journal_unavailable` with nothing logged, so it looks like a journal outage.
  DR's cell run of #2740 hit it at `POST /passkeys`. `POST /mandate-versions` makes the same put, so
  the web's version create should fail the same way; that has not been observed yet. The CLI path
  works because the control runs mount the store `rw`. Until the fix lands, the demo's set-up goes
  through the CLI.
  **The fix:**
  - Give the API process write access to only the content-addressed tree, `sha256/` and its
    `tmp/`, and never `datasets/` or anything else under the store, following DEC-2279's pattern of
    a store each writer may write.
  - Change `deploy/tests/test_artifacts.py`, which pins the modes today, and the README line in the
    same change.
  - Writes stay add-only by construction: an object is added under its own digest.
  **The spec needs naming too:** §3.11 item 2 says the store is "written only by item 11's routes"
  (versions, confirm, deploy). §3.6's `POST /passkeys` also stores "the public key's COSE bytes as
  an artifact", and item 2 does not name it. The same change, or a spec PR ahead of it under ES-22,
  should add §3.6's enrolment to item 2's writers.
  **Done when** a cell run of `web/e2e-cell/setup.spec.ts` (`cargo xtask cell web-setup <ws>`)
  reaches `passkey_enrolled` and `version_created`.
<!-- section: E21 Operations and infrastructure -->
