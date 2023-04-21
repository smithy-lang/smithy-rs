#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

import polars as pl

def main():
    file = open("/tmp/compiletime-benchmark.txt", "r").read()
    stack = []
    idx = 0
    hashmap = {}
    for i in file.split("\n"):
        if idx > 4:
            idx = 0

        if idx == 0:
            hashmap["sdk"] = i
        elif idx == 1:
            hashmap["dev"] = i
        elif idx == 2:
            hashmap["release"] = i
        elif idx == 3:
            hashmap["dev-w-all-features"] = i
        elif idx == 4:
            hashmap["release-w-all-features"] = i

        idx+=1

    df = pl.DataFrame(stack).sort("sdk").select(pl.all().exclude("sdk").cast(pl.Float64))
    dev = df.filter(pl.col("sdk") == "s3").to_dicts()[0]["dev"]
    df = df.with_columns(pl.col(pl.Float64)/dev)
    print(df)
    df.to_pandas().to_markdown("./benchmark.md")

main()
