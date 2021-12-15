/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import java.util.Properties

extra["displayName"] = "Smithy :: Rust :: AWS-SDK"
extra["moduleName"] = "software.amazon.smithy.rust.awssdk"

tasks["jar"].enabled = false

plugins {
    id("software.amazon.smithy").version("0.5.3")
}

val smithyVersion: String by project

val outputDir = buildDir.resolve("aws-sdk")
val sdkOutputDir = outputDir.resolve("sdk")
val examplesOutputDir = outputDir.resolve("examples")

val runtimeModules = listOf(
    "aws-smithy-async",
    "aws-smithy-client",
    "aws-smithy-eventstream",
    "aws-smithy-http",
    "aws-smithy-http-tower",
    "aws-smithy-json",
    "aws-smithy-protocol-test",
    "aws-smithy-query",
    "aws-smithy-types",
    "aws-smithy-types-convert",
    "aws-smithy-xml"
)
val awsModules = listOf(
    "aws-config",
    "aws-endpoint",
    "aws-http",
    "aws-hyper",
    "aws-sig-auth",
    "aws-sigv4",
    "aws-types"
)

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

// get a project property by name if it exists (including from local.properties)
fun getProperty(name: String): String? {
    if (project.hasProperty(name)) {
        return project.properties[name].toString()
    }

    val localProperties = Properties()
    val propertiesFile: File = rootProject.file("local.properties")
    if (propertiesFile.exists()) {
        propertiesFile.inputStream().use { localProperties.load(it) }

        if (localProperties.containsKey(name)) {
            return localProperties[name].toString()
        }
    }
    return null
}

// Class and functions for service and protocol membership for SDK generation

val awsServices: List<AwsService> by lazy { discoverServices(loadServiceMembership()) }
val eventStreamAllowList: Set<String> by lazy { eventStreamAllowList() }

fun loadServiceMembership(): Membership {
    val membershipOverride = getProperty("aws.services")?.let { parseMembership(it) }
    println(membershipOverride)
    val fullSdk =
        parseMembership(getProperty("aws.services.fullsdk") ?: throw kotlin.Exception("full sdk list missing"))
    val tier1 =
        parseMembership(getProperty("aws.services.smoketest") ?: throw kotlin.Exception("smoketest list missing"))
    return membershipOverride ?: if ((getProperty("aws.fullsdk") ?: "") == "true") {
        fullSdk
    } else {
        tier1
    }
}

fun eventStreamAllowList(): Set<String> {
    val list = getProperty("aws.services.eventstream.allowlist") ?: ""
    return list.split(",").map { it.trim() }.toSet()
}

fun generateSmithyBuild(services: List<AwsService>): String {
    val serviceProjections = services.map { service ->
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
                        "moduleVersion": "${getProperty("aws.sdk.version")}",
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
    inputs.property("servicelist", awsServices.sortedBy { it.module }.toString())
    inputs.property("eventStreamAllowList", eventStreamAllowList)
    inputs.dir(projectDir.resolve("aws-models"))
    outputs.file(projectDir.resolve("smithy-build.json"))

    doFirst {
        projectDir.resolve("smithy-build.json").writeText(generateSmithyBuild(awsServices))
    }
}

task("generateDocs") {
    inputs.property("servicelist", awsServices.sortedBy { it.module }.toString())
    outputs.file(outputDir.resolve("docs.md"))
    doLast {
        project.docsLandingPage(awsServices, outputDir)
    }
}

task("relocateServices") {
    description = "relocate AWS services to their final destination"
    doLast {
        awsServices.forEach {
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
            include("examples/**")
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

fun rewriteCrateVersion(line: String, version: String): String = line.replace(
    """^\s*version\s+=\s+"0.0.0-smithy-rs-head"$""".toRegex(),
    "version = \"$version\""
)

/**
 * AWS runtime crate versions are all `0.0.0-smithy-rs-head`. When copying over to the AWS SDK,
 * these should be changed to the AWS SDK version.
 */
fun rewriteAwsSdkCrateVersion(line: String): String = rewriteCrateVersion(line, getProperty("aws.sdk.version")!!)

/**
 * Smithy runtime crate versions in smithy-rs are all `0.0.0-smithy-rs-head`. When copying over to the AWS SDK,
 * these should be changed to the smithy-rs version.
 */
fun rewriteSmithyRsCrateVersion(line: String): String =
    rewriteCrateVersion(line, getProperty("smithy.rs.runtime.crate.version")!!)

/** Patches a file with the result of the given `operation` being run on each line */
fun patchFile(path: File, operation: (String) -> String) {
    val patchedContents = path.readLines().joinToString("\n", transform = operation)
    path.writeText(patchedContents)
}

tasks.register<Copy>("copyAllRuntimes") {
    from("$rootDir/aws/rust-runtime") {
        awsModules.forEach { include("$it/**") }
    }
    from("$rootDir/rust-runtime") {
        runtimeModules.forEach { include("$it/**") }
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
        awsModules.forEach { moduleName ->
            patchFile(sdkOutputDir.resolve("$moduleName/Cargo.toml")) { line ->
                line.let(::rewritePathDependency)
                    .let(::rewriteAwsSdkCrateVersion)
            }
        }
    }
}
tasks.register("relocateRuntime") {
    dependsOn("copyAllRuntimes")
    doLast {
        // Patch the Cargo.toml files
        runtimeModules.forEach { moduleName ->
            patchFile(sdkOutputDir.resolve("$moduleName/Cargo.toml")) { line ->
                line.let(::rewriteSmithyRsCrateVersion)
            }
        }
    }
}

fun generateCargoWorkspace(services: List<AwsService>): String {
    val generatedModules = services.map { it.module }.toSet()
    val examples = projectDir.resolve("examples")
        .listFiles { file -> !file.name.startsWith(".") }.orEmpty().toList()
        .filter { file ->
            val cargoToml = File(file, "Cargo.toml")
            if (cargoToml.exists()) {
                val usedModules = cargoToml.readLines()
                    .map { line -> line.substringBefore('=').trim() }
                    .filter { line -> line.startsWith("aws-sdk-") }
                    .map { line -> line.substringAfter("aws-sdk-") }
                    .toSet()
                generatedModules.containsAll(usedModules)
            } else {
                false
            }
        }
        .map { "examples/${it.name}" }

    val modules = (
        services.map(AwsService::module).map { "sdk/$it" } +
            runtimeModules.map { "sdk/$it" } +
            awsModules.map { "sdk/$it" } +
            examples.toList()
        ).sorted()
    return """
    |[workspace]
    |members = [${"\n"}${modules.joinToString(",\n") { "|    \"$it\"" }}
    |]
    """.trimMargin()
}
task("generateCargoWorkspace") {
    description = "generate Cargo.toml workspace file"
    doFirst {
        outputDir.mkdirs()
        outputDir.resolve("Cargo.toml").writeText(generateCargoWorkspace(awsServices))
    }
    inputs.property("servicelist", awsServices.sortedBy { it.module }.toString())
    inputs.dir(projectDir.resolve("examples"))
    outputs.file(outputDir.resolve("Cargo.toml"))
    outputs.upToDateWhen { false }
}

task("finalizeSdk") {
    dependsOn("assemble")
    outputs.upToDateWhen { false }
    finalizedBy(
        "relocateServices",
        "relocateRuntime",
        "relocateAwsRuntime",
        "relocateExamples",
        "generateDocs"
    )
}

tasks["smithyBuildJar"].inputs.file(projectDir.resolve("smithy-build.json"))
tasks["smithyBuildJar"].inputs.dir(projectDir.resolve("aws-models"))
tasks["smithyBuildJar"].dependsOn("generateSmithyBuild")
tasks["smithyBuildJar"].dependsOn("generateCargoWorkspace")
tasks["smithyBuildJar"].outputs.upToDateWhen { false }
tasks["assemble"].dependsOn("smithyBuildJar")
tasks["assemble"].finalizedBy("finalizeSdk")

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

tasks.register<RunExampleTask>("runExample") {
    dependsOn("assemble")
    outputDir = outputDir
}

// TODO: validate that the example exists. Otherwise this fails with a hidden error.
open class RunExampleTask @javax.inject.Inject constructor() : Exec() {
    @Option(option = "example", description = "Example to run")
    var example: String? = null
        set(value) {
            workingDir = workingDir.resolve(value!!)
            field = value
        }

    @org.gradle.api.tasks.InputDirectory
    var outputDir: File? = null
        set(value) {
            workingDir = value!!.resolve("examples")
            commandLine = listOf("cargo", "run")
            field = value
        }
}

tasks["test"].finalizedBy("cargoClippy", "cargoTest", "cargoDocs")

tasks["clean"].doFirst {
    delete("smithy-build.json")
}
