#!/usr/bin/env bash
# Keep tests/performance/drivers/core/app/Cargo.lock in sync with the isolated
# core-perf-driver workspace (it is not a member of the root workspace).
#
# The Core perf Dockerfile runs `cargo build --locked` against that lockfile.
# If a path-dep manifest gains an edge the lockfile does not record, that image
# build fails on main instead of on the PR that changed the manifest.
#
# The hook also matches those path-dep manifests, so it runs in checkouts that
# omit tests/performance. Check mode succeeds when this workspace is absent.
# Refresh still requires the manifest.
#
# Usage:
#   scripts/check-core-perf-cargo-lock.sh          # fail if --locked would rewrite
#   scripts/check-core-perf-cargo-lock.sh refresh  # rewrite lock without version bumps
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mode="${1:-check}"
manifest="$root/tests/performance/drivers/core/app/Cargo.toml"

if [[ ! -f "$manifest" ]]; then
  if [[ "$mode" == "check" ]]; then
    echo "tests/performance/drivers/core/app/Cargo.toml is absent; skipping the core-perf-driver lock check."
    exit 0
  fi
  if [[ "$mode" == "refresh" ]]; then
    echo "tests/performance/drivers/core/app/Cargo.toml does not exist; nothing to refresh." >&2
    exit 1
  fi
fi

cargo_metadata() {
  local extra=()
  if [[ "$1" == "locked" ]]; then
    extra+=(--locked)
  fi
  cargo metadata \
    --manifest-path "$manifest" \
    --format-version 1 \
    "${extra[@]}" \
    >/dev/null
}

case "$mode" in
  check)
    if ! cargo_metadata locked; then
      echo "tests/performance/drivers/core/app/Cargo.lock is stale for the core-perf-driver workspace." >&2
      echo "Refresh it with: scripts/check-core-perf-cargo-lock.sh refresh" >&2
      exit 1
    fi
    echo "tests/performance/drivers/core/app/Cargo.lock accepts cargo metadata --locked"
    ;;
  refresh)
    cargo_metadata rewrite
    cargo_metadata locked
    echo "Wrote $root/tests/performance/drivers/core/app/Cargo.lock"
    ;;
  *)
    echo "Usage: $0 [check|refresh]" >&2
    exit 2
    ;;
esac
