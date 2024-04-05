#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
set -eux

# The script populates an output file with key-value pairs that are needed in the release CI workflow to carry out
# the next steps in the release flow: the name of the release branch and a boolean flag that is set to 'true' if this
# is the beginning of a new release series.

if [ -z "$1" ]; then
  echo "You need to specify the path of the file where you want to collect the output"
  exit 1
else
  output_file="$1"
fi

branch_name="smithy-rs-release-1.x.y"

if [[ "${DRY_RUN}" == "true" ]]; then
  branch_name="${branch_name}-preview"
fi
echo "release_branch=${branch_name}" >"${output_file}"

commit_sha=$(git rev-parse --short HEAD)
# the git repo is in a weird state because **main has never been checked out**!
# This prevents the `git branch --contains` from working because there is no _local_ ref for main
git checkout main
git checkout "${commit_sha}"
if ! git ls-remote --exit-code --heads origin "${branch_name}"; then
  # The release branch does not exist.
  # We need to make sure that the commit SHA that we are releasing is on `main`.
  git fetch origin main
  echo "Branches: "
  git branch --contains "${commit_sha}"
  git show origin/main | head -n 1
  if git branch --contains "${commit_sha}" | grep main; then
    # We can then create the release branch and set the current commit as its tip
    if [[ "${DRY_RUN}" == "true" ]]; then
      git push --force origin "HEAD:refs/heads/${branch_name}"
    else
      git checkout -b "${branch_name}"
      git push origin "${branch_name}"
    fi
  else
    echo "You must choose a commit from main to create a new release branch!"
    exit 1
  fi
else
  echo "Patch release ${branch_name} already exists"
fi
