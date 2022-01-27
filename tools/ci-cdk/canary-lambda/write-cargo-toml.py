#!/usr/bin/env python3
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#

# Generates a Cargo.toml with the given AWS SDK version for this canary

import argparse
import sys

BASE_MANIFEST = """
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0.
#
# IMPORTANT: Don't edit this file directly! Run `write-cargo-toml.py` to modify this file instead.
[package]
name = "aws-sdk-rust-lambda-canary"
version = "0.1.0"
edition = "2018"
license = "Apache-2.0"

# Emit an empty workspace so that the canary can successfully build when
# built from the aws-sdk-rust repo, which has a workspace in it.
[workspace]

[[bin]]
name = "bootstrap"
path = "src/main.rs"

[dependencies]
anyhow = "1"
async-stream = "0.3"
bytes = "1"
hound = "3.4"
async-trait = "0.1"
lambda_runtime = "0.4"
serde_json = "1"
thiserror = "1"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["fmt", "env-filter"] }
uuid = { version = "0.8", features = ["v4"] }
tokio-stream = "0"
tracing-texray = "0.1.1"
"""

notable_versions = [
    # first version to add support for paginators
    "0.4.1"
]

def main():
    args = Args()

    with open("Cargo.toml", "w") as file:
        print(BASE_MANIFEST, file=file)
        print(format_dependency("aws-config",
              args.path, args.sdk_version), file=file)
        print(format_dependency("aws-sdk-s3",
              args.path, args.sdk_version), file=file)
        print(format_dependency("aws-sdk-ec2",
                                args.path, args.sdk_version), file=file)
        print(format_dependency("aws-sdk-transcribestreaming",
              args.path, args.sdk_version), file=file)
        print("[features]", file=file)
        for version in notable_versions:
            print(f'"v{version}" = []', file=file)
        enabled = ', '.join(enabled_versions(args.sdk_version))
        print(f'default = [{enabled}]', file=file)

def enabled_versions(sdk_version):
    if sdk_version is None:
        return [f'"v{version}"' for version in notable_versions]
    else:
        return [f'"v{version}"' for version in notable_versions if version.split('.') <= sdk_version.split('.')]

def format_dependency(crate, path, version):
    if path is None:
        return f'{crate} = "{version}"'
    else:
        crate_path = crate
        if crate_path.startswith("aws-sdk-"):
            crate_path = crate_path[(len("aws-sdk-")):]
        if version is None:
            return f'{crate} = {{ path = "{path}/{crate_path}" }}'
        else:
            return f'{crate} = {{ path = "{path}/{crate_path}", version = "{version}" }}'


class Args:
    def __init__(self):
        parser = argparse.ArgumentParser()
        parser.add_argument("--path", dest="path", type=str,
                            help="Path to the generated AWS Rust SDK")
        parser.add_argument("--sdk-version", dest="sdk_version",
                            type=str, help="AWS Rust SDK version")

        args = parser.parse_args()
        self.path = args.path
        self.sdk_version = args.sdk_version
        if self.path == None and self.sdk_version == None:
            print("Either of path or sdk-version are required")
            sys.exit(1)


if __name__ == "__main__":
    main()
