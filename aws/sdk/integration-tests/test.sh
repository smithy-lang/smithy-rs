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
         cd webassembly && cargo component test && cd ..
      fi
   fi
done
