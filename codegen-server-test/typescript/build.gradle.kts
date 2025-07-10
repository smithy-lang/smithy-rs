/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

description = "Generates Rust/Typescript code from Smithy models and runs the protocol tests"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Server :: Typescript :: Test"
extra["moduleName"] = "software.amazon.smithy.rust.kotlin.codegen.server.typescript.test"

tasks["jar"].enabled = false

plugins {
    java
    alias(libs.plugins.smithy.gradle.base)
    alias(libs.plugins.smithy.gradle.jar)
}

val properties = PropertyRetriever(rootProject, project)
val buildDir = layout.buildDirectory.get().asFile

val pluginName = "rust-server-codegen-typescript"
val workingDirUnderBuildDir = "smithyprojections/codegen-server-test-typescript/"

configure<software.amazon.smithy.gradle.SmithyExtension> {
    outputDirectory = layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile
}

dependencies {
    implementation(project(":codegen-server:typescript"))
    implementation(libs.smithy.aws.protocol.tests)
    implementation(libs.smithy.protocol.test.traits)
    implementation(libs.smithy.aws.traits)
}

val allCodegenTests = "../../codegen-core/common-test-models".let { commonModels ->
    listOf(
        CodegenTest("com.amazonaws.simple#SimpleService", "simple", imports = listOf("$commonModels/simple.smithy")),
        CodegenTest("com.aws.example.ts#PokemonService", "pokemon-service-server-sdk"),
    )
}

project.registerGenerateSmithyBuildTask(rootProject, pluginName, allCodegenTests)
project.registerGenerateCargoWorkspaceTask(rootProject, pluginName, allCodegenTests, workingDirUnderBuildDir)
project.registerGenerateCargoConfigTomlTask(buildDir.resolve(workingDirUnderBuildDir))

tasks["smithyBuild"].dependsOn("generateSmithyBuild")
tasks["assemble"].finalizedBy("generateCargoWorkspace")

project.registerModifyMtimeTask()
project.registerCargoCommandsTasks(buildDir.resolve(workingDirUnderBuildDir))

tasks["test"].finalizedBy(cargoCommands(properties).map { it.toString })

tasks["clean"].doFirst { delete("smithy-build.json") }
