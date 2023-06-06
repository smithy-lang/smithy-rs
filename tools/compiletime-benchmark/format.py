#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

import itertools


def main():
    file = open("/tmp/compiletime-benchmark.txt", "r").read()
    iter = map(lambda x: x.split("END"), file.split("START"))
    iter = itertools.chain.from_iterable(iter)
    markdown = """
    | sdk name | dev | release | dev all features | release all features |
    | -------- | --- | ------- | ---------------- | -------------------- |
    """
    idx = 0
    for i in iter:
        idx += 1
        print("============")
        print(idx)
        print(i)
        print("============")
        
        lines = i.splitlines()
        if len(lines) > 1:
            continue

        sdk_name = lines[0]
        row = "\n|" + sdk_name + \
            "|".join(map(lambda x: float(x), lines[1:])) + "|"

        markdown += row
        
    print(markdown)
    # send markdown to somewhere


main()
