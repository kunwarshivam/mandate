#!/usr/bin/env bash
# Signs a release with the release key, in `release.yml`'s `release-signing` job only (DEC-2024; the
# founder's route is DEC-2605 rows 8 and 9). It runs beside the key, so it builds nothing and runs
# no repository build: it checks the handed-on binaries in dist/bin against dist/SHA256SUMS and
# SHA256SUMS against build-a's DIGEST, installs cosign from COSIGN_URL pinned to COSIGN_SHA256, and
# signs each binary and SHA256SUMS into signatures/ in a clean environment that holds only the
# password, HOME and PATH. RELEASE_SIGNING_KEY is cosign.key as single-line base64, decoded into a
# 0600 file under RUNNER_TEMP and deleted on every exit; neither secret is printed.
#
# Usage: release-sign.sh   (with DIGEST, COSIGN_URL, COSIGN_SHA256, RELEASE_SIGNING_KEY,
#                           COSIGN_PASSWORD and RUNNER_TEMP set)
set -euo pipefail
umask 077

(cd dist/bin && sha256sum --check --strict ../SHA256SUMS)
echo "${DIGEST}  dist/SHA256SUMS" | sha256sum --check --strict

tools="$RUNNER_TEMP/release-tools"
key_dir="$RUNNER_TEMP/release-key"
trap 'rm -rf "$key_dir"' EXIT
mkdir -p "$tools" "$key_dir/home" signatures
curl -sSfL --retry 5 --retry-delay 2 --retry-all-errors -o "$tools/cosign" "$COSIGN_URL"
printf '%s  %s\n' "$COSIGN_SHA256" "$tools/cosign" > "$tools/cosign.sha256"
sha256sum --check --strict "$tools/cosign.sha256"
chmod 0500 "$tools/cosign"

printf '%s' "$RELEASE_SIGNING_KEY" > "$key_dir/cosign.key.base64"
base64 --decode "$key_dir/cosign.key.base64" > "$key_dir/cosign.key"
unset RELEASE_SIGNING_KEY
for file in dist/bin/* dist/SHA256SUMS; do
  env -i HOME="$key_dir/home" PATH="$tools:/usr/bin:/bin" COSIGN_PASSWORD="$COSIGN_PASSWORD" \
    cosign sign-blob --yes --new-bundle-format=false --use-signing-config=false \
    --tlog-upload=false --key "$key_dir/cosign.key" \
    --output-signature "signatures/$(basename "$file").sig" "$file"
done
