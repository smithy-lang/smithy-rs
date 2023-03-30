#!/usr/bin/env python3
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# converts time's stdout to markdown
import polars as pl
import subprocess

DELIMITER="======================================="
def main(file: str):
    fp = open(file, "r")
    contents = fp.read()

    stack = []
    for i in contents.split(DELIMITER):
        hashmap = {}
        for line in i.splitlines():
            [key, val] = list(filter(lambda x: x != "", line.split(" ")))
            hashmap[key] = val

        stack.append(hashmap)

    df = pl.DataFrame(stack)
    # converts it to markdown file
    df.to_pandas().to_markdown(file.replace(".txt", ".md"))


main("unoptimized.txt")
main("optimized.txt")