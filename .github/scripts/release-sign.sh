#!/usr/bin/env bash
# Signs a release with the release key, in `release.yml`'s `release-signing` job only (DEC-2024; the
# founder's route is DEC-2605 rows 8 and 9). It runs beside the key, so it builds nothing and runs
# no repository build. It turns off any trace the job's environment asked for before anything else,
# so no secret is echoed. It requires dist and dist/bin to be real directories and dist/SHA256SUMS
# a regular file, dist/bin to hold exactly the names SHA256SUMS lists, once each, as regular files,
# and says which rule failed. It copies those files into a private directory under RUNNER_TEMP,
# checks the copies against SHA256SUMS and SHA256SUMS against build-a's DIGEST, and signs the
# copies, so nothing can change between the check and the signature. It installs cosign from
# COSIGN_URL pinned to COSIGN_SHA256. Each signature starts release-sign-cosign.sh from an empty
# environment (`env -i`); it sets HOME and PATH, reads the password from its stdin and execs cosign
# with exactly HOME, PATH and COSIGN_PASSWORD, so the password is never on a command line
# (`AGENTS.md` rule 7). RELEASE_SIGNING_KEY is cosign.key as single-line base64, decoded into a 0600 file under
# RUNNER_TEMP; the key and the copies are deleted on every exit, and neither secret is printed.
#
# Usage: release-sign.sh   (with DIGEST, COSIGN_URL, COSIGN_SHA256, RELEASE_SIGNING_KEY,
#                           COSIGN_PASSWORD and RUNNER_TEMP set)
set +x +v
set -euo pipefail
umask 077

sums=dist/SHA256SUMS
if [ ! -d dist ] || [ -L dist ]; then
  echo "dist must be a real directory, not a symlink; nothing is signed" >&2
  exit 1
fi
if [ ! -f "$sums" ] || [ -L "$sums" ]; then
  echo "dist/SHA256SUMS must be a regular file, not a symlink; nothing is signed" >&2
  exit 1
fi
if [ ! -d dist/bin ] || [ -L dist/bin ]; then
  echo "dist/bin must be a real directory, not a symlink; nothing is signed" >&2
  exit 1
fi
if [ -n "$(find dist/bin -mindepth 1 -not -type f -print -quit)" ]; then
  echo "dist/bin holds something that is not a regular file; nothing is signed" >&2
  exit 1
fi
if grep -Evq '^[0-9a-f]{64}  [A-Za-z0-9._-]+$' "$sums" || [ ! -s "$sums" ]; then
  echo "SHA256SUMS is empty or holds a line that is not '<sha256>  <name>'" >&2
  exit 1
fi
mapfile -t names < <(cut -c67- dist/SHA256SUMS | LC_ALL=C sort)
mapfile -t present < <(find dist/bin -mindepth 1 -printf '%P\n' | LC_ALL=C sort)
previous=
for name in "${names[@]}"; do
  if [ "$name" = "$previous" ]; then
    echo "SHA256SUMS lists $name twice; nothing is signed" >&2
    exit 1
  fi
  previous=$name
done
for name in "${present[@]}"; do
  found=
  for want in "${names[@]}"; do
    if [ "$want" = "$name" ]; then
      found=yes
    fi
  done
  if [ -z "$found" ]; then
    echo "dist/bin holds $name, which SHA256SUMS does not list; nothing is signed" >&2
    exit 1
  fi
done
if [ "${#present[@]}" -ne "${#names[@]}" ]; then
  echo "SHA256SUMS lists a binary dist/bin does not hold; nothing is signed" >&2
  exit 1
fi

verified="$RUNNER_TEMP/release-verified"
tools="$RUNNER_TEMP/release-tools"
key_dir="$RUNNER_TEMP/release-key"
trap 'rm -rf "$key_dir" "$verified"' EXIT
rm -rf "$verified"
mkdir -p "$verified/bin"
cp -- "$sums" "$verified/SHA256SUMS"
for name in "${names[@]}"; do
  cp -- "dist/bin/$name" "$verified/bin/$name"
done
(cd "$verified/bin" && sha256sum --check --strict ../SHA256SUMS)
printf '%s  SHA256SUMS\n' "$DIGEST" > "$verified/DIGEST"
(cd "$verified" && sha256sum --check --strict DIGEST)

mkdir -p "$tools" "$key_dir/home" signatures
curl -sSfL --retry 5 --retry-delay 2 --retry-all-errors -o "$tools/cosign" "$COSIGN_URL"
printf '%s  %s\n' "$COSIGN_SHA256" "$tools/cosign" > "$tools/cosign.sha256"
sha256sum --check --strict "$tools/cosign.sha256"
chmod 0500 "$tools/cosign"

printf '%s' "$RELEASE_SIGNING_KEY" > "$key_dir/cosign.key.base64"
base64 --decode "$key_dir/cosign.key.base64" > "$key_dir/cosign.key"
unset RELEASE_SIGNING_KEY
for file in "${names[@]/#/$verified/bin/}" "$verified/SHA256SUMS"; do
  env -i "$(dirname "$0")/release-sign-cosign.sh" "$key_dir/home" "$tools" "$key_dir/cosign.key" \
    "signatures/$(basename "$file").sig" "$file" <<<"$COSIGN_PASSWORD"
done
