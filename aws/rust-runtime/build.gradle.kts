/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

description = "Rust Runtime"
plugins {
    kotlin("jvm")
}

group = "software.amazon.aws.rustruntime"

version = "0.0.3"

tasks.jar {
    from("./") {
        include("aws-inlineable/src/*.rs")
        include("aws-inlineable/Cargo.toml")
    }
}
