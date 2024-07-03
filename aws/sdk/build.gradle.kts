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
    java
    id("software.amazon.smithy.gradle.smithy-base")
    id("software.amazon.smithy.gradle.smithy-jar")
}

java {
    sourceCompatibility = JavaVersion.VERSION_11
    targetCompatibility = JavaVersion.VERSION_11
}

configure<software.amazon.smithy.gradle.SmithyExtension> {
    smithyBuildConfigs = files(layout.buildDirectory.file("smithy-build.json"))
    allowUnknownTraits = true
}

val smithyVersion: String by project
val properties = PropertyRetriever(rootProject, project)

val crateHasherToolPath = rootProject.projectDir.resolve("tools/ci-build/crate-hasher")
val publisherToolPath = rootProject.projectDir.resolve("tools/ci-build/publisher")
val sdkVersionerToolPath = rootProject.projectDir.resolve("tools/ci-build/sdk-versioner")
val awsConfigPath = rootProject.projectDir.resolve("aws/rust-runtime/aws-config")
val rustRuntimePath = rootProject.projectDir.resolve("rust-runtime")
val awsRustRuntimePath = rootProject.projectDir.resolve("aws/rust-runtime")
val checkedInCargoLock = rootProject.projectDir.resolve("aws/sdk/Cargo.lock")
val outputDir = layout.buildDirectory.dir("aws-sdk").get()
val sdkOutputDir = outputDir.dir("sdk")
val examplesOutputDir = outputDir.dir("examples")


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
fun getNullabilityCheckMode(): String = properties.get("nullability.check.mode") ?: "CLIENT_CAREFUL"

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
    val awsConfigVersion = properties.get(CrateSet.STABLE_VERSION_PROP_NAME)
        ?: throw IllegalStateException("missing ${CrateSet.STABLE_VERSION_PROP_NAME} for aws-config version")
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
        val defaultConfigPath =
            services.defaultConfigPath.let { software.amazon.smithy.utils.StringUtils.escapeJavaString(it, "") }
        val partitionsConfigPath =
            services.partitionsConfigPath.let { software.amazon.smithy.utils.StringUtils.escapeJavaString(it, "") }
        val integrationTestPath = project.projectDir.resolve("integration-tests")
            .let { software.amazon.smithy.utils.StringUtils.escapeJavaString(it, "") }
        """
            "${service.module}": {
                "imports": [${files.joinToString()}],

                "plugins": {
                    "rust-client-codegen": {
                        "runtimeConfig": {
                            "relativePath": "../"
                        },
                        "codegen": {
                            "includeFluentClient": false,
                            "includeEndpointUrlConfig": false,
                            "renameErrors": false,
                            "debugMode": $debugMode,
                            "eventStreamAllowList": [$eventStreamAllowListMembers],
                            "enableUserConfigurableRuntimePlugins": false,
                            "nullabilityCheckMode": "${getNullabilityCheckMode()}"
                        },
                        "service": "${service.service}",
                        "module": "$moduleName",
                        "moduleVersion": "${crateVersioner.decideCrateVersion(moduleName, service)}",
                        "moduleAuthors": ["AWS Rust SDK Team <aws-sdk-rust@amazon.com>", "Russell Cohen <rcoh@amazon.com>"],
                        "moduleDescription": "${service.moduleDescription}",
                        ${service.examplesUri(project)?.let { """"examples": "$it",""" } ?: ""}
                        "moduleRepository": "https://github.com/awslabs/aws-sdk-rust",
                        "license": "Apache-2.0",
                        "minimumSupportedRustVersion": "${getRustMSRV()}",
                        "customizationConfig": {
                            "awsSdk": {
                                "awsSdkBuild": true,
                                "awsConfigVersion": "$awsConfigVersion",
                                "defaultConfigPath": $defaultConfigPath,
                                "partitionsConfigPath": $partitionsConfigPath,
                                "integrationTestPath": $integrationTestPath
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

/**
 * Task to generate smithyBuild.json dynamically
 */
tasks.register("generateSmithyBuild") {
    description = "generate smithy-build.json"
    inputs.property("servicelist", awsServices.services.toString())
    inputs.property("eventStreamAllowList", eventStreamAllowList)
    inputs.dir(projectDir.resolve("aws-models"))
    outputs.file(layout.buildDirectory.file("smithy-build.json"))

    doFirst {
        layout.buildDirectory.file("smithy-build.json").get().asFile.writeText(generateSmithyBuild(awsServices))
    }
    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3599)
    outputs.upToDateWhen { false }
}

tasks.register("generateIndexMd") {
    dependsOn("jar")

    inputs.property("servicelist", awsServices.services.toString())
    val indexMd = outputDir.file("index.md").asFile
    outputs.file(indexMd)
    doLast {
        project.docsLandingPage(awsServices, indexMd)
    }
}

tasks.register("relocateServices") {
    description = "relocate AWS services to their final destination"
    dependsOn("jar")

    doLast {
        awsServices.services.forEach {
            logger.info("Relocating ${it.module}...")
            copy {
                from(layout.buildDirectory.dir("smithyprojections/sdk/${it.module}/rust-client-codegen"))
                into(sdkOutputDir.dir(it.module))
            }

            copy {
                from(projectDir.resolve("integration-tests/${it.module}/tests"))
                into(sdkOutputDir.dir(it.module).dir("tests"))
            }

            copy {
                from(projectDir.resolve("integration-tests/${it.module}/benches"))
                into(sdkOutputDir.dir(it.module).dir("benches"))
            }
        }
    }
    inputs.dir(layout.buildDirectory.dir("smithyprojections/sdk"))
    outputs.dir(sdkOutputDir)
}

tasks.register("relocateExamples") {
    description = "relocate the examples folder & rewrite path dependencies"
    dependsOn("jar")

    doLast {
        if (awsServices.examples.isNotEmpty()) {
            copy {
                from(projectDir)
                awsServices.examples.forEach { example ->
                    include("$example/**")
                }
                into(outputDir)
                exclude("**/target")
                exclude("**/rust-toolchain.toml")
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
    dependsOn("jar")

    doLast {
        if (awsServices.rootTests.isNotEmpty()) {
            copy {
                val testDir = projectDir.resolve("integration-tests")
                from(testDir)
                awsServices.rootTests.forEach { test ->
                    include(test.path.toRelativeString(testDir) + "/**")
                }
                into(outputDir.dir("tests"))
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
    dependsOn("relocateExamples")

    toolPath = sdkVersionerToolPath
    binaryName = "sdk-versioner"
    arguments = listOf(
        "use-path-and-version-dependencies",
        "--sdk-path", sdkOutputDir.asFile.absolutePath,
        "--versions-toml", outputDir.file("versions.toml").asFile.absolutePath,
        outputDir.dir("examples").asFile.absolutePath,
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
    dependsOn("jar")
    from("$rootDir/aws/rust-runtime") {
        CrateSet.AWS_SDK_RUNTIME.forEach { include("${it.name}/**") }
    }
    from("$rootDir/rust-runtime") {
        CrateSet.AWS_SDK_SMITHY_RUNTIME.forEach { include("${it.name}/**") }
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
        CrateSet.AWS_SDK_RUNTIME.forEach { module ->
            patchFile(sdkOutputDir.file("${module.name}/Cargo.toml").asFile) { line ->
                rewriteRuntimeCrateVersion(
                    properties.get(module.versionPropertyName)!!,
                    line.let(::rewritePathDependency),
                )
            }
        }
    }
}
tasks.register("relocateRuntime") {
    dependsOn("copyAllRuntimes")
    doLast {
        // Patch the Cargo.toml files
        CrateSet.AWS_SDK_SMITHY_RUNTIME.forEach { module ->
            patchFile(sdkOutputDir.file("${module.name}/Cargo.toml").asFile) { line ->
                rewriteRuntimeCrateVersion(properties.get(module.versionPropertyName)!!, line)
            }
        }
    }
}

tasks.register<Copy>("relocateChangelog") {
    dependsOn("jar")
    from("$rootDir/aws")
    include("SDK_CHANGELOG.md")
    into(outputDir)
    rename("SDK_CHANGELOG.md", "CHANGELOG.md")
}

fun generateCargoWorkspace(services: AwsServices): String {
    return """
    |[workspace]
    |resolver = "2"
    |exclude = [${"\n"}${services.excludedFromWorkspace().joinToString(",\n") { "|    \"$it\"" }}
    |]
    |members = [${"\n"}${services.includedInWorkspace().joinToString(",\n") { "|    \"$it\"" }}
    |]
    """.trimMargin()
}

tasks.register("generateCargoWorkspace") {
    description = "generate Cargo.toml workspace file"
    doFirst {
        outputDir.asFile.mkdirs()
        outputDir.file("Cargo.toml").asFile.writeText(generateCargoWorkspace(awsServices))
        rootProject.rootDir.resolve("clippy-root.toml").copyTo(outputDir.file("clippy.toml").asFile, overwrite = true)
    }
    inputs.property("servicelist", awsServices.moduleNames.toString())
    if (awsServices.examples.isNotEmpty()) {
        inputs.dir(projectDir.resolve("examples"))
    }
    for (test in awsServices.rootTests) {
        inputs.dir(test.path)
    }
    outputs.file(outputDir.file("Cargo.toml"))
    outputs.file(outputDir.file("clippy.toml"))
    outputs.upToDateWhen { false }
}

tasks.register<ExecRustBuildTool>("fixManifests") {
    description = "Run the publisher tool's `fix-manifests` sub-command on the generated services"
    dependsOn("relocateServices")
    dependsOn("relocateRuntime")
    dependsOn("relocateAwsRuntime")
    dependsOn("relocateExamples")
    dependsOn("relocateTests")

    inputs.dir(publisherToolPath)
    outputs.dir(outputDir)

    toolPath = publisherToolPath
    binaryName = "publisher"
    arguments = mutableListOf("fix-manifests", "--location", outputDir.asFile.absolutePath)
}

tasks.register<ExecRustBuildTool>("hydrateReadme") {
    description = "Run the publisher tool's `hydrate-readme` sub-command to create the final AWS Rust SDK README file"
    dependsOn("generateVersionManifest")

    inputs.dir(publisherToolPath)
    inputs.file(rootProject.projectDir.resolve("aws/SDK_README.md.hb"))
    outputs.file(outputDir.file("README.md").asFile.absolutePath)

    toolPath = publisherToolPath
    binaryName = "publisher"
    arguments = listOf(
        "hydrate-readme",
        "--versions-manifest", outputDir.file("versions.toml").toString(),
        "--msrv", getRustMSRV(),
        "--input", rootProject.projectDir.resolve("aws/SDK_README.md.hb").toString(),
        "--output", outputDir.file("README.md").asFile.absolutePath,
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
        "--input-location",
        sdkOutputDir.asFile.absolutePath,
        "--output-location",
        outputDir.asFile.absolutePath,
        "--smithy-build",
        layout.buildDirectory.file("smithy-build.json").get().asFile.normalize().absolutePath,
        "--examples-revision",
        properties.get("aws.sdk.examples.revision") ?: "missing",
    ).apply {
        getPreviousReleaseVersionManifestPath()?.let { manifestPath ->
            add("--previous-release-versions")
            add(manifestPath)
        }
    }
}

tasks["smithyBuild"].apply {
    inputs.file(layout.buildDirectory.file("smithy-build.json"))
    inputs.dir(projectDir.resolve("aws-models"))
    dependsOn("generateSmithyBuild")
    dependsOn("generateCargoWorkspace")
    outputs.upToDateWhen { false }
}
tasks["assemble"].apply {
    dependsOn(
        "deleteSdk",
        "jar",
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
    finalizedBy("copyCheckedInCargoLock")
    outputs.upToDateWhen { false }
}

tasks.register<Copy>("copyCheckedInCargoLock") {
    description = "Copy the checked in Cargo.lock file back to the build directory"
    this.outputs.upToDateWhen { false }
    from(checkedInCargoLock)
    into(outputDir)
}

project.registerCargoCommandsTasks(outputDir.asFile)
project.registerGenerateCargoConfigTomlTask(outputDir.asFile)

//The task name "test" is already registered by one of our plugins
tasks.register("sdkTest") {
    description = "Run Cargo clippy/test/docs against the generated SDK."
    dependsOn("assemble")
    finalizedBy(Cargo.CLIPPY.toString, Cargo.TEST.toString, Cargo.DOCS.toString)
}

//Tasks for generating individual Cargo.lock files
fun Project.registerLockfileGeneration(
    dir: File,
    name: String,
): TaskProvider<Exec> {
    return tasks.register<Exec>("generate${name}Lockfile") {
        workingDir(dir)
        environment("RUSTFLAGS", "--cfg aws_sdk_unstable")
        commandLine("cargo", "generate-lockfile")
    }
}

val generateAwsConfigLockfile = registerLockfileGeneration(awsConfigPath, "AwsConfig")
val generateAwsRuntimeLockfile = registerLockfileGeneration(awsRustRuntimePath, "AwsRustRuntime")
val generateSmithytRuntimeLockfile = registerLockfileGeneration(rustRuntimePath, "RustRuntime")

//Generates a lockfile from the aws-sdk-rust repo and copies it into the smithy-rs repo
val generateAwsSdkRustLockfile = tasks.register<Exec>("generateAwsSdkRustLockfile") {
    val sdkRustPath: String =
        properties.get("aws-sdk-rust-path") ?: throw Exception("A -Paws-sdk-rust-path argument must be specified")
    workingDir(sdkRustPath)
    environment("RUSTFLAGS", "--cfg aws_sdk_unstable")
    commandLine("cargo", "generate-lockfile")
    copy {
        from("${sdkRustPath}/Cargo.lock")
        into(rootProject.projectDir.resolve("aws/sdk"))
    }
}

//Parent task to generate all the Cargo.lock files
tasks.register("generateAllLockfiles") {
    description =
        "Create Cargo.lock files for aws-config, aws/rust-runtime, rust-runtime, and the workspace created by" +
            "the assemble task."
    finalizedBy(
        generateAwsSdkRustLockfile,
        generateAwsConfigLockfile,
        generateAwsRuntimeLockfile,
        generateSmithytRuntimeLockfile,
    )
}

tasks.register<Delete>("deleteSdk") {
    delete(
        fileTree(outputDir) {
            // Delete files but keep directories so that terminals don't get messed up in local development
            include("**/*.*")
        },
    )
}
tasks["clean"].dependsOn("deleteSdk")
tasks["clean"].doFirst {
    delete(layout.buildDirectory.file("smithy-build.json"))
}
