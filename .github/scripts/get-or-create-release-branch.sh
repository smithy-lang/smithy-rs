#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
set -eux

# Compute the name of the release branch starting from the version that needs to be released ($SEMANTIC_VERSION).
# If it's the beginning of a new release series, the branch is created and pushed to the remote (chosen according to
# the value $DRY_RUN).
#
# The script populates an output file with key-value pairs that are needed in the release CI workflow to carry out
# the next steps in the release flow: the name of the release branch and a boolean flag that is set to 'true' if this
# is the beginning of a new release series.

if [ -z "$SEMANTIC_VERSION" ]; then
    echo "'SEMANTIC_VERSION' must be populated."
    exit 1
fi

if [ -z "$1" ]; then
    echo "You need to specify the path of the file where you want to collect the output"
    exit 1
else
    output_file="$1"
fi

# Split on the dots
version_array=(${SEMANTIC_VERSION//./ })
major=${version_array[0]}
minor=${version_array[1]}
patch=${version_array[2]}
if [[ "${major}" == "" || "${minor}" == "" || "${patch}" == "" ]]; then
    echo "'${SEMANTIC_VERSION}' is not a valid semver tag"
    exit 1
fi
if [[ $major == 0 ]]; then
    branch_name="smithy-rs-release-${major}.${minor}.x"
    if [[ $patch == 0 ]]; then
        echo "new_release_series=true" >"${output_file}"
    fi
else
    branch_name="smithy-rs-release-${major}.x.y"
    if [[ $minor == 0 && $patch == 0 ]]; then
        echo "new_release_series=true" >"${output_file}"
    fi
fi

if [[ "${DRY_RUN}" == "true" ]]; then
    branch_name="${branch_name}-preview"
fi
echo "release_branch=${branch_name}" >"${output_file}"

if [[ "${DRY_RUN}" == "true" ]]; then
    git push --force origin "HEAD:refs/heads/${branch_name}"
else
    commit_sha=$(git rev-parse --short HEAD)
    if ! git ls-remote --exit-code --heads origin "${branch_name}"; then
        # The release branch does not exist.
        # We need to make sure that the commit SHA that we are releasing is on `main`.
        git fetch origin main
        if git branch --contains "${commit_sha}" | grep main; then
            # We can then create the release branch and set the current commit as its tip
            git checkout -b "${branch_name}"
            git push origin "${branch_name}"
        else
            echo "You must choose a commit from main to create a new release branch!"
            exit 1
        fi
    fi
fi
