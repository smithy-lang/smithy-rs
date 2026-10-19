#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

# Compilation alone cannot establish compatibility. Execute all corpus assertions.
set -euo pipefail
exec python3 "$(dirname "$0")/run_tests.py" "$@"
