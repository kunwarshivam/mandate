#!/usr/bin/env bash
# Starts the OpenBao dev server that CI's `full-checks` job and every `mutants` shard run
# `mandate-vault`'s conformance suite against (E21-32, DEC-977 item 6): the pinned image (D-5,
# DEC-969), in dev mode with TLS, holding fake secrets only. It writes the server's CA certificate
# and its fake root token to the files MANDATE_OPENBAO_CA and MANDATE_OPENBAO_OPERATOR_TOKEN_FILE
# name, which the job's env sets beside MANDATE_OPENBAO_ADDR; the token itself is never in the
# environment.
set -euo pipefail

digest="sha256:6d2b93856e3fcf7b18ad855a0b51eaba474dc8b79cf554379ea32034797d2acf"
image="openbao/openbao:2.7.1@$digest"
ca="${MANDATE_OPENBAO_CA:?the job sets MANDATE_OPENBAO_CA}"
token_file="${MANDATE_OPENBAO_OPERATOR_TOKEN_FILE:?the job sets MANDATE_OPENBAO_OPERATOR_TOKEN_FILE}"
mkdir -p "$(dirname "$ca")" "$(dirname "$token_file")"

# The same image by digest from two registries. Docker verifies a pull by digest against the
# content it receives, so either can only serve the pinned image.
sources=("ghcr.io/$image" "quay.io/$image")
pulled=""
for delay in 2 4 8 0; do
  for source in "${sources[@]}"; do
    if docker pull --quiet "$source"; then
      pulled="$source"
      break 2
    fi
  done
  sleep "$delay"
done
if [ -z "$pulled" ]; then
  echo "could not pull $image from any registry" >&2
  exit 1
fi
echo "pulled $pulled"

# A random fake root token for this run only: the dev server holds nothing real.
token=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')
(umask 077 && printf '%s' "$token" > "$token_file")
docker run -d --name openbao --cap-add IPC_LOCK -p 127.0.0.1:8200:8200 \
  -e "BAO_DEV_ROOT_TOKEN_ID=$token" "$pulled" \
  server -dev -dev-tls -dev-tls-cert-dir=/tmp -dev-listen-address=0.0.0.0:8200 >/dev/null
unset token

for _ in $(seq 1 30); do
  if docker exec openbao test -s /tmp/vault-ca.pem 2>/dev/null \
    && docker cp openbao:/tmp/vault-ca.pem "$ca" 2>/dev/null \
    && curl -sSf --noproxy '*' --cacert "$ca" https://127.0.0.1:8200/v1/sys/health >/dev/null 2>&1; then
    echo "OpenBao is up: CA at $ca, token file at $token_file"
    exit 0
  fi
  sleep 2
done
echo "OpenBao did not become ready" >&2
docker logs openbao >&2 || true
exit 1
