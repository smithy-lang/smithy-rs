#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

# This script can be run and tested locally. To do so, you should check out
# a second smithy-rs repository so that you can work on the script and still
# run it without it immediately bailing for an unclean working tree.
#
# Example:
# `smithy-rs/` - the main repo you're working out of
# `test/smithy-rs/` - the repo you're testing against
#
# ```
# $ cd test/smithy-rs
# $ ../../smithy-rs/tools/codegen-diff-revisions.sh . <some commit hash to diff against>
# ```
#
# It will diff the generated code from HEAD against any commit hash you feed it. If you want to test
# a specific range, change the HEAD of the test repository.
#
# This script requires `diff2html-cli` to be installed from NPM:
# ```
# $ npm install -g diff2html-cli@5.1.11
# ```
# Make sure the local version matches the version referenced from the GitHub Actions workflow.

set -e

generate_and_commit_generated_code() {
    ./gradlew :aws:sdk:assemble
    ./gradlew :codegen-server-test:assemble

    # Move generated code into codegen-diff/ directory
    rm -rf codegen-diff/
    mkdir codegen-diff
    mv aws/sdk/build/aws-sdk codegen-diff/
    mv codegen-server-test/build/smithyprojections/codegen-server-test codegen-diff/

    # Cleanup the server-test folder
    rm -rf "codegen-diff/codegen-server-test/source"
    find "codegen-diff/codegen-server-test" | grep -E "smithy-build-info.json|sources/manifest|model.json" | xargs rm -f

    git add -f codegen-diff
    git -c "user.name=GitHub Action (generated code preview)" \
        -c "user.email=generated-code-action@github.com" \
        commit --no-verify -m "Generated code for $(git rev-parse HEAD)" --allow-empty
}

# Redirect stdout to stderr for all the code in `{ .. }`
{
    if [ "$#" -ne 2 ]; then
        echo "Usage: codegen-diff-revisions.sh <repository root> <base commit sha>"
        exit 1
    fi

    REPOSITORY_ROOT="$1"
    BASE_COMMIT_SHA="$2"
    HEAD_COMMIT_SHA=$(git rev-parse HEAD)
    cd "${REPOSITORY_ROOT}"

    git diff --quiet || (echo 'working tree not clean, aborting' && exit 1)

    echo "Fetching base revision ${BASE_COMMIT_SHA} from GitHub..."
    git fetch --no-tags --progress --no-recurse-submodules --depth=1 origin ${BASE_COMMIT_SHA}

    echo "Creating temporary branch with generated code for the HEAD revision ${HEAD_COMMIT_SHA}..."
    git checkout ${HEAD_COMMIT_SHA} -b __tmp-localonly-head
    generate_and_commit_generated_code

    echo "Creating temporary branch with generated code for the base revision..."
    git checkout ${BASE_COMMIT_SHA} -b __tmp-localonly-base
    generate_and_commit_generated_code

    # Generate HTML diff. This uses the diff2html-cli, which defers to `git diff` under the hood.
    # All arguments after the first `--` go to the `git diff` command.
    diff2html -s line -f html -d word -i command -F "diff-${BASE_COMMIT_SHA}-${HEAD_COMMIT_SHA}.html" -- \
        __tmp-localonly-base __tmp-localonly-head -- codegen-diff

    # Clean-up that's only really useful when testing the script in local-dev
    if [ "${GITHUB_ACTIONS}" -ne "true" ]; then
        git checkout main
        git branch -D __tmp-localonly-base
        git branch -D __tmp-localonly-head
    fi
} >&2
