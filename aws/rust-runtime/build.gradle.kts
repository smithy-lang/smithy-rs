/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

plugins {
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "AWS SDK Rust Runtime"
extra["displayName"] = "Smithy :: Rust :: AWS SDK Rust Runtime"
extra["moduleName"] = "software.amazon.smithy.rust.aws.runtime"

tasks.jar {
    from("./") {
        include("aws-inlineable/src/*.rs")
        include("aws-inlineable/Cargo.toml")
    }
}
