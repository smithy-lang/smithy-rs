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

val publisherToolPath = rootProject.projectDir.resolve("tools/ci-build/publisher")
val outputDir = buildDir.resolve("sdk")

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

data class Service(
    val serviceId: String,
    val moduleName: String,
    val imports: List<String>,
)
val servicesToGenerate = listOf(
    Service(
        "com.amazonaws.dynamodb#DynamoDB_20120810",
        "aws-sdk-dynamodb",
        listOf("../sdk/aws-models/dynamodb.json"),
    ),
    Service(
        "com.amazonaws.s3#AmazonS3",
        "aws-sdk-s3",
        listOf("../sdk/aws-models/s3.json", "../sdk/aws-models/s3-tests.smithy"),
    ),
)
val allCodegenTests = servicesToGenerate.map {
    CodegenTest(
        it.serviceId,
        it.moduleName,
        imports = it.imports,
        extraConfig = """
            ,
            "codegen": {
                "includeFluentClient": false,
                "enableNewSmithyRuntime": "orchestrator",
                "includeEndpointUrlConfig": false
            },
            "customizationConfig": {
                "awsSdk": {
                    "generateReadme": false
                }
            }
        """,
    )
}

project.registerGenerateSmithyBuildTask(rootProject, pluginName, allCodegenTests)
project.registerGenerateCargoWorkspaceTask(rootProject, pluginName, allCodegenTests, workingDirUnderBuildDir)
project.registerGenerateCargoConfigTomlTask(buildDir.resolve(workingDirUnderBuildDir))

tasks["smithyBuildJar"].dependsOn("generateSmithyBuild")
tasks["assemble"].finalizedBy("generateCargoWorkspace")

project.registerModifyMtimeTask()
project.registerCargoCommandsTasks(buildDir.resolve(workingDirUnderBuildDir), defaultRustDocFlags)

tasks["test"].finalizedBy(cargoCommands(properties).map { it.toString })

tasks["clean"].doFirst { delete("smithy-build.json") }

/**
 * The aws/rust-runtime crates depend on local versions of the Smithy core runtime enabling local compilation. However,
 * those paths need to be replaced in the final build. We should probably fix this with some symlinking.
 */
fun rewritePathDependency(line: String): String {
    // some runtime crates are actually dependent on the generated bindings:
    return line.replace("../sdk/build/aws-sdk/sdk/", "")
        // others use relative dependencies::
        .replace("../../rust-runtime/", "")
}

tasks.register("relocateServices") {
    description = "relocate AWS services to their final destination"
    doLast {
        servicesToGenerate.forEach { service ->
            logger.info("Relocating ${service.moduleName}...")
            copy {
                from("$buildDir/smithyprojections/sdk-sra-test/${service.moduleName}/rust-client-codegen")
                into(outputDir.resolve(service.moduleName))
            }
            copy {
                from(projectDir.resolve("integration-tests/${service.moduleName}/tests"))
                into(outputDir.resolve(service.moduleName).resolve("tests"))
            }
        }
    }
    dependsOn("smithyBuildJar")
    inputs.dir("$buildDir/smithyprojections/sdk-sra-test/")
    outputs.dir(outputDir)
}
tasks["assemble"].apply {
    dependsOn("relocateServices")
}
