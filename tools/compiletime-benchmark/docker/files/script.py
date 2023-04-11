#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

import polars as pl

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
    df.write_csv(file.replace(".txt", ".csv"))


main("unoptimized.txt")
main("optimized.txt")