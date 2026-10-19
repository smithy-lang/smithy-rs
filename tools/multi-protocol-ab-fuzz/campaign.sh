#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

# Three-phase A/B AFL campaign: legacy single-protocol servers against
#   1. schema single-protocol servers        (strict comparison)
#   2. schema non-conflicting bunch servers  (strict comparison)
#   3. schema full multi-protocol server     (strict comparison)
#
#   bash campaign.sh                 # 1.5h + 1.5h + 2h of fuzzing, phases sequential
#   PHASE_SECONDS="600 600 600" bash campaign.sh   # shorter smoke campaign
#
# Requires the three generation passes in $FUZZ_ROOT/gen (see AB_FUZZING.md): the baseline
# single side, the schema side with MP_FUZZ_ISOLATE_PROTOCOLS=true, and the schema side with
# MP_FUZZ_VARIANTS=full/full-xml/bunch-json/bunch-xml.
set -euo pipefail
cd "$(dirname "$0")"
: "${FUZZ_ROOT:=$PWD/artifacts-live}"
export FUZZ_ROOT
export SUITES="${SUITES:-multiprotocol pokemon}"
read -r P1_SECONDS P2_SECONDS P3_SECONDS <<< "${PHASE_SECONDS:-5400 5400 7200}"

PHASE1_CASES="aws-json-10 aws-json-11 rest-json1 rest-xml rpcv2-cbor"
PHASE2_CASES="aws-json-10@bunch-json aws-json-11@bunch-xml rest-json1@bunch-json rest-xml@bunch-xml rpcv2-cbor@bunch-json rpcv2-cbor@bunch-xml"
PHASE3_CASES="aws-json-10@full aws-json-11@full rest-json1@full rest-xml@full rpcv2-cbor@full rest-xml-nojson@full-xml"

# Rebuild the driver so campaign behavior matches the current runtime sources.
source env.sh
echo "=== driver: rebuilding aws-smithy-fuzz"
cargo afl install --quiet --path "$CURRENT_REPO/rust-runtime/aws-smithy-fuzz" \
  --root "$FUZZ_ROOT/driver" --force > "$FUZZ_ROOT/driver/campaign-install.log" 2>&1 \
  || { echo "driver rebuild failed, see $FUZZ_ROOT/driver/campaign-install.log"; exit 1; }

phase() {
  local name=$1 cases=$2 seconds=$3 fuzzers=$4
  echo "=== phase $name: init ($(date))"
  CASES="$cases" JOBS=6 bash init.sh
  echo "=== phase $name: fuzz ${seconds}s, $fuzzers fuzzers/case ($(date))"
  CASES="$cases" bash fuzz.sh "$seconds" "$fuzzers"
  echo "=== phase $name: replay ($(date))"
  CASES="$cases" bash replay.sh
}

phase single "$PHASE1_CASES" "$P1_SECONDS" 5
phase bunch "$PHASE2_CASES" "$P2_SECONDS" 4
phase full "$PHASE3_CASES" "$P3_SECONDS" 4
echo "=== campaign complete ($(date))"
