# mandate-objectstore

The object store behind a plain S3 endpoint, and the lock check that refuses any store weaker than
[journal spec §6.2](../../docs/specs/journal.md)'s write-once retention (backlog E21-40,
[DEC-973](../../docs/project/decisions/DEC-973.md)). Cloudflare R2's locks are weaker than §6.2
(DEC-973 item 5); where the §6.2 objects go instead is [DEC-974](../../docs/project/decisions/DEC-974.md),
Proposed.

## The conformance suite

`tests/conformance.rs` runs the check and the locked writer against a real S3-compatible store.
Without `MANDATE_S3_URL` each test says it is skipping and passes; with `MANDATE_S3_REQUIRED` set as
well (CI's `full` job and every mutants shard), a missing store fails instead. CI starts the store
with `.github/scripts/start-minio.sh`: MinIO, digest-pinned, with a fixed test-only root key. The
suite narrows that key per test with an STS `AssumeRole` session policy, so the writer it checks
cannot write the bucket's lock rule.

```bash
.github/scripts/start-minio.sh     # needs Docker
export MANDATE_S3_URL=http://127.0.0.1:9000
export MANDATE_S3_ACCESS_KEY_ID=mandate-test MANDATE_S3_SECRET_ACCESS_KEY=mandate-test-only
cargo nextest run -p mandate-objectstore
cargo xtask ci objectstore         # the same run CI's `full` job makes
```

## The run against Cloudflare R2

`.github/workflows/r2-lockcheck.yml` tests DEC-973 item 5's finding against R2 itself. It runs on
demand from `main` only, in the `production` environment, which is restricted to `main` and holds
`R2_SECRET_ACCESS_KEY` and `CLOUDFLARE_API_TOKEN` as secrets and `R2_ENDPOINT`, `R2_ACCESS_KEY_ID`
and `CLOUDFLARE_ACCOUNT_ID` as variables; no pull-request job sees them (ES-12). Start it from the
Actions tab (`r2-lockcheck`, Run workflow, branch `main`).

`examples/r2_finding.rs` never touches the cell's buckets. It creates a scratch bucket
(`mandate-lockcheck-<time>`, location hint ENAM), runs the lock check against R2's S3 API, sets R2's
strongest bucket lock rule (indefinite, the whole bucket), reads it back through Cloudflare's API,
writes an object and tries to delete it while the rule holds, removes the rule with the
bucket-configuration token, deletes the object again, and then deletes the scratch bucket whatever
happened. It prints outcomes and refusal codes only. It exits 0 when R2 behaves as DEC-973 found:
every check refused, the delete refused under the rule and done once the token removed it. Any
other result exits 1 and reopens DEC-973 rather than being acted on. Record the run's link in the
E21-40 claim issue.

## The run against another provider (B2, if DEC-974 is taken)

`examples/lockcheck.rs` runs the check against any S3-compatible bucket with a writer key read from
files, and prints the verdict. It writes one small probe object under the prefix, locked for six
years if the store locks it.

1. Create the bucket with Object Lock enabled at creation and a default rule in compliance mode of
   at least six years.
2. Make the writer's key for that bucket only, without `bypassGovernance` and without
   `writeBucketRetentions`; save its key ID and secret into two files, mode 0600.
3. Run:

```bash
export MANDATE_OBJECTSTORE_URL=https://s3.<region>.backblazeb2.com
export MANDATE_OBJECTSTORE_REGION=<region> MANDATE_OBJECTSTORE_BUCKET=<bucket>
export MANDATE_OBJECTSTORE_PREFIX=cold/
export MANDATE_OBJECTSTORE_KEY_ID_FILE=~/keys/id MANDATE_OBJECTSTORE_SECRET_FILE=~/keys/secret
cargo run -p mandate-objectstore --example lockcheck
```

Expected: `accepted`. With `MANDATE_R2_ACCOUNT_ID` and `MANDATE_R2_TOKEN_FILE` set as well, it also
reads an R2 bucket's lock rules and prints their verdict.
