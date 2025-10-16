/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

plugins {
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "Smithy traits for Rust code generation"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Traits"
extra["moduleName"] = "software.amazon.smithy.framework.rust"

dependencies {
    implementation(libs.smithy.model)
    testImplementation(libs.junit.jupiter)
}
