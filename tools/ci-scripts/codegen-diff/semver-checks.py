#!/usr/bin/env python3

#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0
import sys
import os
from diff_lib import get_cmd_output, get_cmd_status, eprint, running_in_docker_build, checkout_commit_and_generate, \
    OUTPUT_PATH


CURRENT_BRANCH = 'current'
BASE_BRANCH = 'base'
# This script runs `cargo semver-checks` against a previous version of codegen
def main(skip_generation=False):
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

    if not skip_generation:
        checkout_commit_and_generate(head_commit_sha, CURRENT_BRANCH, targets=['aws:sdk'])
        checkout_commit_and_generate(base_commit_sha, BASE_BRANCH, targets=['aws:sdk'])
    get_cmd_output(f'git checkout {CURRENT_BRANCH}')
    sdk_directory = os.path.join(OUTPUT_PATH, 'aws-sdk', 'sdk')
    os.chdir(sdk_directory)

    failures = []
    deny_list = [
        # add crate names here to exclude them from the semver checks
    ]
    for path in os.listdir():
        eprint(f'checking {path}...', end='')
        if path in deny_list:
            eprint(f"skipping {path} because it is in 'deny_list'")
        elif get_cmd_status(f'git cat-file -e base:{sdk_directory}/{path}/Cargo.toml') != 0:
            eprint(f'skipping {path} because it does not exist in base')
        else:
            (_, out, _) = get_cmd_output('cargo pkgid', cwd=path, quiet=True)
            pkgid = parse_package_id(out)
            (status, out, err) = get_cmd_output(f'cargo semver-checks check-release '
                                    f'--baseline-rev {BASE_BRANCH} '
                                    # in order to get semver-checks to work with publish-false crates, need to specify
                                    # package and manifest path explicitly
                                    f'--manifest-path {path}/Cargo.toml '
                                    '-v '
                                    f'-p {pkgid} '
                                    f'--all-features '
                                    f'--release-type minor', check=False, quiet=True)
            if status == 0:
                eprint('ok!')
            else:
                failures.append(f"{out}{err}")
                eprint('failed!')
                if out:
                    eprint(out)
                eprint(err)
    if failures:
        eprint('One or more crates failed semver checks!')
        eprint("\n".join(failures))
        exit(1)


def parse_package_id(id):
    if '#' in id and '@' in id:
        return id.split('#')[1].split('@')[0]
    elif '#' in id:
        return id.split('/')[-1].split('#')[0]
    else:
        eprint(id)
        raise Exception("unknown format")


import unittest


class SelfTest(unittest.TestCase):
    def test_foo(self):
        self.assertEqual(parse_package_id("file:///Users/rcoh/code/smithy-rs-ci/smithy-rs/tmp-codegen-diff/aws-sdk/sdk/aws-smithy-runtime-api#0.56.1"), "aws-smithy-runtime-api")
        self.assertEqual(parse_package_id("file:///Users/rcoh/code/smithy-rs-ci/smithy-rs/tmp-codegen-diff/aws-sdk/sdk/s3#aws-sdk-s3@0.0.0-local"), "aws-sdk-s3")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        sys.argv.pop()
        unittest.main()
    else:
        skip_generation = bool(os.environ.get('SKIP_GENERATION') or False)
        main(skip_generation=skip_generation)
