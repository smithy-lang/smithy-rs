/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import aws.sdk.AwsServices
import aws.sdk.Membership
import aws.sdk.discoverServices
import aws.sdk.docsLandingPage
import aws.sdk.parseMembership

extra["displayName"] = "Smithy :: Rust :: AWS-SDK"
extra["moduleName"] = "software.amazon.smithy.rust.awssdk"

tasks["jar"].enabled = false

plugins {
    id("software.amazon.smithy").version("0.5.3")
}

val smithyVersion: String by project
val properties = PropertyRetriever(rootProject, project)

val outputDir = buildDir.resolve("aws-sdk")
val sdkOutputDir = outputDir.resolve("sdk")
val examplesOutputDir = outputDir.resolve("examples")

buildscript {
    val smithyVersion: String by project
    dependencies {
        classpath("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
        classpath("software.amazon.smithy:smithy-cli:$smithyVersion")
    }
}

dependencies {
    implementation(project(":aws:sdk-codegen"))
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-iam-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-cloudformation-traits:$smithyVersion")
}

// Class and functions for service and protocol membership for SDK generation

val awsServices: AwsServices by lazy { discoverServices(loadServiceMembership()) }
val eventStreamAllowList: Set<String> by lazy { eventStreamAllowList() }

fun loadServiceMembership(): Membership {
    val membershipOverride = properties.get("aws.services")?.let { parseMembership(it) }
    println(membershipOverride)
    val fullSdk =
        parseMembership(properties.get("aws.services.fullsdk") ?: throw kotlin.Exception("full sdk list missing"))
    val tier1 =
        parseMembership(properties.get("aws.services.smoketest") ?: throw kotlin.Exception("smoketest list missing"))
    return membershipOverride ?: if ((properties.get("aws.fullsdk") ?: "") == "true") {
        fullSdk
    } else {
        tier1
    }
}

fun eventStreamAllowList(): Set<String> {
    val list = properties.get("aws.services.eventstream.allowlist") ?: ""
    return list.split(",").map { it.trim() }.toSet()
}

fun generateSmithyBuild(services: AwsServices): String {
    val serviceProjections = services.services.map { service ->
        val files = service.files().map { extraFile ->
            software.amazon.smithy.utils.StringUtils.escapeJavaString(
                extraFile.absolutePath,
                ""
            )
        }
        val eventStreamAllowListMembers = eventStreamAllowList.joinToString(", ") { "\"$it\"" }
        """
            "${service.module}": {
                "imports": [${files.joinToString()}],

                "plugins": {
                    "rust-codegen": {
                        "runtimeConfig": {
                            "relativePath": "../",
                            "version": "DEFAULT"
                        },
                        "codegen": {
                            "includeFluentClient": false,
                            "renameErrors": false,
                            "eventStreamAllowList": [$eventStreamAllowListMembers]
                        },
                        "service": "${service.service}",
                        "module": "aws-sdk-${service.module}",
                        "moduleVersion": "${properties.get("aws.sdk.version")}",
                        "moduleAuthors": ["AWS Rust SDK Team <aws-sdk-rust@amazon.com>", "Russell Cohen <rcoh@amazon.com>"],
                        "moduleDescription": "${service.moduleDescription}",
                        ${service.examplesUri(project)?.let { """"examples": "$it",""" } ?: ""}
                        "moduleRepository": "https://github.com/awslabs/aws-sdk-rust",
                        "license": "Apache-2.0"
                        ${service.extraConfig ?: ""}
                    }
                }
            }
        """.trimIndent()
    }
    val projections = serviceProjections.joinToString(",\n")
    return """
    {
        "version": "1.0",
        "projections": { $projections }
    }
    """
}

task("generateSmithyBuild") {
    description = "generate smithy-build.json"
    inputs.property("servicelist", awsServices.services.toString())
    inputs.property("eventStreamAllowList", eventStreamAllowList)
    inputs.dir(projectDir.resolve("aws-models"))
    outputs.file(projectDir.resolve("smithy-build.json"))

    doFirst {
        projectDir.resolve("smithy-build.json").writeText(generateSmithyBuild(awsServices))
    }
}

task("generateDocs") {
    inputs.property("servicelist", awsServices.services.toString())
    outputs.file(outputDir.resolve("docs.md"))
    doLast {
        project.docsLandingPage(awsServices, outputDir)
    }
}

task("relocateServices") {
    description = "relocate AWS services to their final destination"
    doLast {
        awsServices.services.forEach {
            logger.info("Relocating ${it.module}...")
            copy {
                from("$buildDir/smithyprojections/sdk/${it.module}/rust-codegen")
                into(sdkOutputDir.resolve(it.module))
            }

            copy {
                from(projectDir.resolve("integration-tests/${it.module}/tests"))
                into(sdkOutputDir.resolve(it.module).resolve("tests"))
            }

            copy {
                from(projectDir.resolve("integration-tests/${it.module}/benches"))
                into(sdkOutputDir.resolve(it.module).resolve("benches"))
            }
        }
    }
    inputs.dir("$buildDir/smithyprojections/sdk/")
    outputs.dir(sdkOutputDir)
}

task("relocateExamples") {
    description = "relocate the examples folder & rewrite path dependencies"
    doLast {
        copy {
            from(projectDir)
            awsServices.examples.forEach { example ->
                include("$example/**")
            }
            into(outputDir)
            exclude("**/target")
            filter { line -> line.replace("build/aws-sdk/sdk/", "sdk/") }
        }
    }
    inputs.dir(projectDir.resolve("examples"))
    outputs.dir(outputDir)
}

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

tasks.register<Copy>("copyAllRuntimes") {
    from("$rootDir/aws/rust-runtime") {
        CrateSet.AWS_SDK_RUNTIME.forEach { include("$it/**") }
    }
    from("$rootDir/rust-runtime") {
        CrateSet.AWS_SDK_SMITHY_RUNTIME.forEach { include("$it/**") }
    }
    exclude("**/target")
    exclude("**/Cargo.lock")
    exclude("**/node_modules")
    into(sdkOutputDir)
}

tasks.register("relocateAwsRuntime") {
    dependsOn("copyAllRuntimes")
    doLast {
        // Patch the Cargo.toml files
        CrateSet.AWS_SDK_RUNTIME.forEach { moduleName ->
            patchFile(sdkOutputDir.resolve("$moduleName/Cargo.toml")) { line ->
                rewriteAwsSdkCrateVersion(properties, line.let(::rewritePathDependency))
            }
        }
    }
}
tasks.register("relocateRuntime") {
    dependsOn("copyAllRuntimes")
    doLast {
        // Patch the Cargo.toml files
        CrateSet.AWS_SDK_SMITHY_RUNTIME.forEach { moduleName ->
            patchFile(sdkOutputDir.resolve("$moduleName/Cargo.toml")) { line ->
                rewriteSmithyRsCrateVersion(properties, line)
            }
        }
    }
}

fun generateCargoWorkspace(services: AwsServices): String {
    return """
    |[workspace]
    |members = [${"\n"}${services.allModules.joinToString(",\n") { "|    \"$it\"" }}
    |]
    """.trimMargin()
}

task("generateCargoWorkspace") {
    description = "generate Cargo.toml workspace file"
    doFirst {
        outputDir.mkdirs()
        outputDir.resolve("Cargo.toml").writeText(generateCargoWorkspace(awsServices))
    }
    inputs.property("servicelist", awsServices.moduleNames.toString())
    inputs.dir(projectDir.resolve("examples"))
    outputs.file(outputDir.resolve("Cargo.toml"))
    outputs.upToDateWhen { false }
}

tasks.register<Exec>("fixManifests") {
    description = "Run the publisher tool's `fix-manifests` sub-command on the generated services"

    val publisherPath = rootProject.projectDir.resolve("tools/publisher")
    inputs.dir(publisherPath)
    outputs.dir(outputDir)

    workingDir(publisherPath)
    commandLine("cargo", "run", "--", "fix-manifests", "--location", outputDir.absolutePath)

    dependsOn("assemble")
    dependsOn("relocateServices")
    dependsOn("relocateRuntime")
    dependsOn("relocateAwsRuntime")
    dependsOn("relocateExamples")
}

task("finalizeSdk") {
    dependsOn("assemble")
    outputs.upToDateWhen { false }
    finalizedBy(
        "relocateServices",
        "relocateRuntime",
        "relocateAwsRuntime",
        "relocateExamples",
        "generateDocs",
        "fixManifests"
    )
}

tasks["smithyBuildJar"].apply {
    inputs.file(projectDir.resolve("smithy-build.json"))
    inputs.dir(projectDir.resolve("aws-models"))
    dependsOn("generateSmithyBuild")
    dependsOn("generateCargoWorkspace")
    outputs.upToDateWhen { false }
}
tasks["assemble"].apply {
    dependsOn("smithyBuildJar")
    finalizedBy("finalizeSdk")
}

tasks.register<Exec>("cargoCheck") {
    workingDir(outputDir)
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "check", "--lib", "--tests", "--benches")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoTest") {
    workingDir(outputDir)
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "test")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoDocs") {
    workingDir(outputDir)
    // disallow warnings
    environment("RUSTDOCFLAGS", "-D warnings")
    commandLine("cargo", "doc", "--no-deps", "--document-private-items")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoClippy") {
    workingDir(outputDir)
    // disallow warnings
    commandLine("cargo", "clippy", "--", "-D", "warnings")
    dependsOn("assemble")
}

tasks["test"].finalizedBy("cargoClippy", "cargoTest", "cargoDocs")

tasks["clean"].doFirst {
    delete("smithy-build.json")
}
