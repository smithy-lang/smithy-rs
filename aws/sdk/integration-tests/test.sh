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
      cargo test --manifest-path "${f}/Cargo.toml" --all-features
   fi
done
