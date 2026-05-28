#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# Gathers coverage from a fuzz target's corpus and shows covered lines.
#
# Prerequisites:
#   rustup component add llvm-tools --toolchain nightly
#
# Usage:
#   ./show-corpus-coverage.sh                            # defaults to schema_xml_deserialize
#   ./show-corpus-coverage.sh schema_xml_serialize
#   ./show-corpus-coverage.sh schema_xml_roundtrip
#   ./show-corpus-coverage.sh schema_xml_struct_reorder
#   ./show-corpus-coverage.sh schema_xml_attribute_mix
set -ex

TARGET="${1:-schema_xml_deserialize}"

# Replay the corpus with coverage instrumentation.
# Produces coverage/TARGET/coverage.profdata
cargo +nightly fuzz coverage "$TARGET"

PROFDATA="coverage/$TARGET/coverage.profdata"

# Find the coverage-instrumented binary
COV_BIN=$(find target -name "$TARGET" -path "*/coverage/*" -type f -executable | head -1)
if [ -z "$COV_BIN" ]; then
    echo "Could not find coverage binary for $TARGET"
    exit 1
fi

# Show line-by-line coverage
$(rustc +nightly --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-cov show \
    --use-color \
    --ignore-filename-regex='/.cargo/registry' \
    --instr-profile="$PROFDATA" \
    --object "$COV_BIN" \
    --show-instantiations \
    --show-line-counts-or-regions | less -R
