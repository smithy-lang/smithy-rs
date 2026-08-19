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
# aws-config is not part of these workspaces and depends on generated SDK crates,
# so it is tested by the dedicated `test-rust-windows-aws-config` CI job (see
# .github/workflows/ci.yml), which supplies the generated SDK and runs
# tools/ci-scripts/test-windows-aws-config.sh.
echo "Testing aws-smithy-http-client with Rustls/Ring and the wire harness"
(cd rust-runtime && cargo test -p aws-smithy-http-client --features rustls-ring,wire-mock) # only ring works on windows
