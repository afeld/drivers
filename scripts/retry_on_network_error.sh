#!/usr/bin/env bash
# Run a package-manager command, retrying only when its output shows a
# registry/network failure.
#
# Usage: retry_on_network_error.sh <npm|nuget> -- <command> [args...]
#
# GitHub-hosted runners intermittently lose the npm registry and nuget.org
# (ECONNRESET, EAI_AGAIN, NU1301 service-index failures, 5xx/429). A failure
# whose output matches none of the markers below (a missing package, a
# dependency conflict, a compiler error) is returned on the first attempt.
set -uo pipefail

MAX_ATTEMPTS="${NETWORK_RETRY_ATTEMPTS:-4}"
BACKOFF_SECONDS="${NETWORK_RETRY_BACKOFF_SECONDS:-15}"

# npm prints "npm error code <CODE>" (npm 11) or "npm ERR! code <CODE>" (npm 10);
# the E<status> codes are registry HTTP statuses.
NPM_PATTERN='npm (ERR!|error) code (ECONNRESET|ECONNREFUSED|ETIMEDOUT|ESOCKETTIMEDOUT|EAI_AGAIN|ENOTFOUND|ENETUNREACH|EHOSTUNREACH|EPIPE|E408|E429|E5[0-9][0-9])([^A-Za-z0-9]|$)|npm (ERR!|error) network |socket hang up'

NUGET_PATTERN='NU1301|Response status code does not indicate success: (408|429|5[0-9][0-9])|The SSL connection could not be established|An error occurred while sending the request|Resource temporarily unavailable|Name or service not known|No such host is known|Connection reset by peer'

usage() {
  echo "usage: $0 <npm|nuget> -- <command> [args...]" >&2
  exit 2
}

[[ $# -ge 3 && "$2" == "--" ]] || usage
case "$1" in
  npm) pattern="$NPM_PATTERN" ;;
  nuget) pattern="$NUGET_PATTERN" ;;
  *) usage ;;
esac
shift 2

log_file="$(mktemp)"
trap 'rm -f "$log_file"' EXIT

for (( attempt = 1; attempt <= MAX_ATTEMPTS; attempt++ )); do
  echo "retry_on_network_error: attempt ${attempt}/${MAX_ATTEMPTS}: $*"

  "$@" 2>&1 | tee "$log_file"
  status="${PIPESTATUS[0]}"

  if [[ "$status" -eq 0 ]]; then
    exit 0
  fi

  if ! grep -Eq "$pattern" "$log_file"; then
    echo "retry_on_network_error: failure is not a network/registry error; not retrying." >&2
    exit "$status"
  fi

  if (( attempt == MAX_ATTEMPTS )); then
    echo "retry_on_network_error: exhausted ${MAX_ATTEMPTS} attempts; failing with exit code ${status}." >&2
    exit "$status"
  fi

  sleep_seconds=$(( BACKOFF_SECONDS * attempt ))
  echo "::warning::retry_on_network_error: network/registry failure on attempt ${attempt}/${MAX_ATTEMPTS}; retrying in ${sleep_seconds}s."
  sleep "$sleep_seconds"
done
