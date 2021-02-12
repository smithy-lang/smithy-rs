#!/bin/bash

#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

set -e
for crate in "$(dirname "$0")"/*/
do
  if [ -d "$crate" ] && [ -f "$crate/Cargo.toml" ]; then
    echo "Testing $crate"
    (cd "$crate" && cargo fmt)
    (cd "$crate" && cargo fmt -- --check)
    (cd "$crate" && cargo clippy -- -D warnings)
    (cd "$crate" && cargo test)
    (cd "$crate" && RUSTDOCFLAGS="-D warnings" cargo doc --no-deps)
  fi
done
