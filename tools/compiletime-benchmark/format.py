#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

import itertools

def main():
    markdown = parser()
    print(markdown)
    # write file
    with open("/tmp/compiletime-benchmark.md", "w") as f:
        f.write(markdown)
        f.flush()
        f.close()


def parser() -> str:
    # read file
    f = open("/tmp/compiletime-benchmark.txt", "r").read()
    iter = map(lambda x: x.split("END"), f.split("START"))
    iter = itertools.chain.from_iterable(iter)

    # I could've used a dataframe like pandas but this works.
    markdown = """
    | sdk name | dev | release | dev all features | release all features |
    | -------- | --- | ------- | ---------------- | -------------------- |
    """

    for i in iter:
        outputs = []
        print(i)
        for l in i.splitlines():
            if not "+" in l:
                outputs.append(l.replace("real", "").replace(" ", ""))

        if len(outputs) != 6:
            continue
        
        outputs = outputs[1:]
        sdk_name = outputs[0]
        row = f"|{sdk_name}|" + \
            "|".join(outputs[1:]) + "|"

        markdown += row

    return markdown


if __name__ == '__main__':
    main()
