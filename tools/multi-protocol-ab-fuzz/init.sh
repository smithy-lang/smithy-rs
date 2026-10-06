#!/usr/bin/env bash
# Build both fuzz targets of every case, write the seed corpus, and compare the targets on it.
#
#   bash init.sh                              # every suite and case
#   SUITES=pokemon CASES="rest-xml rpcv2-cbor" bash init.sh
#   FRESH=1 bash init.sh                      # also delete previous AFL output
#   JOBS=2 bash init.sh                       # cases built at once (default 3)
#
# Without FRESH=1 the existing afl-output (saved crashes) is kept, so after a fix you can rebuild
# and replay old crashes against the new targets with replay.sh. Targets are always rebuilt:
# they depend on both checkouts' runtimes by path.
set -u
source "$(dirname "$0")/env.sh"

init_one() {
  local S=$1 C=$2
  local H="$FUZZ_ROOT/gen/$S/$(case_harness "$C")" W="$FUZZ_ROOT/work-isolated-v1/$S/$C"
  if [ ! -f "$H/lexicon.json" ]; then
    echo "$S/$C: no harness at $H (run generate.sh first)"; return 1
  fi
  [ "${FRESH:-0}" = 1 ] && rm -rf "$W/afl-input" "$W/afl-output" "$W/replay.jsonl"
  mkdir -p "$W" && cd "$W" || return 1
  if ! aws-smithy-fuzz initialize --lexicon "$H/lexicon.json" \
      --target-crate "$H/single" --target-crate "$H/multi" \
      --release --force-rebuild > initialize.log 2>&1; then
    echo "$S/$C: initialize FAILED, see $W/initialize.log"; return 1
  fi
  python3 "$AB_FUZZ_DIR/abfuzz.py" seed "$S" "$C" > /dev/null
  python3 "$AB_FUZZ_DIR/abfuzz.py" corpus-diff "$S" "$C" > corpus-diff.log 2>&1
  tail -1 corpus-diff.log
}

for S in $SUITES; do
  for C in $CASES; do
    while [ "$(jobs -rp | wc -l)" -ge "${JOBS:-3}" ]; do wait -n; done
    init_one "$S" "$C" &
  done
done
wait
