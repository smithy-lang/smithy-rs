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
	gh_branch=${GITHUB_HEAD_REF##*/}
	base_branch=${GITHUB_BASE_REF##*/}
	if [ -n "$base_branch" ]; then
	  base_branch=__generated-$base_branch
  else
    base_branch=__generated__
	fi
	echo "Loaded branch from GitHub: $gh_branch ($GITHUB_HEAD_REF). Base branch: $base_branch"
	current_branch="${gh_branch:-$(git rev-parse --abbrev-ref HEAD)}"
	echo "Current branch resolved to: $current_branch"
	gen_branch="__generated-$current_branch"
	repo_root=$(git rev-parse --show-toplevel)
	cd "$repo_root" && ./gradlew :aws:sdk:assemble
	target="$(mktemp -d)"
	mv "$repo_root"/aws/sdk/build/aws-sdk "$target"
	# checkout and reset $gen_branch to be based on the __generated__ history
	git fetch origin "$base_branch"
	git checkout -B "$gen_branch" origin/"$base_branch"
	cd "$repo_root" && git rm -rf .
	rm -rf "$repo_root/aws-sdk"
	mv "$target"/aws-sdk "$repo_root"/.
	git add "$repo_root"/aws-sdk
	PRE_COMMIT_ALLOW_NO_CONFIG=1 git \
  	-c "user.name=GitHub Action (generated code preview)" \
	  -c "user.email=generated-code-action@github.com" \
	  commit -m "Generated code for $current_branch" --allow-empty
} >&2
