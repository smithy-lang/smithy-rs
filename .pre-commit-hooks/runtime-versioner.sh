#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

set -e
# Clear the git environment that hooks inherit, since the tool runs git in other directories
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE
cd "$(git rev-parse --show-toplevel)/tools/ci-build/runtime-versioner" && cargo run -- audit
