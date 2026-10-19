#!/usr/bin/env bash
# Runs one cosign signature for release-sign.sh, which starts it with an empty environment
# (`env -i`; DEC-2024 item 4). It sets HOME and PATH from its arguments, reads the release key's
# password from its stdin, so the password is never on a command line (`AGENTS.md` rule 7), and
# execs cosign without the variables bash adds, so cosign's environment holds exactly HOME, PATH
# and COSIGN_PASSWORD.
#
# Usage: release-sign-cosign.sh <home> <tools> <key> <signature> <file>   (the password on stdin)
set +x +v
set -euo pipefail
export HOME="$1" PATH="$2:/usr/bin:/bin"
IFS= read -r COSIGN_PASSWORD
export COSIGN_PASSWORD
exec env -u SHLVL -u PWD -u OLDPWD -u _ cosign sign-blob --yes --new-bundle-format=false \
  --use-signing-config=false --tlog-upload=false --key "$3" --output-signature "$4" "$5"
