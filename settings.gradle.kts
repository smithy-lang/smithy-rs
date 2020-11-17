/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

pluginManagement {
    repositories {
        maven("https://dl.bintray.com/kotlin/kotlin-eap")
        mavenCentral()
        maven("https://plugins.gradle.org/m2/")
        google()
        gradlePluginPortal()
    }
}


rootProject.name = "software.amazon.smithy.rust.codegen.smithy-rs"
enableFeaturePreview("GRADLE_METADATA")

include(":codegen")
include(":codegen-test")
//include(":software.amazon.smithy.rust.codegen.smithy-kotlin-codegen-test")

/*
include(":client-runtime")
include(":client-runtime:testing")
include(":client-runtime:software.amazon.smithy.rust.codegen.smithy-test")
include(":client-runtime:client-rt-core")
include(":client-runtime:utils")
include(":client-runtime:io")
include(":client-runtime:protocol:http")
include(":client-runtime:serde")
include(":client-runtime:serde:serde-json")
include(":client-runtime:serde:serde-xml")
include(":client-runtime:serde:serde-test")

include(":client-runtime:protocol:http-client-engines:http-client-engine-ktor")

// for now include the POC project
include(":example")
project(":example").projectDir = file("./design/example")
include(":example:mock-server")
include(":example:lambda-example")
include(":example:s3-example")
*/
