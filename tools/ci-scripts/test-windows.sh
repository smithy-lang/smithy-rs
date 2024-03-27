#!/usr/bin/env bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

set -eu -o pipefail

exclusions=("--exclude" "aws-smithy-http-server-python" "--exclude" "aws-smithy-http-server-typescript" "--exclude" "aws-smithy-experimental")
for runtime_path in "rust-runtime" "aws/rust-runtime"; do
  pushd "${runtime_path}" &>/dev/null
  # aws-smithy-http-server-python cannot be compiled on Windows since it uses the `signal-hook` crate
  # which is not really yet fully supported on the platform.
  # aws-smithy-http-server-typescript cannot be compiled right now on Windows.
  cargo test --all-features --workspace "${exclusions[@]}"
  cargo doc --no-deps --document-private-items --all-features --workspace "${exclusions[@]}"
  popd &>/dev/null
done
(cd aws/rust-runtime/aws-config && cargo test) # aws-config is not part of the workspace so we have to test it separately
(cd rust-runtime && cargo test -p aws-smithy-experimental --features crypto-ring) # only ring works on windows
