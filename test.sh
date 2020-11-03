#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

set -e
./gradlew test
./gradlew ktlintFormat
./gradlew ktlint