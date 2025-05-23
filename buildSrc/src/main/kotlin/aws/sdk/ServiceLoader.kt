/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package aws.sdk

import CrateSet
import org.gradle.api.Project
import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.traits.TitleTrait
import java.io.File
import kotlin.streams.toList

data class RootTest(
    val path: File,
    val manifestName: String,
)

class AwsServices(
    private val project: Project,
    services: List<AwsService>,
    val partitionsConfigPath: File,
    val defaultConfigPath: File,
) {
    val services: List<AwsService>
    val moduleNames: Set<String> by lazy { services.map { it.module }.toSortedSet() }

    init {
        this.services = services.sortedBy { it.module }
    }

    val allModules: Set<String> by lazy {
        (
            services.map(AwsService::module).map { "sdk/$it" } +
                CrateSet.AWS_SDK_SMITHY_RUNTIME.map { "sdk/${it.name}" } +
                CrateSet.AWS_SDK_RUNTIME.map { "sdk/${it.name}" }
            // Root tests should not be included since they can't be part of the root Cargo workspace
            // in order to test differences in Cargo features. Examples should not be included either
            // because each example itself is a workspace.
        ).toSortedSet()
    }

    val examples: List<String> by lazy {
        val examplesRoot = project.projectDir.resolve("examples")
        examplesRoot.listFiles { file ->
            !file.name.startsWith(".") && file.isDirectory() && file.resolve("Cargo.toml").exists()
        }.orEmpty().toList().map { "examples/${it.name}" }
    }

    /**
     * Tests in `aws/sdk/integration-tests` that are not named after a service module, and therefore,
     * are not included in a service's `tests/` directory. These are to be included at the SDK root
     * `tests/` directory for inclusion in CI.
     */
    val rootTests: List<RootTest> by lazy {
        project.projectDir.resolve("integration-tests")
            .listFiles { file -> !file.name.startsWith(".") }.orEmpty().toList()
            .filter { file -> !moduleNames.contains(file.name) && manifestCompatibleWithGeneratedServices(file) }
            .map { file -> RootTest(file, "tests/${file.name}") }
    }

    /**
     * Returns true if the Cargo manifest in the given path is compatible with the set of generated services.
     */
    private fun manifestCompatibleWithGeneratedServices(path: File) =
        File(path, "Cargo.toml").let { cargoToml ->
            if (cargoToml.exists()) {
                val usedModules =
                    cargoToml.readLines()
                        .map { line -> line.substringBefore('=').trim() }
                        .filter { line -> line.startsWith("aws-sdk-") }
                        .map { line -> line.substringAfter("aws-sdk-") }
                        .toSet()
                moduleNames.containsAll(usedModules)
            } else {
                false
            }
        }

    /**
     * Returns a sorted set of members included in the workspace.
     */
    fun includedInWorkspace() = allModules

    /**
     * Returns a list of crates excluded from the workspace.
     */
    fun excludedFromWorkspace() = examples + rootTests.map(RootTest::manifestName)
}

/**
 * Discovers services from the `aws-models` directory within the project.
 *
 * Since this function parses all models, it is relatively expensive to call. The result should be cached in a property
 * during build.
 */
fun Project.discoverServices(
    awsModelsPath: String?,
    serviceMembership: Membership,
): AwsServices {
    val models = awsModelsPath?.let { File(it) } ?: project.file("aws-models")
    val extrasDir = project.file("aws-models-extra")
    logger.info("Using model path: $models")
    val files = fileTree(models).sortedBy { file -> file.name }
    logger.info("Discovered potential model files: ${files.map { file -> file.name }}")
    val baseServices =
        files
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
                    val sdkId =
                        service.expectTrait(ServiceTrait::class.java).sdkId
                            .lowercase()
                            .replace(" ", "")
                            // The smithy models should not include the suffix "service" but currently they do
                            .removeSuffix("service")
                            .removeSuffix("api")
                    val testFile = extrasDir.resolve("$sdkId-tests.smithy")
                    val extras =
                        if (testFile.exists()) {
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
                        // Order is important for the versions.toml model hash calculation
                        extraFiles = extras.sorted(),
                        humanName = title,
                    )
                }
            }
    val baseModules = baseServices.map { it.module }.toSet()
    logger.info("Discovered base service modules to generate: $baseModules")

    // validate the full exclusion list hits if the models directory is set
    if (awsModelsPath != null) {
        serviceMembership.exclusions.forEach { disabledService ->
            check(baseModules.contains(disabledService)) {
                "Service $disabledService was explicitly disabled but no service was generated with that name. Generated:\n ${
                    baseModules.joinToString(
                        "\n ",
                    )
                }"
            }
        }
    }

    // validate inclusion list hits
    serviceMembership.inclusions.forEach { service ->
        check(baseModules.contains(service)) { "Service $service was in explicit inclusion list but not generated!" }
    }
    return AwsServices(
        this,
        baseServices.filter {
            serviceMembership.isMember(it.module)
        }.also { services ->
            val moduleNames = services.map { it.module }
            logger.info("Final service module list: $moduleNames")
        },
        models.resolve("sdk-partitions.json"),
        models.resolve("sdk-default-configuration.json"),
    )
}

data class Membership(val inclusions: Set<String> = emptySet(), val exclusions: Set<String> = emptySet())

data class AwsService(
    val service: String,
    val module: String,
    val moduleDescription: String,
    val modelFile: File,
    val extraConfig: String? = null,
    val extraFiles: List<File>,
    val humanName: String,
) {
    fun modelFiles(): List<File> = listOf(modelFile) + extraFiles

    fun Project.examples(): File = projectDir.resolve("examples").resolve(module)

    /**
     * Generate a link to the examples for a given service
     */
    fun examplesUri(project: Project) =
        if (project.examples().exists()) {
            "https://github.com/awslabs/aws-sdk-rust/tree/main/examples/$module"
        } else {
            null
        }
}

fun AwsService.crate(): String = "aws-sdk-$module"

private fun Membership.isMember(member: String): Boolean =
    when {
        exclusions.contains(member) -> false
        inclusions.contains(member) -> true
        inclusions.isEmpty() -> true
        else -> false
    }

fun parseMembership(rawList: String): Membership {
    val inclusions = mutableSetOf<String>()
    val exclusions = mutableSetOf<String>()

    rawList.split(",").map { it.trim() }.filter { it.isNotEmpty() }.forEach { item ->
        when {
            item.startsWith('-') -> exclusions.add(item.substring(1))
            item.startsWith('+') -> inclusions.add(item.substring(1))
            else -> error("Must specify inclusion (+) or exclusion (-) prefix character to $item.")
        }
    }

    val conflictingMembers = inclusions.intersect(exclusions)
    require(conflictingMembers.isEmpty()) {
        "$conflictingMembers specified both for inclusion and exclusion in $rawList"
    }

    return Membership(inclusions, exclusions)
}
