#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

set -e
# Used by codegen-test/smithy-build.json
declare REPO_ROOT
REPO_ROOT="$(git rev-parse --show-toplevel)"
export REPO_ROOT
./gradlew test
./gradlew ktlintFormat
./gradlew ktlint