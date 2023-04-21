#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#
def main():
    file = open("/tmp/compiletime-benchmark.txt", "r").read()
    stack = []
    idx = 0
    hashmap = {}
    for i in file.split("\n"):
        idx += 1
        if idx > 5:
            idx = 1

        if idx == 1:
            hashmap["sdk"] = i
            continue

        i = float(i)
        if idx == 2:
            hashmap["dev"] = i
        elif idx == 3:
            hashmap["release"] = i
        elif idx == 4:
            hashmap["dev-w-all-features"] = i
        elif idx == 5:
            hashmap["release-w-all-features"] = i
            stack.append(hashmap)
            hashmap = {}

    header = "|".join(stack[0].keys())
    header = f"|{header}|"
    table = [header]
    for hashmap in stack:
        row = []
        for i in hashmap.keys():
            row.append(str(hashmap[i]))
        inner = "|".join(row)
        inner = f"|{inner}|"
        table.append(inner)

    markdown = "\n".join(table)
    print(markdown)
    # send markdown to somewhere

main()
