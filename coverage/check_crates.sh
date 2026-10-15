#!/usr/bin/env bash
# Compilation alone cannot establish compatibility. Execute all corpus assertions.
set -euo pipefail
exec python3 "$(dirname "$0")/run_tests.py" "$@"
