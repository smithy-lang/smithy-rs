/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

extra["displayName"] = "Smithy :: Rust :: AWS-SDK :: Ad-hoc Test"
extra["moduleName"] = "software.amazon.smithy.rust.awssdk.adhoc.test"

tasks["jar"].enabled = false

plugins {
    id("software.amazon.smithy")
}

val smithyVersion: String by project
val defaultRustDocFlags: String by project
val properties = PropertyRetriever(rootProject, project)

val pluginName = "rust-client-codegen"
val workingDirUnderBuildDir = "smithyprojections/sdk-adhoc-test/"

configure<software.amazon.smithy.gradle.SmithyExtension> {
    outputDirectory = file("$buildDir/$workingDirUnderBuildDir")
}

buildscript {
    val smithyVersion: String by project
    dependencies {
        classpath("software.amazon.smithy:smithy-cli:$smithyVersion")
    }
}

dependencies {
    implementation(project(":aws:sdk-codegen"))
    implementation("software.amazon.smithy:smithy-aws-protocol-tests:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
}

fun getSmithyRuntimeMode(): String = properties.get("smithy.runtime.mode") ?: "orchestrator"

val allCodegenTests = listOf(
    CodegenTest(
        "com.amazonaws.apigateway#BackplaneControlService",
        "apigateway",
        imports = listOf("models/apigateway-rules.smithy"),
        extraConfig = """
            ,
            "codegen": {
                "includeFluentClient": false,
                "enableNewSmithyRuntime": "${getSmithyRuntimeMode()}"
            },
            "customizationConfig": {
                "awsSdk": {
                    "generateReadme": false
                }
            }
        """,
    ),
    CodegenTest(
        "com.amazonaws.testservice#TestService",
        "endpoint-test-service",
        imports = listOf("models/single-static-endpoint.smithy"),
        extraConfig = """
            ,
            "codegen": {
                "includeFluentClient": false,
                "enableNewSmithyRuntime": "${getSmithyRuntimeMode()}"
            },
            "customizationConfig": {
                "awsSdk": {
                    "generateReadme": false
                }
            }
        """,
    ),
)

project.registerGenerateSmithyBuildTask(rootProject, pluginName, allCodegenTests)
project.registerGenerateCargoWorkspaceTask(rootProject, pluginName, allCodegenTests, workingDirUnderBuildDir)
project.registerGenerateCargoConfigTomlTask(buildDir.resolve(workingDirUnderBuildDir))

tasks["smithyBuildJar"].dependsOn("generateSmithyBuild")
tasks["assemble"].finalizedBy("generateCargoWorkspace")

project.registerModifyMtimeTask()
project.registerCargoCommandsTasks(buildDir.resolve(workingDirUnderBuildDir), defaultRustDocFlags)

tasks["test"].finalizedBy(cargoCommands(properties).map { it.toString })

tasks["clean"].doFirst { delete("smithy-build.json") }
