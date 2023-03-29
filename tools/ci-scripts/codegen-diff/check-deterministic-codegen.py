#!/usr/bin/env python3

#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

import sys
import os
from diff_lib import get_cmd_output, generate_and_commit_generated_code

def main():
    repository_root = sys.argv[1]
    os.chdir(repository_root)
    (_, head_commit_sha, _) = get_cmd_output("git rev-parse HEAD")
    get_cmd_output("git checkout -B once")
    generate_and_commit_generated_code(head_commit_sha, targets=['aws:sdk'])
    get_cmd_output(f"git checkout {head_commit_sha}")
    get_cmd_output("git checkout -B twice")
    generate_and_commit_generated_code(head_commit_sha, targets=['aws:sdk'])
    get_cmd_output('git diff once..twice --exit-code')


if __name__ == "__main__":
    main()
