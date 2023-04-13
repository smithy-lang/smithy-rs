#!/usr/bin/env python3

#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

import os
import sys

from diff_lib import eprint, run, get_cmd_status, get_cmd_output, generate_and_commit_generated_code, make_diffs, \
    write_to_file, HEAD_BRANCH_NAME, BASE_BRANCH_NAME, OUTPUT_PATH, running_in_docker_build


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
# $ ../../smithy-rs/tools/ci-scripts/codegen-diff/codegen-diff-revisions.py . <some commit hash to diff against>
# ```
#
# It will diff the generated code from HEAD against any commit hash you feed it. If you want to test
# a specific range, change the HEAD of the test repository.
#
# This script requires `difftags` to be installed from `tools/ci-build/difftags`:
# ```
# $ cargo install --path tools/ci-build/difftags
# ```
# Make sure the local version matches the version referenced from the GitHub Actions workflow.



def main():
    if len(sys.argv) != 3:
        eprint("Usage: codegen-diff-revisions.py <repository root> <base commit sha>")
        sys.exit(1)

    repository_root = sys.argv[1]
    base_commit_sha = sys.argv[2]
    os.chdir(repository_root)
    (_, head_commit_sha, _) = get_cmd_output("git rev-parse HEAD")

    # Make sure the working tree is clean
    if get_cmd_status("git diff --quiet") != 0:
        eprint("working tree is not clean. aborting")
        sys.exit(1)

    if running_in_docker_build():
        eprint(f"Fetching base revision {base_commit_sha} from GitHub...")
        run(f"git fetch --no-tags --progress --no-recurse-submodules --depth=1 origin {base_commit_sha}")

    # Generate code for HEAD
    eprint(f"Creating temporary branch with generated code for the HEAD revision {head_commit_sha}")
    run(f"git checkout {head_commit_sha} -b {HEAD_BRANCH_NAME}")
    generate_and_commit_generated_code(head_commit_sha)

    # Generate code for base
    eprint(f"Creating temporary branch with generated code for the base revision {base_commit_sha}")
    run(f"git checkout {base_commit_sha} -b {BASE_BRANCH_NAME}")
    generate_and_commit_generated_code(base_commit_sha)

    bot_message = make_diffs(base_commit_sha, head_commit_sha)
    write_to_file(f"{OUTPUT_PATH}/bot-message", bot_message)

    # Clean-up that's only really useful when testing the script in local-dev
    if not running_in_docker_build():
        run("git checkout main")
        run(f"git branch -D {BASE_BRANCH_NAME}")
        run(f"git branch -D {HEAD_BRANCH_NAME}")


if __name__ == "__main__":
    main()
