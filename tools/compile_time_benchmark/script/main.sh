#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

git clone https://github.com/awslabs/smithy-rs.git
./gradlew :codegen-client

cargo build
cargo build --release
