/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
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
    id("software.amazon.smithy")
}

configure<software.amazon.smithy.gradle.SmithyExtension> {
    smithyBuildConfigs = files(buildDir.resolve("smithy-build.json"))
}

val smithyVersion: String by project
val defaultRustDocFlags: String by project
val properties = PropertyRetriever(rootProject, project)

val crateHasherToolPath = rootProject.projectDir.resolve("tools/ci-build/crate-hasher")
val publisherToolPath = rootProject.projectDir.resolve("tools/ci-build/publisher")
val sdkVersionerToolPath = rootProject.projectDir.resolve("tools/ci-build/sdk-versioner")
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

val awsServices: AwsServices by lazy {
    discoverServices(properties.get("aws.sdk.models.path"), loadServiceMembership())
}
val eventStreamAllowList: Set<String> by lazy { eventStreamAllowList() }
val crateVersioner by lazy { aws.sdk.CrateVersioner.defaultFor(rootProject, properties) }

fun getRustMSRV(): String = properties.get("rust.msrv") ?: throw Exception("Rust MSRV missing")
fun getPreviousReleaseVersionManifestPath(): String? = properties.get("aws.sdk.previous.release.versions.manifest")

fun loadServiceMembership(): Membership {
    val membershipOverride = properties.get("aws.services")?.let { parseMembership(it) }
    println(membershipOverride)
    val fullSdk =
        parseMembership(properties.get("aws.services") ?: throw Exception("aws.services list missing"))
    return membershipOverride ?: fullSdk
}

fun eventStreamAllowList(): Set<String> {
    val list = properties.get("aws.services.eventstream.allowlist") ?: ""
    return list.split(",").map { it.trim() }.toSet()
}

fun generateSmithyBuild(services: AwsServices): String {
    val awsConfigVersion = properties.get("smithy.rs.runtime.crate.version")
        ?: throw IllegalStateException("missing smithy.rs.runtime.crate.version for aws-config version")
    val debugMode = properties.get("debugMode").toBoolean()
    val serviceProjections = services.services.map { service ->
        val files = service.modelFiles().map { extraFile ->
            software.amazon.smithy.utils.StringUtils.escapeJavaString(
                extraFile.absolutePath,
                "",
            )
        }
        val moduleName = "aws-sdk-${service.module}"
        val eventStreamAllowListMembers = eventStreamAllowList.joinToString(", ") { "\"$it\"" }
        """
            "${service.module}": {
                "imports": [${files.joinToString()}],

                "plugins": {
                    "rust-client-codegen": {
                        "runtimeConfig": {
                            "relativePath": "../",
                            "version": "DEFAULT"
                        },
                        "codegen": {
                            "includeFluentClient": false,
                            "renameErrors": false,
                            "debugMode": $debugMode,
                            "eventStreamAllowList": [$eventStreamAllowListMembers],
                            "enableNewCrateOrganizationScheme": true,
                            "enableNewSmithyRuntime": false
                        },
                        "service": "${service.service}",
                        "module": "$moduleName",
                        "moduleVersion": "${crateVersioner.decideCrateVersion(moduleName, service)}",
                        "moduleAuthors": ["AWS Rust SDK Team <aws-sdk-rust@amazon.com>", "Russell Cohen <rcoh@amazon.com>"],
                        "moduleDescription": "${service.moduleDescription}",
                        ${service.examplesUri(project)?.let { """"examples": "$it",""" } ?: ""}
                        "moduleRepository": "https://github.com/awslabs/aws-sdk-rust",
                        "license": "Apache-2.0",
                        "customizationConfig": {
                            "awsSdk": {
                                "generateReadme": true,
                                "awsConfigVersion": "$awsConfigVersion",
                                "defaultConfigPath": "${services.defaultConfigPath}",
                                "endpointsConfigPath": "${services.endpointsConfigPath}",
                                "integrationTestPath": "${project.projectDir.resolve("integration-tests")}"
                            }
                        }
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

tasks.register("generateSmithyBuild") {
    description = "generate smithy-build.json"
    inputs.property("servicelist", awsServices.services.toString())
    inputs.property("eventStreamAllowList", eventStreamAllowList)
    inputs.dir(projectDir.resolve("aws-models"))
    outputs.file(buildDir.resolve("smithy-build.json"))

    doFirst {
        buildDir.resolve("smithy-build.json").writeText(generateSmithyBuild(awsServices))
    }
    outputs.upToDateWhen { false }
}

tasks.register("generateIndexMd") {
    inputs.property("servicelist", awsServices.services.toString())
    val indexMd = outputDir.resolve("index.md")
    outputs.file(indexMd)
    doLast {
        project.docsLandingPage(awsServices, indexMd)
    }
}

tasks.register("relocateServices") {
    description = "relocate AWS services to their final destination"
    doLast {
        awsServices.services.forEach {
            logger.info("Relocating ${it.module}...")
            copy {
                from("$buildDir/smithyprojections/sdk/${it.module}/rust-client-codegen")
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

tasks.register("relocateExamples") {
    description = "relocate the examples folder & rewrite path dependencies"
    doLast {
        if (awsServices.examples.isNotEmpty()) {
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
    }
    if (awsServices.examples.isNotEmpty()) {
        inputs.dir(projectDir.resolve("examples"))
    }
    outputs.dir(outputDir)
}

tasks.register("relocateTests") {
    description = "relocate the root integration tests and rewrite path dependencies"
    doLast {
        if (awsServices.rootTests.isNotEmpty()) {
            copy {
                val testDir = projectDir.resolve("integration-tests")
                from(testDir)
                awsServices.rootTests.forEach { test ->
                    include(test.path.toRelativeString(testDir) + "/**")
                }
                into(outputDir.resolve("tests"))
                exclude("**/target")
                filter { line -> line.replace("build/aws-sdk/sdk/", "sdk/") }
            }
        }
    }
    for (test in awsServices.rootTests) {
        inputs.dir(test.path)
    }
    outputs.dir(outputDir)
}

tasks.register<ExecRustBuildTool>("fixExampleManifests") {
    description = "Adds dependency path and corrects version number of examples after relocation"
    enabled = awsServices.examples.isNotEmpty()

    toolPath = sdkVersionerToolPath
    binaryName = "sdk-versioner"
    arguments = listOf(
        "use-path-and-version-dependencies",
        "--isolate-crates",
        "--sdk-path", "../../sdk",
        "--versions-toml", outputDir.resolve("versions.toml").absolutePath,
        outputDir.resolve("examples").absolutePath,
    )

    outputs.dir(outputDir)
    dependsOn("relocateExamples", "generateVersionManifest")
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
                rewriteRuntimeCrateVersion(properties, line.let(::rewritePathDependency))
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
                rewriteRuntimeCrateVersion(properties, line)
            }
        }
    }
}

tasks.register<Copy>("relocateChangelog") {
    from("$rootDir/aws")
    include("SDK_CHANGELOG.md")
    into(outputDir)
    rename("SDK_CHANGELOG.md", "CHANGELOG.md")
}

fun generateCargoWorkspace(services: AwsServices): String {
    return """
    |[workspace]
    |exclude = [${"\n"}${services.excludedFromWorkspace().joinToString(",\n") { "|    \"$it\"" }}
    |]
    |members = [${"\n"}${services.includedInWorkspace().joinToString(",\n") { "|    \"$it\"" }}
    |]
    """.trimMargin()
}

tasks.register("generateCargoWorkspace") {
    description = "generate Cargo.toml workspace file"
    doFirst {
        outputDir.mkdirs()
        outputDir.resolve("Cargo.toml").writeText(generateCargoWorkspace(awsServices))
    }
    inputs.property("servicelist", awsServices.moduleNames.toString())
    if (awsServices.examples.isNotEmpty()) {
        inputs.dir(projectDir.resolve("examples"))
    }
    for (test in awsServices.rootTests) {
        inputs.dir(test.path)
    }
    outputs.file(outputDir.resolve("Cargo.toml"))
    outputs.upToDateWhen { false }
}

tasks.register<ExecRustBuildTool>("fixManifests") {
    description = "Run the publisher tool's `fix-manifests` sub-command on the generated services"

    inputs.dir(publisherToolPath)
    outputs.dir(outputDir)

    toolPath = publisherToolPath
    binaryName = "publisher"
    arguments = mutableListOf("fix-manifests", "--location", outputDir.absolutePath).apply {
        if (crateVersioner.independentVersioningEnabled()) {
            add("--disable-version-number-validation")
        }
    }

    dependsOn("assemble")
    dependsOn("relocateServices")
    dependsOn("relocateRuntime")
    dependsOn("relocateAwsRuntime")
    dependsOn("relocateExamples")
    dependsOn("relocateTests")
}

tasks.register<ExecRustBuildTool>("hydrateReadme") {
    description = "Run the publisher tool's `hydrate-readme` sub-command to create the final AWS Rust SDK README file"

    dependsOn("generateVersionManifest")

    inputs.dir(publisherToolPath)
    inputs.file(rootProject.projectDir.resolve("aws/SDK_README.md.hb"))
    outputs.file(outputDir.resolve("README.md").absolutePath)

    toolPath = publisherToolPath
    binaryName = "publisher"
    arguments = listOf(
        "hydrate-readme",
        "--versions-manifest", outputDir.resolve("versions.toml").toString(),
        "--msrv", getRustMSRV(),
        "--input", rootProject.projectDir.resolve("aws/SDK_README.md.hb").toString(),
        "--output", outputDir.resolve("README.md").absolutePath,
    )
}

tasks.register<RequireRustBuildTool>("requireCrateHasher") {
    description = "Ensures the crate-hasher tool is available"
    inputs.dir(crateHasherToolPath)
    toolPath = crateHasherToolPath
}

tasks.register<ExecRustBuildTool>("generateVersionManifest") {
    description = "Generate the SDK version.toml file"
    dependsOn("requireCrateHasher")
    dependsOn("fixManifests")

    inputs.dir(publisherToolPath)

    toolPath = publisherToolPath
    binaryName = "publisher"
    arguments = mutableListOf(
        "generate-version-manifest",
        "--location",
        outputDir.absolutePath,
        "--smithy-build",
        buildDir.resolve("smithy-build.json").normalize().absolutePath,
        "--examples-revision",
        properties.get("aws.sdk.examples.revision") ?: "missing",
    ).apply {
        val previousReleaseManifestPath = getPreviousReleaseVersionManifestPath()?.let { manifestPath ->
            add("--previous-release-versions")
            add(manifestPath)
        }
    }
}

tasks.register("finalizeSdk") {
    dependsOn("assemble")
    outputs.upToDateWhen { false }
    finalizedBy(
        "relocateServices",
        "relocateRuntime",
        "relocateAwsRuntime",
        "relocateExamples",
        "relocateTests",
        "generateIndexMd",
        "fixManifests",
        "generateVersionManifest",
        "fixExampleManifests",
        "hydrateReadme",
        "relocateChangelog",
    )
}

tasks["smithyBuildJar"].apply {
    inputs.file(buildDir.resolve("smithy-build.json"))
    inputs.dir(projectDir.resolve("aws-models"))
    dependsOn("generateSmithyBuild")
    dependsOn("generateCargoWorkspace")
    outputs.upToDateWhen { false }
}
tasks["assemble"].apply {
    dependsOn("deleteSdk")
    dependsOn("smithyBuildJar")
    finalizedBy("finalizeSdk")
}

project.registerCargoCommandsTasks(outputDir, defaultRustDocFlags)
project.registerGenerateCargoConfigTomlTask(outputDir)

tasks["test"].finalizedBy(Cargo.CLIPPY.toString, Cargo.TEST.toString, Cargo.DOCS.toString)

tasks.register<Delete>("deleteSdk") {
    delete = setOf(outputDir)
}
tasks["clean"].dependsOn("deleteSdk")
tasks["clean"].doFirst {
    delete(buildDir.resolve("smithy-build.json"))
}
