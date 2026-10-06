#!/usr/bin/env bash
# Run AFL campaigns for every case in parallel.
#
#   bash fuzz.sh <seconds> <fuzzers-per-case>
#   bash fuzz.sh 900 2                        # 15 min, 2 fuzzers x 12 cases = 24 cores
#   SUITES=pokemon CASES=rest-xml bash fuzz.sh 28800 12
#
# Keep (fuzzers-per-case x cases) at or below your core count minus two: oversubscribing makes
# AFL file ordinary inputs as hangs. Each case writes fuzz.log and summary.log to its work directory.
set -u
source "$(dirname "$0")/env.sh"
SECONDS_TO_RUN=${1:?seconds}; N=${2:?fuzzers per case}

run_one() {
  local S=$1 C=$2
  export SMITHY_FUZZ_PROTOCOL="$C"
  cd "$FUZZ_ROOT/work-isolated-v1/$S/$C" || { echo "$S/$C: not initialized"; return; }
  timeout -s INT "$SECONDS_TO_RUN" aws-smithy-fuzz fuzz --num-fuzzers "$N" > fuzz.log 2>&1
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

for S in $SUITES; do for C in $CASES; do run_one "$S" "$C" & done; done
wait
