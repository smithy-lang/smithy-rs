/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

extra["displayName"] = "Smithy :: Rust :: AWS-SDK :: SRA Test"
extra["moduleName"] = "software.amazon.smithy.rust.awssdk.sra.test"

tasks["jar"].enabled = false

plugins {
    id("software.amazon.smithy")
}

val smithyVersion: String by project
val defaultRustDocFlags: String by project
val properties = PropertyRetriever(rootProject, project)

val pluginName = "rust-client-codegen"
val workingDirUnderBuildDir = "smithyprojections/sdk-sra-test/"

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
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
}

val allCodegenTests = listOf(
    CodegenTest(
        "com.amazonaws.dynamodb#DynamoDB_20120810",
        "aws-sdk-dynamodb",
        imports = listOf("../sdk/aws-models/dynamodb.json"),
        extraConfig = """
            ,
            "codegen": {
                "includeFluentClient": false,
                "enableNewSmithyRuntime": true
            },
            "customizationConfig": {
                "awsSdk": {
                    "generateReadme": false
                }
            }
        """,
    ),
    CodegenTest(
        "com.amazonaws.s3#AmazonS3",
        "aws-sdk-s3",
        imports = listOf("../sdk/aws-models/s3.json", "../sdk/aws-models/s3-tests.smithy"),
        extraConfig = """
            ,
            "codegen": {
                "includeFluentClient": false,
                "enableNewSmithyRuntime": true
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
