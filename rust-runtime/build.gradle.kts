/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

description = "Rust Runtime"
plugins {
    kotlin("jvm")
}

group = "software.amazon.rustruntime"

version = "0.0.3"

tasks.jar {
    from("./") {
        include("inlineable/src/*.rs")
        include("inlineable/Cargo.toml")
    }
}
