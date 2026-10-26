# E21-62 debt: a failed temporary write's cleanup has no test

- **E21-62t (Proposed; debt; lane GW)** `FsArtifactStore::write_temp` removes a temporary file whose
  write, permissions or sync failed after it was created (DEC-2279 item 4: only a crashed put
  leaves one behind), but no test forces that failure, so dropping the removal passes every test.
  *Accepted when:* a test injects the failure after creation (a filesystem that refuses the write,
  or a writer seam) and finds no file left in `tmp/`.
<!-- section: E21 Operations and infrastructure -->
