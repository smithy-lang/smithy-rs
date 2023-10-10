#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

set -e
cd "$(git rev-parse --show-toplevel)/tools/ci-build/sdk-lints" && cargo run -- check --all
