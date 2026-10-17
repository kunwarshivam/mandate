#!/usr/bin/env bash
# Starts the S3-compatible test store that `mandate-objectstore`'s conformance suite runs against
# (E21-40, DEC-973): MinIO, digest-pinned, with Object Lock, in `full-checks` and every `mutants`
# shard, so a mutant in code only that suite reaches is judged by it rather than reported missed
# while it skips (the DEC-519 rule, as for PostgreSQL). The root key is a fixed test value for this
# throwaway container, never a real credential; `MANDATE_S3_URL` and the two `MANDATE_S3_*_KEY*`
# names in `ci.yml` carry the same values. MinIO publishes no community images any more, so the
# image is Chainguard's build, test-only (D-5, DEC-969; `docs/dependencies.md`).
set -euo pipefail

digest="sha256:c93745c56fccdaf0e5dcb88999a96a65a136428beb045ef5d546255a80d747a7"
image="cgr.dev/chainguard/minio@$digest"

# A transient registry error would redden a required check (DEC-464), so the pull is retried, as
# start-postgres.sh does. Docker verifies a pull by digest against the content it receives.
pulled=""
for delay in 2 4 8 0; do
  if docker pull --quiet "$image"; then
    pulled="$image"
    break
  fi
  sleep "$delay"
done
if [ -z "$pulled" ]; then
  echo "could not pull $image" >&2
  exit 1
fi

docker run -d --name minio -p 127.0.0.1:9000:9000 \
  -e MINIO_ROOT_USER=mandate-test \
  -e MINIO_ROOT_PASSWORD=mandate-test-only \
  "$pulled" server /data

for _ in $(seq 1 30); do
  if curl -sSf -o /dev/null http://127.0.0.1:9000/minio/health/ready; then
    exit 0
  fi
  sleep 2
done
echo "MinIO did not become ready" >&2
exit 1
