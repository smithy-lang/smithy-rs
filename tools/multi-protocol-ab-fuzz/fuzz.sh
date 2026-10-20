#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

# Check prerequisites, generate targets, initialize, fuzz, and replay every selected case.
#
#   bash fuzz.sh <seconds> <fuzzers-per-case>
#   bash fuzz.sh 900 2                        # 15 min, 2 fuzzers x 10 cases = 20 cores
#   SUITES=pokemon CASES=rest-xml bash fuzz.sh 28800 12
#   bash fuzz.sh --prepared 900 2             # use targets already generated and initialized
#
# Keep (fuzzers-per-case x cases) at or below your core count minus two: oversubscribing makes
# AFL file ordinary inputs as hangs. Each case writes fuzz.log and summary.log to its work directory.
set -uo pipefail

usage() {
  echo "Usage: bash fuzz.sh [--prepared] <seconds> <fuzzers-per-case>"
  echo "Set BASELINE_REPO to a clean baseline checkout. SUITES and CASES select comparisons."
  echo "Default: check, generate, initialize, fuzz, replay. --prepared skips generation and initialization."
}

prepared=0
case "${1:-}" in
  --help|-h) usage; exit 0 ;;
  --prepared) prepared=1; shift ;;
esac
if [[ $# -ne 2 || ! $1 =~ ^[1-9][0-9]*$ || ! $2 =~ ^[1-9][0-9]*$ ]]; then
  usage >&2
  exit 2
fi
SECONDS_TO_RUN=$1
N=$2
source "$(dirname "$0")/env.sh"

if [[ $prepared -eq 0 ]]; then
  for command in git java cargo rustup python3 timeout; do
    if ! command -v "$command" > /dev/null; then
      echo "Missing prerequisite: $command" >&2
      exit 1
    fi
  done
  if ! cargo afl --version > /dev/null 2>&1; then
    echo "Missing cargo-afl; install the version matching rust-runtime/aws-smithy-fuzz/Cargo.lock." >&2
    exit 1
  fi
  for repo in "$BASELINE_REPO" "$CURRENT_REPO"; do
    if ! git -C "$repo" rev-parse --show-toplevel > /dev/null 2>&1 || [[ ! -x "$repo/gradlew" ]]; then
      echo "Expected a smithy-rs checkout with executable gradlew at $repo" >&2
      exit 1
    fi
  done
  baseline_status="$(git -C "$BASELINE_REPO" status --porcelain)" || exit 1
  if [[ -n "$baseline_status" ]]; then
    echo "Baseline must be clean: $BASELINE_REPO. Preserve and resolve local changes before generation." >&2
    exit 1
  fi
  echo "=== generate"
  bash "$AB_FUZZ_DIR/generate.sh" || exit $?
  echo "=== initialize and compare seeds"
  bash "$AB_FUZZ_DIR/init.sh" || exit $?
fi

# Validate every case before starting any workers.
missing=0
for S in $SUITES; do
  for C in $CASES; do
    if [[ ! -f "$FUZZ_ROOT/work-isolated-v1/$S/$C/smithy-fuzz-config.json" ]]; then
      echo "$S/$C: not initialized; run fuzz.sh without --prepared" >&2
      missing=1
    fi
  done
done
[[ $missing -eq 0 ]] || exit 1
if ! command -v aws-smithy-fuzz > /dev/null; then
  echo "Fuzz driver missing; run fuzz.sh without --prepared" >&2
  exit 1
fi
echo "=== fuzz"

run_one() {
  local S=$1 C=$2 P
  # The pinned identity is the before side's wire protocol; `@variant` names the after server.
  P="${C%%@*}"
  case "$P" in rest-xml*) P=rest-xml ;; esac
  export SMITHY_FUZZ_PROTOCOL="$P"
  cd "$FUZZ_ROOT/work-isolated-v1/$S/$C" || { echo "$S/$C: not initialized"; return 1; }
  local result=0
  timeout -s INT "$SECONDS_TO_RUN" aws-smithy-fuzz fuzz --num-fuzzers "$N" > fuzz.log 2>&1 || result=$?
  case "$result" in
    0|124|130) ;;
    *) echo "$S/$C: fuzzer failed, see $PWD/fuzz.log"; return "$result" ;;
  esac
  sleep 5
  {
    echo "$S/$C: crashes=$(find afl-output -path '*/crashes/id:*' -type f 2>/dev/null | wc -l) hangs=$(find afl-output -path '*/hangs/id:*' -type f 2>/dev/null | wc -l)"
    for f in $(find afl-output -name fuzzer_stats 2>/dev/null | sort); do
      printf '  %s ' "$(basename "$(dirname "$f")")"
      awk -F': ' '/run_time|execs_done|execs_per_sec|saved_crashes|saved_hangs|corpus_count|bitmap_cvg/ { gsub(/ /,"",$1); printf "%s=%s ", $1, $2 } END { print "" }' "$f"
    done
  } > summary.log
  cat summary.log
}

pids=()
for S in $SUITES; do for C in $CASES; do run_one "$S" "$C" & pids+=("$!"); done; done
status=0
for pid in "${pids[@]}"; do wait "$pid" || status=1; done
echo "=== replay"
bash "$AB_FUZZ_DIR/replay.sh" || status=1
exit "$status"
