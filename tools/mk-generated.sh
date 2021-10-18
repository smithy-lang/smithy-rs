#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

# Create a branch containing exclusively generated code
#
# This script creates a new branch `__generated-$current_branch` that contains the results of the smoke-test codegen
# This script outputs the name of the new branch to stdout

set -e

# Redirect stdout to stderr for all the code in `{ .. }`
{
	git diff --quiet || (echo 'working tree not clean, aborting' && exit 1)
	current_branch="$(git rev-parse --abbrev-ref HEAD)"
	gen_branch="__generated-$current_branch"
	git branch -D "$gen_branch" 1>&2
	repo_root=$(git rev-parse --show-toplevel)
	cd "$repo_root" && ./gradlew :aws:sdk:assemble
	target="$(mktemp -d)"
	mv "$repo_root"/aws/sdk/build/aws-sdk "$target"
	git checkout --orphan "$gen_branch"
	cd "$repo_root" && git rm -rf .
	rm -rf "$repo_root/aws-sdk"
	mv "$target"/aws-sdk "$repo_root"/.
	git add "$repo_root"/aws-sdk
	PRE_COMMIT_ALLOW_NO_CONFIG=1 git -c "user.name=GitHub Action (generated code preview)" -c "user.email=generated-code-action@github.com" commit -m "Generated code for $current_branch"
	git checkout "$current_branch"
} >&2

# Output the name of the generated branch
echo "$gen_branch"
