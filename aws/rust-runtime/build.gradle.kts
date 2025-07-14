/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

plugins {
    kotlin("jvm")
}

description = "AWS SDK Rust Runtime"

tasks.jar {
    from("./") {
        include("aws-inlineable/src/*.rs")
        include("aws-inlineable/Cargo.toml")
    }
}
