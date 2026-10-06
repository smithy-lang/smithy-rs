#!/usr/bin/env bash
# Generate the servers and fuzz targets for every suite and protocol.
#
#   bash generate.sh
#
# 1. In $BASELINE_REPO: one legacy server per protocol. The generator test is compiled in through
#    a Gradle init script; no file in that checkout's source tree is touched.
# 2. In $CURRENT_REPO: one schema server per protocol, then a paired harness for every comparison.
# 3. AFL++ (once) and the `aws-smithy-fuzz` driver, from $CURRENT_REPO, into $FUZZ_ROOT.
#
# The baseline must be pristine. The current checkout includes local changes.
# Both revisions are recorded in $FUZZ_ROOT/gen/revisions.txt.
set -eu
source "$(dirname "$0")/env.sh"

if [[ -n "$(git -C "$BASELINE_REPO" status --porcelain)" ]]; then
  echo "Baseline must be clean: $BASELINE_REPO. Preserve and resolve local changes before generation." >&2
  exit 1
fi

GEN="$FUZZ_ROOT/gen"
TEST=software/amazon/smithy/rust/codegen/fuzz/MultiProtocolAbFuzzHarnessTest.kt
mkdir -p "$GEN/test-src/$(dirname "$TEST")"
cp "$CURRENT_REPO/fuzzgen/src/test/kotlin/$TEST" "$GEN/test-src/$TEST"

export MP_FUZZ_GENERATE=true
export MP_FUZZ_ISOLATE_PROTOCOLS=true
export MP_FUZZ_OUTPUT="$GEN"
export MP_FUZZ_MODELS="$CURRENT_REPO/codegen-core/common-test-models"
export MP_FUZZ_SUITES="${SUITES// /,}"
FILTER='*MultiProtocolAbFuzzHarnessTest*'

{
  for repo in "$BASELINE_REPO" "$CURRENT_REPO"; do
    echo "== $repo"
    git -C "$repo" log -1 --format='%H %s'
    git -C "$repo" status --short
  done
} > "$GEN/revisions.txt"

echo "baseline: generating single-protocol servers (log: $GEN/single.log)"
(cd "$BASELINE_REPO" && MP_FUZZ_SIDE=single MP_FUZZ_TEST_SRC="$GEN/test-src" \
  ./gradlew -I "$AB_FUZZ_DIR/fuzzgen-extra-tests.init.gradle" \
  :fuzzgen:test --rerun --tests "$FILTER" --no-configuration-cache) > "$GEN/single.log" 2>&1

echo "current: generating isolated schema servers and harnesses (log: $GEN/multi.log)"
(cd "$CURRENT_REPO" && MP_FUZZ_SIDE=multi MP_FUZZ_SINGLE_RUNTIME="$BASELINE_REPO/rust-runtime" \
  ./gradlew :fuzzgen:test --rerun --tests "$FILTER" --no-configuration-cache) > "$GEN/multi.log" 2>&1

mkdir -p "$FUZZ_ROOT/driver" "$XDG_DATA_HOME"
if ! find "$XDG_DATA_HOME" -path '*/afl/bin/afl-fuzz' | grep -q .; then
  echo "driver: building AFL++ for cargo-afl (log: $XDG_DATA_HOME/afl-build.log)"
  cargo afl config --build --force > "$XDG_DATA_HOME/afl-build.log" 2>&1
fi
echo "driver: building aws-smithy-fuzz (log: $FUZZ_ROOT/driver/install.log)"
cargo afl install --path "$CURRENT_REPO/rust-runtime/aws-smithy-fuzz" --root "$FUZZ_ROOT/driver" --force \
  > "$FUZZ_ROOT/driver/install.log" 2>&1

find "$GEN" -name lexicon.json -path '*ab-harness*' | sort
