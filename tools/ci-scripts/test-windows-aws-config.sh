#!/usr/bin/env bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

set -eu -o pipefail

# `aws-config` is not part of the `aws/rust-runtime` workspace and depends on
# generated SDK crates (STS, SSO, ...), so it cannot be tested by
# `test-windows.sh` alongside the other runtime crates. This script tests it
# separately on Windows.
#
# The generated SDK must already be extracted to `aws/sdk/build/aws-sdk`. In CI
# the `test-rust-windows-aws-config` job downloads the `generate-aws-sdk-smoketest`
# artifact (produced by the `generate` job) and puts it there before running this
# script, mirroring how the Linux `check-aws-config` job obtains the SDK.
#
# We deliberately build with `--no-default-features --features
# credentials-process,rt-tokio` instead of `--all-features`: the default HTTPS
# client pulls in `rustls`/`aws-lc-rs`, whose `aws-lc-sys` C build requires NASM
# and does not build on the `windows-latest` runner. This is the same reason
# `test-windows.sh` tests `aws-smithy-http-client` with `rustls-ring` only. This
# feature set pulls in neither `aws-lc-sys` nor `openssl-sys`, and it still
# exercises the `credential_process` provider, which is where the Windows-specific
# `cmd.exe` handling lives (see the `credential_process` Windows integration
# tests).

if [[ ! -d "aws/sdk/build/aws-sdk/sdk" ]]; then
  echo "error: generated SDK not found at 'aws/sdk/build/aws-sdk'." >&2
  echo "Extract the 'generate-aws-sdk-smoketest' artifact there before running this script." >&2
  exit 1
fi

FEATURES="credentials-process,rt-tokio"

echo "Testing aws-config on Windows (--no-default-features --features ${FEATURES})"
pushd "aws/rust-runtime/aws-config" &>/dev/null
cargo clippy --no-default-features --features "${FEATURES}"
cargo test --no-default-features --features "${FEATURES}"
popd &>/dev/null
