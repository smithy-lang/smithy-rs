#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
set -e

# Split on the dots
version_array=( ${SEMANTIC_VERSION//./ } )
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
    echo "new_release_series=true" > $GITHUB_OUTPUT
  fi
else
  branch_name="smithy-rs-release-${major}.x.y"
  if [[ $minor == 0 && $patch == 0 ]]; then
    echo "new_release_series=true" > $GITHUB_OUTPUT
  fi
fi

if [[ "${DRY_RUN}" == "true" ]]; then
  branch_name="${branch_name}-preview"
fi
echo "release_branch=${branch_name}" > $GITHUB_OUTPUT

if [[ "${DRY_RUN}" == "true" ]]; then
  git push -f origin "HEAD:${branch_name}"
else
  commit_sha=$(git rev-parse --verify --short HEAD)
  if git ls-remote --exit-code --heads origin "${branch_name}"; then
    # The release branch already exists, we need to make sure that our commit is its current tip
    branch_head_sha=$(git rev-parse --verify --short refs/heads/patches)
    if [[ "${branch_head_sha}" != "${commit_sha}" ]]; then
      echo "The release branch - ${branch_name} - already exists. ${commit_sha}, the commit you chose when "
      echo "launching this release, is not its current HEAD (${branch_head_sha}). This is not allowed: you "
      echo "MUST release from the HEAD of the release branch if it already exists."
      exit 1
    fi
  else
    # The release branch does not exist.
    # We need to make sure that the commit SHA that we are releasing is on `main`.
    git fetch origin main
    if git branch --contains "${commit_sha}" | grep main; then
      # We can then create create the release branch and set the current commit as its tip
      git checkout -b "${branch_name}"
      git push origin "${branch_name}"
    else
      echo "You must choose a commit from main to create a new release series!"
      exit 1
    fi
  fi
fi
