#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

set -e

# TTY colors
C_CYAN='\033[1;36m'
C_YELLOW='\033[1;33m'
C_RESET='\033[0m'

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 [crate workspace path(s)...]"
    echo "Recursively visits 'additional-ci' scripts in the given directories and executes them."
    exit 1
fi

for area in "$@"; do
    pushd "${area}" &>/dev/null
    echo -e "${C_CYAN}Scanning '${area}'...${C_RESET}"

    find . -name 'additional-ci' -print0 | while read -d $'\0' file; do
        echo
        echo -e "${C_YELLOW}Running additional checks script '${file}'...${C_RESET}"
        echo
        pushd "$(dirname ${file})" &>/dev/null
        ./additional-ci
        popd &>/dev/null
    done

    echo
    popd &>/dev/null
done
