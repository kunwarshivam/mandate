#!/bin/bash
set -euo pipefail

cluster=mandate-runner-smoke
cleanup() {
  k3d cluster delete "$cluster" > /dev/null 2>&1 || true
}
trap cleanup EXIT

test "$RUNNER_ENVIRONMENT" = self-hosted
test "$(id -u)" -eq 1001
test ! -e /tmp/mandate-runner-smoke-persist
touch /tmp/mandate-runner-smoke-persist
sudo -n true
docker info
k3d version
kubectl version --client
helm version --short
curl -fsS --max-time 20 https://api.github.com/meta > /dev/null
if curl -sS --connect-timeout 2 --max-time 3 -o /dev/null http://169.254.169.254/metadata/v1/ 2> /dev/null; then
  echo "the job reached the host metadata service" >&2
  exit 1
fi
if curl -sS --connect-timeout 2 --max-time 3 -o /dev/null http://10.213.0.1/ 2> /dev/null; then
  echo "the job reached the host bridge" >&2
  exit 1
fi
if curl -sS --connect-timeout 2 --max-time 3 -o /dev/null http://10.0.0.1/ 2> /dev/null; then
  echo "the job reached a private address" >&2
  exit 1
fi
k3d cluster create "$cluster" --wait --timeout 3m
kubectl --context "k3d-$cluster" get nodes
