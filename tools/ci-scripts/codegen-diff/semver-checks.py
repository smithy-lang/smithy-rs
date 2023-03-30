#!/usr/bin/env python3

#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0
import sys
import os
from diff_lib import get_cmd_output, get_cmd_status, eprint, running_in_docker_build, checkout_commit_and_generate, \
    OUTPUT_PATH


# This script runs `cargo semver-checks` against a previous version of codegen
def main():
    if len(sys.argv) != 3:
        eprint("Usage: semver-checks.py <repository root> <base commit sha>")
        sys.exit(1)

    repository_root = sys.argv[1]
    base_commit_sha = sys.argv[2]
    os.chdir(repository_root)
    (_, head_commit_sha, _) = get_cmd_output("git rev-parse HEAD")

    # Make sure the working tree is clean
    if get_cmd_status("git diff --quiet") != 0:
        eprint("working tree is not clean. aborting")
        sys.exit(1)

    checkout_commit_and_generate(head_commit_sha, 'current', targets=['aws:sdk'])
    checkout_commit_and_generate(base_commit_sha, 'base', targets=['aws:sdk'])
    get_cmd_output('git checkout current')
    sdk_directory = os.path.join(OUTPUT_PATH, 'aws-sdk', 'sdk')
    os.chdir(sdk_directory)

    for path in os.listdir():
        eprint(f'checking {path}...', end='')
        if get_cmd_status(f'git cat-file -e base:{sdk_directory}/{path}/Cargo.toml') == 0:
            get_cmd_output(
                f'cargo semver-checks check-release --baseline-rev {base_commit_sha} --manifest-path {path}/Cargo.toml -p {path} --release-type patch')
            eprint('ok!')
        else:
            eprint(f'skipping {path} because it does not exist in base')


if __name__ == "__main__":
    main()
