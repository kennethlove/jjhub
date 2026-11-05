#!/usr/bin/env bash
# The kata pin has exactly one home: the shell default at the checkout
# command in Dockerfile.kata. Coolify passes --build-arg KATA_COMMIT bare
# with an empty value (BUG-002), which beats ARG defaults and compose
# defaults alike, so the fallback must live where the value is consumed.
# CI extracts the pin from there too — a second literal would drift.
set -euo pipefail

cd "$(dirname "$0")/.."

fail=0

pin=$(sed -n 's/.*KATA_COMMIT:-\([0-9a-f]\{40\}\).*/\1/p' Dockerfile.kata)
if [ -z "$pin" ]; then
  echo "FAIL: Dockerfile.kata has no KATA_COMMIT:-<40-hex> shell default" >&2
  fail=1
fi

if grep -nE 'KATA_COMMIT.*[0-9a-f]{40}' docker-compose.yml; then
  echo "FAIL: docker-compose.yml carries a second KATA_COMMIT pin literal" >&2
  fail=1
fi

if ! grep -q 'Dockerfile.kata' .github/workflows/ci.yml; then
  echo "FAIL: CI does not extract the pin from Dockerfile.kata" >&2
  fail=1
fi

if [ "$fail" -eq 0 ]; then
  echo "OK: kata pin single-sourced in Dockerfile.kata ($pin)"
fi
exit "$fail"
