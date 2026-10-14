#!/bin/bash
# Capture obscured SQLGetData values from a replay test IR against the OLD
# (reference) driver, apply them into ir.yaml, and regenerate test.cpp.
#
# Usage: odbc_trace_tool/capture_reference.sh <replay_test_dir> [-n name] [-t tag]
#
# Example:
#   odbc_trace_tool/capture_reference.sh \
#     odbc_tests/tests/replay/excel/powerquery/raw_sql/all_datatypes
#
# The regenerated TEST_CASE keeps the name and tags already in test.cpp.
# A directory with no test.cpp requires -n and -t. -t is the tag interior
# generate writes inside "[...]", for example excel][vba_ado][transactions.
# Tags such as vba_ado, dtm, and long_running are not directory segments.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

if [ $# -lt 1 ] || [ -z "${1:-}" ] || [[ "$1" == -* ]]; then
  echo "Usage: $0 <replay_test_dir> [-n name] [-t tag]" >&2
  exit 1
fi
TEST_DIR="$1"
shift

REPLAY_NAME_ARG=""
REPLAY_TAG_ARG=""
while [ $# -gt 0 ]; do
  case "$1" in
    -n)
      if [ $# -lt 2 ] || [ -z "${2:-}" ]; then
        echo "Error: -n requires a test name" >&2
        exit 1
      fi
      REPLAY_NAME_ARG="$2"
      shift 2
      ;;
    -t)
      if [ $# -lt 2 ] || [ -z "${2:-}" ]; then
        echo "Error: -t requires a Catch2 tag" >&2
        exit 1
      fi
      REPLAY_TAG_ARG="$2"
      shift 2
      ;;
    *)
      echo "Error: unknown argument: $1" >&2
      echo "Usage: $0 <replay_test_dir> [-n name] [-t tag]" >&2
      exit 1
      ;;
  esac
done

if [ ! -f "$TEST_DIR/ir.yaml" ]; then
  echo "Error: $TEST_DIR/ir.yaml not found" >&2
  exit 1
fi

# Tags such as vba_ado, dtm, and long_running are not directory segments,
# so a missing test.cpp does not invent a Catch2 identity from the path.
replay_name_and_tag() {
  local test_dir="$1"
  local cpp="$test_dir/test.cpp"
  if [ -f "$cpp" ]; then
    if [ -n "$REPLAY_NAME_ARG" ] || [ -n "$REPLAY_TAG_ARG" ]; then
      echo "Error: $cpp already defines the Replay TEST_CASE; omit -n and -t" >&2
      return 1
    fi
    local line
    line="$(grep -m1 -E 'TEST_CASE\("Replay: ' "$cpp" || true)"
    if [ -z "$line" ]; then
      echo "Error: $cpp has no Replay TEST_CASE" >&2
      return 1
    fi
    if [[ ! "$line" =~ TEST_CASE\(\"Replay:\ ([^\"]+)\",\ \"\[(.*)\]\"\) ]]; then
      echo "Error: could not parse TEST_CASE in $cpp" >&2
      return 1
    fi
    printf '%s\n%s\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
    return 0
  fi

  if [ -z "$REPLAY_NAME_ARG" ] || [ -z "$REPLAY_TAG_ARG" ]; then
    echo "Error: $cpp not found. Pass -n and -t. The directory path is not a Catch2 identity." >&2
    return 1
  fi
  printf '%s\n%s\n' "$REPLAY_NAME_ARG" "$REPLAY_TAG_ARG"
}

REPLAY_IDENTITY="$(replay_name_and_tag "$TEST_DIR")"
REPLAY_NAME="$(printf '%s\n' "$REPLAY_IDENTITY" | sed -n '1p')"
REPLAY_TAG="$(printf '%s\n' "$REPLAY_IDENTITY" | sed -n '2p')"

if ! docker info >/dev/null 2>&1; then
  echo "Error: Docker is not running" >&2
  exit 1
fi

CAPTURE_CPP="$PROJECT_ROOT/odbc_tests/tests/capture_harness/capture.cpp"
CAPTURE_JSON="$PROJECT_ROOT/odbc_tests/tests/capture_harness/capture.json"
BUILD_DIR="$PROJECT_ROOT/odbc_tests/cmake-build-capture"
IR="$TEST_DIR/ir.yaml"

REFERENCE_ODBC_VERSION=$(cat "$PROJECT_ROOT/ci/reference-odbc-version" | tr -d '[:space:]')
. "$PROJECT_ROOT/ci/reference-odbc-checksums"

HOST_ARCH=$(uname -m)
if [ "$HOST_ARCH" = "x86_64" ]; then
  REFERENCE_ODBC_SHA256="$DEB_X86_64_SHA256"
else
  REFERENCE_ODBC_SHA256="$DEB_AARCH64_SHA256"
fi

cleanup() {
  rm -f "$CAPTURE_CPP" "$CAPTURE_JSON"
  rm -rf "$BUILD_DIR"
}
trap cleanup EXIT

# Pre-clean so a stale capture.json from an aborted run can never be applied.
rm -f "$CAPTURE_CPP" "$CAPTURE_JSON"
rm -rf "$BUILD_DIR"

echo "Generating capture harness from $IR ..."
cargo run -p odbc_trace_tool -- generate \
  --emit-capture-harness \
  -i "$IR" \
  -o "$CAPTURE_CPP"

echo "Building Docker image for ODBC reference tests ..."
docker build \
  --build-arg REFERENCE_ODBC_VERSION="$REFERENCE_ODBC_VERSION" \
  --build-arg REFERENCE_ODBC_SHA256="$REFERENCE_ODBC_SHA256" \
  -t odbc-reference-tests "$PROJECT_ROOT/odbc_tests"

echo "Running capture harness against OLD driver ..."
set +e
docker run --rm \
  -v "$PROJECT_ROOT":/workspace \
  -w /workspace \
  -e DRIVER_PATH="/usr/lib/snowflake/odbc/lib/libSnowflake.so" \
  -e PARAMETER_PATH="/workspace/parameters.json" \
  -e GIT_ROOT="/workspace" \
  -e CAPTURE_OUTPUT_PATH="/workspace/odbc_tests/tests/capture_harness/capture.json" \
  odbc-reference-tests \
  bash -c "
    set -e
    set -x
    if [ ! -f /workspace/parameters.json ]; then
      echo 'Error: parameters.json not found. Please run ./scripts/decode_secrets.sh first.'
      exit 1
    fi
    ODBC_LIB=\$(find /usr/lib -name 'libodbc.so' -print -quit)
    cd /workspace/odbc_tests/
    mkdir -p cmake-build-capture
    cmake -B cmake-build-capture \\
      -D ODBC_LIBRARY=\"\$ODBC_LIB\" \\
      -D ODBC_INCLUDE_DIR='/usr/include' \\
      -D DRIVER_TYPE=OLD \\
      -D BUILD_CAPTURE_HARNESS=ON \\
      -D CMAKE_CXX_COMPILER_LAUNCHER=ccache \\
      -D CMAKE_C_COMPILER_LAUNCHER=ccache \\
      .
    cmake --build cmake-build-capture --target capture_harness -- -j \$(nproc)
    ctest -C Debug --test-dir cmake-build-capture -R capture_harness --output-on-failure
  "
CTEST_EXIT=$?
set -e

if [ "$CTEST_EXIT" -ne 0 ]; then
  echo "Error: capture harness failed (ctest exit $CTEST_EXIT); ir.yaml left untouched" >&2
  exit "$CTEST_EXIT"
fi

if [ ! -f "$CAPTURE_JSON" ]; then
  echo "Error: capture.json was not written" >&2
  exit 1
fi

echo "Applying captured values to $IR ..."
cargo run -p odbc_trace_tool -- apply-capture \
  -i "$IR" \
  --values "$CAPTURE_JSON"

echo "Regenerating test.cpp ..."
cargo run -p odbc_trace_tool -- generate \
  -i "$IR" \
  -o "$TEST_DIR/test.cpp" \
  -n "$REPLAY_NAME" \
  -t "$REPLAY_TAG"

echo "Capture complete: $IR and $TEST_DIR/test.cpp updated"
