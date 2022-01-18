#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

set -e

# TTY colors
C_CYAN='\033[1;36m'
C_YELLOW='\033[1;33m'
C_RESET='\033[0m'

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 [crate workspace path(s)...]"
    echo "Visits all immediate sub-directories of the given workspace path,"
    echo "looks for Cargo.toml files and additional-ci scripts. If both are"
    echo "found, it executes the additional-ci script in that directory."
    exit 1
fi

for workspace in "$@"; do
    pushd "${workspace}" &>/dev/null
    echo -e "${C_CYAN}Scanning ${workspace}...${C_RESET}"

    for path in *; do
        if [[ -d "${path}" && -f "${path}/Cargo.toml" && -f "${path}/additional-ci" ]]; then
            echo
            echo -e "${C_YELLOW}Running additional checks for ${path}...${C_RESET}"
            echo
            pushd "${path}" &>/dev/null
            ./additional-ci
            popd &>/dev/null
        fi
    done

    echo
    popd &>/dev/null
done
