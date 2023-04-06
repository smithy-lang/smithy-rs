#!/usr/bin/env python3

#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0

import os
import sys

from diff_lib import get_cmd_output, checkout_commit_and_generate


def main():
    repository_root = sys.argv[1]
    os.chdir(repository_root)
    (_, head_commit_sha, _) = get_cmd_output("git rev-parse HEAD")
    checkout_commit_and_generate(head_commit_sha, targets=['aws:sdk'], branch_name='once')
    checkout_commit_and_generate(head_commit_sha, targets=['aws:sdk'], branch_name='twice')
    get_cmd_output('git diff once..twice --exit-code')


if __name__ == "__main__":
    main()
