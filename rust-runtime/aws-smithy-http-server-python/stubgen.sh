#!/usr/bin/env bash
#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

set -x

if [ $# -lt 3 ]; then
    echo "usage: $0 package manifest_path output_directory"
    exit 1
fi

# input arguments
package=$1
manifest=$2
output=$3

# the directory of the script
source_dir="$(git rev-parse --show-toplevel)"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -n "$source_dir" ]; then
    CARGO_TARGET_DIR="$source_dir/target"
else
    CARGO_TARGET_DIR=$(mktemp -d)
    mkdir -p "$CARGO_TARGET_DIR"
    # cleanup temporary directory
    function cleanup {
        # shellcheck disable=2317
        rm -rf "$CARGO_TARGET_DIR"
    }
    # register the cleanup function to be called on the EXIT signal
    trap cleanup EXIT
fi
export CARGO_TARGET_DIR

# generate the Python stubs
cargo build --manifest-path "$manifest"
ln -sf "$CARGO_TARGET_DIR/debug/lib$package.so" "$CARGO_TARGET_DIR/debug/$package.so"
PYTHONPATH=$CARGO_TARGET_DIR/debug:$PYTHONPATH python "$script_dir/stubgen.py" "$package" "$output"

exit 0
