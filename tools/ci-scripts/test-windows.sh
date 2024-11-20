#!/usr/bin/env bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

set -eu -o pipefail

exclusions=("--exclude" "aws-smithy-http-server-python" "--exclude" "aws-smithy-http-server-typescript" "--exclude" "aws-smithy-http-client")
for runtime_path in "rust-runtime" "aws/rust-runtime"; do
  echo "testing $runtime_path"
  pushd "${runtime_path}" &>/dev/null
  # aws-smithy-http-server-python cannot be compiled on Windows since it uses the `signal-hook` crate
  # which is not really yet fully supported on the platform.
  # aws-smithy-http-server-typescript cannot be compiled right now on Windows.
  cargo test --all-features --workspace "${exclusions[@]}"
  cargo doc --no-deps --document-private-items --all-features --workspace "${exclusions[@]}"
  popd &>/dev/null
done
# TODO(https://github.com/awslabs/aws-sdk-rust/issues/1117) We don't have a way to codegen the deps needed by the aws-config crate
# (cd aws/rust-runtime/aws-config && cargo test --all-features) # aws-config is not part of the workspace so we have to test it separately
echo "Testing isolated features of aws-smithy-http-client"
(cd rust-runtime && cargo test -p aws-smithy-http-client --features rustls-ring) # only ring works on windows
