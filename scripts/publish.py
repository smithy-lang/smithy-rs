#!/usr/bin/env python
#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0.

import os
import shlex
import subprocess
import sys
import time


def confirm(prompt):
    return input(f"{prompt} OK? [Y/N]? ").lower() == "y"


def main():
    with open('complete') as f:
        complete = set(f.read().splitlines())
    crates = os.listdir('build')
    confirm(f'Found {len(crates)} crates.')
    print(f'Already done: {len(complete)}')
    for_real = len(sys.argv) == 2 and sys.argv[1] == '--no-dry-run'
    flag = '' if for_real else '--dry-run'
    for crate in crates:
        if crate in complete:
            print(f'Skipping {crate} already done')
            continue
        command = shlex.split(f'cargo publish {flag}')
        confirm(command)
        subprocess.run(command, cwd=f'build/{crate}', check=True)
        subprocess.run(shlex.split('cargo owner --add github:awslabs:rust-sdk-owners'), check=True)
        print('sleeping for 2 minutes...')
        with open('complete', 'a') as f:
            print(crate, file=f)
        time.sleep(120)


if __name__ == "__main__":
    main()
