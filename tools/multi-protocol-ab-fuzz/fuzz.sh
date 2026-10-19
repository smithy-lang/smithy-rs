#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

# Run AFL campaigns for every case in parallel.
#
#   bash fuzz.sh <seconds> <fuzzers-per-case>
#   bash fuzz.sh 900 2                        # 15 min, 2 fuzzers x 12 cases = 24 cores
#   SUITES=pokemon CASES=rest-xml bash fuzz.sh 28800 12
#
# Keep (fuzzers-per-case x cases) at or below your core count minus two: oversubscribing makes
# AFL file ordinary inputs as hangs. Each case writes fuzz.log and summary.log to its work directory.
set -uo pipefail
source "$(dirname "$0")/env.sh"
SECONDS_TO_RUN=${1:?seconds}; N=${2:?fuzzers per case}

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
exit "$status"
