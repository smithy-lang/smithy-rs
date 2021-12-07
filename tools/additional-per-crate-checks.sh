#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 <runtime crate workspace path>"
    exit 1
fi

cd $1

set -e

for path in *; do
    if [[ -d "${path}" && -f "${path}/Cargo.toml" && -f "${path}/additional-ci" ]]; then
        echo
        echo "# Running additional checks for ${path}..."
        echo
        pushd "${path}"
        ./additional-ci
        popd
    fi
done
