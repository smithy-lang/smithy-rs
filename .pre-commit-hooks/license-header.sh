#!/bin/bash
set -e

#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

cd "$(git rev-parse --show-toplevel)/tools/ci-build/sdk-lints" && cargo run -- check --license --changelog
