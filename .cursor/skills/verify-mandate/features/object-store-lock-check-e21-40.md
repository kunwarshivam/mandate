# Object store lock check (E21-40)

- **Spec:** `docs/specs/journal.md` §6.2; DEC-973 (the check, and the finding that Cloudflare R2's
  locks are weaker than §6.2), DEC-974 (Proposed: the §6.2 objects on Backblaze B2);
  the managed-cells plan, lane S1 (PR #1431).
- **Code:** `mandate-objectstore`: `crates/mandate-objectstore/src/lock.rs` (the pure judgement of
  S3 Object Lock evidence and of R2's bucket lock rules, every refusal named),
  `crates/mandate-objectstore/src/sigv4.rs` (SigV4 header signing),
  `crates/mandate-objectstore/src/s3.rs` (the plain S3 endpoint: HTTPS or loopback HTTP, no
  redirects, a 4xx or 501 a rejection and anything else an error),
  `crates/mandate-objectstore/src/check.rs` (the start-up probe with the writer's own key, the
  `LockCertificate` only a passed check issues, and the `LockedWriter` only a certificate builds),
  `crates/mandate-objectstore/src/r2.rs` (R2's rules through Cloudflare's API), and
  `crates/mandate-objectstore/examples/lockcheck.rs` (the run against any provider), and
  `crates/mandate-objectstore/examples/r2_finding.rs` with `.github/workflows/r2-lockcheck.yml`
  (the run against R2 on a scratch bucket, from `main` in the `production` environment).
- **Tests:** `crates/mandate-objectstore/tests/lock.rs` (one test per refusal, the six-year
  boundaries, R2's rules, and a property test whose oracle derives the expected refusals from the
  weakenings it applied), `crates/mandate-objectstore/tests/sigv4.rs` (Amazon's four worked S3
  examples), `crates/mandate-objectstore/tests/conformance.rs` (against the digest-pinned S3 test
  store: accepted only with a writer that cannot rewrite the rule, each weaker bucket refused with
  its own reason, the probe reused, a certificate bound to its store),
  `crates/mandate-objectstore/tests/r2.rs` (a loopback fake of Cloudflare's API), and
  `crates/mandate-objectstore/tests/endpoint.rs` (configuration, and an unreachable store failing
  closed).
- **Run:** `.github/scripts/start-minio.sh`, then `MANDATE_S3_URL=http://127.0.0.1:9000
  MANDATE_S3_ACCESS_KEY_ID=mandate-test MANDATE_S3_SECRET_ACCESS_KEY=mandate-test-only cargo xtask
  ci objectstore`; `crates/mandate-objectstore/README.md` for the run against R2 or B2.
