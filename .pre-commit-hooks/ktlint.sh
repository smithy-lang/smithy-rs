#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

set -e

cd "$(git rev-parse --show-toplevel)"
# `-q`: run gradle in quiet mode
# `--console plain`: Turn off the fancy terminal printing in gradle
# `2>/dev/null`: Suppress the build success/failure output at the end since pre-commit will report failures
./gradlew -q --console plain ktlintPreCommit -DktlintPreCommitArgs="$*" 2>/dev/null
