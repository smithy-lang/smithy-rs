#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#
# Run by CI to check the canary-lambda
set -e
cd "$(dirname $0)"

SDK_PATH="$(git rev-parse --show-toplevel)"/aws/sdk/build/aws-sdk/sdk
if [[ "${GITHUB_ACTIONS}" == "true" ]]; then
   SDK_PATH="$(git rev-parse --show-toplevel)"/aws-sdk/sdk
fi

./write-cargo-toml.py --path "${SDK_PATH}"
cargo check
cargo clippy
