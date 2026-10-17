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

## The run against a real provider (R2 on the day H3 is bought; B2 if DEC-974 is taken)

The founder runs this once, before anything is stored at the provider (the plan's local-cell gate,
DEC-969). It writes one small probe object under the prefix, locked for six years if the store
locks it. Keys are read from files and never printed; the output is the verdict and refusal codes.

1. Create the bucket. For R2: in the dashboard, location hint eastern North America, and add the
   strongest bucket lock rule R2 offers (no prefix, indefinite), so the run judges R2 at its best.
   For B2: Object Lock enabled at creation, and a default rule in compliance mode of at least six
   years.
2. Make the writer's key: for R2, an R2 API token with Object Read & Write on that bucket only;
   for B2, an application key for that bucket without `bypassGovernance` and without
   `writeBucketRetentions`. Save its access key ID and secret into two files, mode 0600.
3. For R2 only, a Cloudflare API token that can read the bucket's configuration, saved to a third
   file.
4. Run:

```bash
export MANDATE_OBJECTSTORE_URL=https://<account_id>.r2.cloudflarestorage.com   # B2: https://s3.<region>.backblazeb2.com
export MANDATE_OBJECTSTORE_REGION=auto                                          # B2: the bucket's region
export MANDATE_OBJECTSTORE_BUCKET=<bucket> MANDATE_OBJECTSTORE_PREFIX=cold/
export MANDATE_OBJECTSTORE_KEY_ID_FILE=~/keys/id MANDATE_OBJECTSTORE_SECRET_FILE=~/keys/secret
export MANDATE_R2_ACCOUNT_ID=<account_id> MANDATE_R2_TOKEN_FILE=~/keys/cf-token          # R2 only
cargo run -p mandate-objectstore --example lockcheck
```

Expected for R2, per DEC-973 item 5: the S3 check refused (R2's S3 API has no Object Lock, so at
least `objectstore_no_lock`), and the rules refused with `objectstore_governance_only`. Record the
output in the E21-40 claim issue. An R2 result that is accepted, or differs from the expectation,
reopens DEC-973 rather than being acted on. Expected for B2 under DEC-974: `accepted`.
