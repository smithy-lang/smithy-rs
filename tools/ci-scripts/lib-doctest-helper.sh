#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
# Shared helper for the nextest-based test scripts. Since `cargo nextest run` does not run
# doctests, those scripts run `cargo test --doc` separately -- but `--doc` errors on packages
# that have no doctest-capable library (binary-only crates, and `cdylib`/`staticlib`-only crates
# like the wasm test crates). This helper runs `--doc` only when a package has a plain `lib`
# target, so the doctest step is a no-op rather than an error on those crates.

# Runs `cargo [+toolchain] test --doc <extra args...>` iff the package (selected by the same
# extra args, e.g. --manifest-path) has a plain `lib` target. Pass the toolchain (or "") first.
#   run_doctests_if_lib "<toolchain-or-empty>" [extra cargo args...]
run_doctests_if_lib() {
    local toolchain="$1"; shift
    local toolchain_arg=()
    [[ -n "${toolchain}" ]] && toolchain_arg=("+${toolchain}")

    # A package can have `lib`, `cdylib`, `staticlib`, `bin`, `proc-macro`, `bench`, `test`
    # targets. `--doc` only works for the plain `lib` kind, so check for it via cargo metadata.
    if cargo "${toolchain_arg[@]}" metadata --no-deps --format-version 1 "$@" 2>/dev/null \
        | python3 -c 'import json,sys; sys.exit(0 if any("lib" in t["kind"] for p in json.load(sys.stdin)["packages"] for t in p["targets"]) else 1)'; then
        cargo "${toolchain_arg[@]}" test --all-features --doc "$@"
    else
        echo "Skipping doctests: no plain lib target"
    fi
}
