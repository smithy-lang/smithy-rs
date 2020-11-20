#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

set -e
export SMITHY_TEST_WORKSPACE=~/.smithy-test-workspace
./gradlew test
./gradlew ktlintFormat
./gradlew ktlint

rust-runtime/test.sh
