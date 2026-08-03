#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
# `cargo nextest run` doesn't run doctests, so the nextest-based scripts run `cargo test --doc`
# too -- but that errors on crates with no plain `lib` target (bin-only, and cdylib/staticlib
# wasm crates). This helper runs `--doc` only when the target package has a `lib`, skipping it
# otherwise.

# run_doctests_if_lib "<toolchain-or-empty>" [extra cargo args...]
run_doctests_if_lib() {
    local toolchain="$1"; shift
    local toolchain_arg=()
    [[ -n "${toolchain}" ]] && toolchain_arg=("+${toolchain}")

    # The package `--doc` targets is the one whose manifest is --manifest-path, else <cwd>/Cargo.toml.
    # cargo metadata --no-deps lists every workspace member, so match on that manifest specifically.
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
