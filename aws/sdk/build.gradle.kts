/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.traits.TitleTrait
import java.util.Properties
import kotlin.streams.toList

extra["displayName"] = "Smithy :: Rust :: AWS-SDK"
extra["moduleName"] = "software.amazon.smithy.rust.awssdk"

tasks["jar"].enabled = false

plugins {
    id("software.amazon.smithy").version("0.5.3")
}

val smithyVersion: String by project

val sdkOutputDir = buildDir.resolve("aws-sdk")
val runtimeModules = listOf(
    "smithy-async",
    "smithy-client",
    "smithy-eventstream",
    "smithy-http",
    "smithy-http-tower",
    "smithy-json",
    "smithy-protocol-test",
    "smithy-query",
    "smithy-types",
    "smithy-xml"
)
val awsModules = listOf(
    "aws-auth",
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
data class Membership(val inclusions: Set<String> = emptySet(), val exclusions: Set<String> = emptySet())

fun Membership.isMember(member: String): Boolean = when {
    exclusions.contains(member) -> false
    inclusions.contains(member) -> true
    inclusions.isEmpty() -> true
    else -> false
}

fun parseMembership(rawList: String): Membership {
    val inclusions = mutableSetOf<String>()
    val exclusions = mutableSetOf<String>()

    rawList.split(",").map { it.trim() }.forEach { item ->
        when {
            item.startsWith('-') -> exclusions.add(item.substring(1))
            item.startsWith('+') -> inclusions.add(item.substring(1))
            else -> error("Must specify inclusion (+) or exclusion (-) prefix character to $item.")
        }
    }

    val conflictingMembers = inclusions.intersect(exclusions)
    require(conflictingMembers.isEmpty()) { "$conflictingMembers specified both for inclusion and exclusion in $rawList" }

    return Membership(inclusions, exclusions)
}

data class AwsService(
    val service: String,
    val module: String,
    val moduleDescription: String,
    val modelFile: File,
    val extraConfig: String? = null,
    val extraFiles: List<File>
) {
    fun files(): List<File> = listOf(modelFile) + extraFiles
}

val awsServices: List<AwsService> by lazy { discoverServices() }
val eventStreamAllowList: Set<String> by lazy { eventStreamAllowList() }

fun loadServiceMembership(): Membership {
    val membershipOverride = getProperty("aws.services")?.let { parseMembership(it) }
    println(membershipOverride)
    val fullSdk = parseMembership(getProperty("aws.services.fullsdk") ?: throw kotlin.Exception("never list missing"))
    val tier1 = parseMembership(getProperty("aws.services.smoketest") ?: throw kotlin.Exception("smoketest list missing"))
    return membershipOverride ?: if ((getProperty("aws.fullsdk") ?: "") == "true") {
        fullSdk
    } else {
        tier1
    }
}

/**
 * Discovers services from the `models` directory
 *
 * Do not invoke this function directly. Use the `awsServices` provider.
 */
fun discoverServices(): List<AwsService> {
    val models = project.file("aws-models")
    val serviceMembership = loadServiceMembership()
    val baseServices = fileTree(models)
        .sortedBy { file -> file.name }
        .mapNotNull { file ->
            val model = Model.assembler().addImport(file.absolutePath).assemble().result.get()
            val services: List<ServiceShape> = model.shapes(ServiceShape::class.java).sorted().toList()
            if (services.size > 1) {
                throw Exception("There must be exactly one service in each aws model file")
            }
            if (services.isEmpty()) {
                logger.info("${file.name} has no services")
                null
            } else {
                val service = services[0]
                val title = service.expectTrait(TitleTrait::class.java).value
                val sdkId = service.expectTrait(ServiceTrait::class.java).sdkId
                    .toLowerCase()
                    .replace(" ", "")
                    // TODO: the smithy models should not include the suffix "service"
                    .removeSuffix("service")
                    .removeSuffix("api")
                val testFile = file.parentFile.resolve("$sdkId-tests.smithy")
                val extras = if (testFile.exists()) {
                    logger.warn("Discovered protocol tests for ${file.name}")
                    listOf(testFile)
                } else {
                    listOf()
                }
                AwsService(
                    service = service.id.toString(),
                    module = sdkId,
                    moduleDescription = "AWS SDK for $title",
                    modelFile = file,
                    extraFiles = extras
                )
            }
        }
    val baseModules = baseServices.map { it.module }.toSet()

    // validate the full exclusion list hits
    serviceMembership.exclusions.forEach { disabledService ->
        check(baseModules.contains(disabledService)) {
            "Service $disabledService was explicitly disabled but no service was generated with that name. Generated:\n ${
            baseModules.joinToString(
                "\n "
            )
            }"
        }
    }
    // validate inclusion list hits
    serviceMembership.inclusions.forEach { service ->
        check(baseModules.contains(service)) { "Service $service was in explicit inclusion list but not generated!" }
    }
    return baseServices.filter {
        serviceMembership.isMember(it.module)
    }
}

fun eventStreamAllowList(): Set<String> {
    val list = getProperty("aws.services.eventstream.allowlist") ?: ""
    return list.split(",").map { it.trim() }.toSet()
}

fun generateSmithyBuild(services: List<AwsService>): String {
    val projections = services.joinToString(",\n") { service ->
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
                            "relativePath": "../"
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
                        "moduleRepository": "https://github.com/awslabs/aws-sdk-rust",
                        "license": "Apache-2.0"
                        ${service.extraConfig ?: ""}
                    }
                }
            }
        """.trimIndent()
    }
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
            into(sdkOutputDir)
            exclude("**/target")
            filter { line -> line.replace("build/aws-sdk/", "") }
        }
    }
    inputs.dir(projectDir.resolve("examples"))
    outputs.dir(sdkOutputDir)
}

/**
 * The aws/rust-runtime crates depend on local versions of the Smithy core runtime enabling local compilation. However,
 * those paths need to be replaced in the final build. We should probably fix this with some symlinking.
 */
fun rewritePathDependency(line: String): String {
    // some runtime crates are actually dependent on the generated bindings:
    return line.replace("../sdk/build/aws-sdk/", "")
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
fun patchFile(path: String, operation: (String) -> String) {
    val patchedContents = File(path).readLines().joinToString("\n", transform = operation)
    File(path).writeText(patchedContents)
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
            patchFile("$sdkOutputDir/$moduleName/Cargo.toml") { line ->
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
            patchFile("$sdkOutputDir/$moduleName/Cargo.toml") { line ->
                line.let(::rewriteSmithyRsCrateVersion)
            }
        }
    }
}

fun generateCargoWorkspace(services: List<AwsService>): String {
    val generatedModules = services.map { it.module }.toSet()
    val examples = projectDir.resolve("examples")
        .listFiles { file -> !file.name.startsWith(".") }.orEmpty().toList()
        .filter { generatedModules.contains(it.name) }
        .map { "examples/${it.name}" }

    val modules = services.map(AwsService::module) + runtimeModules + awsModules + examples.toList()
    return """
    [workspace]
    members = [
        ${modules.joinToString(",") { "\"$it\"" }}
    ]
    """.trimIndent()
}
task("generateCargoWorkspace") {
    description = "generate Cargo.toml workspace file"
    doFirst {
        sdkOutputDir.mkdirs()
        sdkOutputDir.resolve("Cargo.toml").writeText(generateCargoWorkspace(awsServices))
    }
    inputs.property("servicelist", awsServices.sortedBy { it.module }.toString())
    inputs.dir(projectDir.resolve("examples"))
    outputs.file(sdkOutputDir.resolve("Cargo.toml"))
    outputs.upToDateWhen { false }
}

task("finalizeSdk") {
    dependsOn("assemble")
    outputs.upToDateWhen { false }
    finalizedBy(
        "relocateServices",
        "relocateRuntime",
        "relocateAwsRuntime",
        "relocateExamples"
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
    workingDir(sdkOutputDir)
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "check", "--lib", "--tests", "--benches")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoTest") {
    workingDir(sdkOutputDir)
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "test")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoDocs") {
    workingDir(sdkOutputDir)
    // disallow warnings
    environment("RUSTDOCFLAGS", "-D warnings")
    commandLine("cargo", "doc", "--no-deps", "--document-private-items")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoClippy") {
    workingDir(sdkOutputDir)
    // disallow warnings
    commandLine("cargo", "clippy", "--", "-D", "warnings")
    dependsOn("assemble")
}

tasks.register<RunExampleTask>("runExample") {
    dependsOn("assemble")
    outputDir = sdkOutputDir
}

// TODO: validate that the example exists. Otherwise this fails with a hiden error.
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
