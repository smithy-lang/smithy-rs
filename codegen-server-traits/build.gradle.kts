/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    kotlin("jvm")
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "Smithy traits for Rust code generation"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Traits"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.traits"

dependencies {
    implementation(libs.smithy.model)
    testImplementation(libs.junit.jupiter)
}
