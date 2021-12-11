#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

set -e

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 <sdk version>"
    exit 1
fi
SDK_VERSION=$1

cd `dirname $0`
./write-cargo-toml.py --sdk-version ${SDK_VERSION}
cargo build --release --target=x86_64-unknown-linux-musl

TARGET_PATH="$(git rev-parse --show-toplevel)/target/x86_64-unknown-linux-musl/release"
pushd "${TARGET_PATH}" &>/dev/null

BIN_NAME="bootstrap"
BIN_SHA1_HASH="$(sha1sum ${BIN_NAME} | awk '{ print $1; }')"
BUNDLE_NAME="canary-lambda-${BIN_SHA1_HASH}.zip"

zip --quiet "${BUNDLE_NAME}" "${BIN_NAME}"
popd &>/dev/null

# Output: the path to the bundle
echo "${TARGET_PATH}/${BUNDLE_NAME}"
