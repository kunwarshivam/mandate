#!/usr/bin/env bash
# The credential guard of CI's `fast-rh-sim-live` job (DEC-964), run in the same shell right
# before the runner's tests in the `live,rh-sim` build: fails when any variable of the environment
# has a name that could carry a broker or market-data credential, naming each such variable and
# never printing a value. The rehearsal build dials loopback alone (DEC-922); this keeps a
# credential from reaching it at all. The name pieces match xtask's `CREDENTIAL_NAME_PARTS`.
#
# Usage: no-credentials.sh
set -euo pipefail
found=$(compgen -e | grep -E -i 'KEY|SECRET|TOKEN|PASSWORD|PASSWD|CREDENTIAL|ALPACA|APCA|ROBINHOOD' || true)
if [ -n "$found" ]; then
  echo "the environment holds names that could carry a credential (DEC-964):" >&2
  echo "$found" >&2
  exit 1
fi
echo "no credential names in the environment"
