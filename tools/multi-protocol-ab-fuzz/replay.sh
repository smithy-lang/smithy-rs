#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

# Send every saved crash and hang of every case to both targets and group the divergences.
#
#   bash replay.sh
#   SUITES=pokemon CASES=rest-xml bash replay.sh
#
# Each case writes replay.jsonl and replay.log to its work directory. For samples of a group:
#   python3 abfuzz.py cluster <suite> <case> -v 3
set -uo pipefail
source "$(dirname "$0")/env.sh"

pids=()
for S in $SUITES; do
  for C in $CASES; do
    (cd "$FUZZ_ROOT/work-isolated-v1/$S/$C" 2>/dev/null && python3 "$AB_FUZZ_DIR/abfuzz.py" replay "$S" "$C" > replay.log 2>&1) &
    pids+=("$!")
  done
done
status=0
for pid in "${pids[@]}"; do wait "$pid" || status=1; done
for S in $SUITES; do for C in $CASES; do cat "$FUZZ_ROOT/work-isolated-v1/$S/$C/replay.log" 2>/dev/null; done; done
exit "$status"
