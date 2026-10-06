#!/bin/zsh
# Compiles every generated server crate in the coverage corpus and prints a PASS/FAIL matrix.
# Shared target dir so runtime deps compile once.
set -u
ROOT=/local/home/fahadzub/decoupled/smithy-rs-fable-clean-multi/coverage/build/smithyprojections/coverage
export CARGO_TARGET_DIR=/tmp/coverage-cargo-target
PASS=0; FAIL=0
: > /tmp/coverage-matrix.txt
for dir in $ROOT/*/rust-server-codegen; do
    [ -f "$dir/Cargo.toml" ] || continue
    module=$(basename $(dirname "$dir"))
    if (cd "$dir" && cargo check --quiet > /tmp/coverage-check-$module.log 2>&1); then
        echo "PASS $module" >> /tmp/coverage-matrix.txt; PASS=$((PASS+1))
    else
        echo "FAIL $module" >> /tmp/coverage-matrix.txt; FAIL=$((FAIL+1))
    fi
done
echo "passed: $PASS  failed: $FAIL"
sort /tmp/coverage-matrix.txt | awk '{print $2, $1}' | column -t
