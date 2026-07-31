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

# Runs `cargo [+toolchain] test --doc <extra args...>` iff the package that `--doc` would target
# has a plain `lib` target. Pass the toolchain (or "") first.
#   run_doctests_if_lib "<toolchain-or-empty>" [extra cargo args...]
#
# Note: `cargo metadata --no-deps` returns EVERY workspace member, not just the one being tested,
# so we can't just check "does any package have a lib". We resolve the target package by the
# manifest `--doc` will use -- the `--manifest-path` arg if given, else `<cwd>/Cargo.toml` -- and
# check only that package's target kinds. `--doc` works only for the plain `lib` kind (not
# `bin`, `cdylib`, `staticlib`, or `proc-macro`).
run_doctests_if_lib() {
    local toolchain="$1"; shift
    local toolchain_arg=()
    [[ -n "${toolchain}" ]] && toolchain_arg=("+${toolchain}")

    # Determine the manifest that `cargo test --doc "$@"` will target.
    local manifest="${PWD}/Cargo.toml"
    local prev=""
    for arg in "$@"; do
        [[ "${prev}" == "--manifest-path" ]] && manifest="${arg}"
        prev="${arg}"
    done

    if cargo "${toolchain_arg[@]}" metadata --no-deps --format-version 1 "$@" 2>/dev/null \
        | MANIFEST="${manifest}" python3 -c '
import json, os, sys
target = os.path.realpath(os.environ["MANIFEST"])
pkgs = json.load(sys.stdin)["packages"]
pkg = next((p for p in pkgs if os.path.realpath(p["manifest_path"]) == target), None)
has_lib = pkg is not None and any("lib" in t["kind"] for t in pkg["targets"])
sys.exit(0 if has_lib else 1)'; then
        cargo "${toolchain_arg[@]}" test --all-features --doc "$@"
    else
        echo "Skipping doctests: no plain lib target for ${manifest}"
    fi
}
