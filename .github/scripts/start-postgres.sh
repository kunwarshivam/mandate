#!/usr/bin/env bash
# Starts the PostgreSQL that CI's Postgres-backed jobs test against: `full-checks`, which runs the
# journal tests (ADR-0001 ES-08, DEC-109), and every `mutants` shard, so a mutant in code only those
# tests reach is judged by them rather than reported missed while they skip (DEC-519). One script, so
# both jobs run the same digest-pinned image, role, and password that `MANDATE_PG_URL` names.
set -euo pipefail

image="postgres:18.6@sha256:5a5a84b19854a9ffaa54082c166ff4ec27473a361e496e5ea167f298f2da9722"

# A transient registry error would redden a required check DEC-464 forbids retrying automatically,
# as a release download's 500 did before DEC-498 gave every download a retry. The digest still
# decides what runs; the retry only decides how often a transient failure costs a job.
pulled=false
for delay in 2 4 8 16 0; do
  if docker pull --quiet "$image"; then
    pulled=true
    break
  fi
  sleep "$delay"
done
if [ "$pulled" != true ]; then
  echo "could not pull $image" >&2
  exit 1
fi

docker run -d --name postgres -e POSTGRES_PASSWORD=postgres -p 5432:5432 "$image"

# Ready over TCP, which is how the tests connect. The image's first-start initialisation runs a
# temporary server that listens on the Unix socket only, so a socket check can pass while that
# server is about to stop and the real one has not yet started.
for _ in $(seq 1 30); do
  if docker exec postgres pg_isready --host 127.0.0.1 --username postgres; then
    exit 0
  fi
  sleep 2
done
echo "PostgreSQL did not become ready" >&2
exit 1
