#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

set -eu -o pipefail

for f in *; do
   if [[ -f "${f}/Cargo.toml" ]]; then
      echo
      echo "Testing ${f}..."
      echo "###############"
      if [ "$f" != "webassembly" ]; then
         cargo test --manifest-path "${f}/Cargo.toml" --all-features
      else
         # The webassembly tests use a custom runner set in config.toml that
         # is not picked up when running the tests outside of the package
         # The tests are written for `wasm32-wasip1` but the manifest config also specifies
         # `wasm32-unknown-unknown` so we can ensure the test build on that platform as well.
         # For executing the tests, however, we explicitly choose a target `wasm32-wasip1`.
         cd webassembly && cargo component test --all-features --target wasm32-wasip1 && cd ..
      fi
   fi
done
