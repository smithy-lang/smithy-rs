/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.util.runCommand
import java.io.File
import java.nio.file.Path
import java.util.logging.Logger

/**
 * A helper class holding common data with defaults that is threaded through several functions, to make their
 * signatures shorter.
 */
data class IntegrationTestParams(
    val addModuleToEventStreamAllowList: Boolean = false,
    val service: String? = null,
    val moduleVersion: String = "1.0.0",
    val runtimeConfig: RuntimeConfig? = null,
    val additionalSettings: ObjectNode = ObjectNode.builder().build(),
    val overrideTestDir: File? = null,
    val command: ((Path) -> Unit)? = null,
    val cargoCommand: String? = null,
)

/**
 * A helper class to allow setting `codegen` object keys to be passed for `additionalSettings`
 * field of `IntegrationTestParams`.
 *
 * Usage:
 *
 * ```kotlin
 *         serverIntegrationTest(
 *             model,
 *             IntegrationTestParams(
 *                  additionalSettings = AdditionalSettings.builder()
 *                      .generateCodegenComments()
 *                      .publicConstrainedTypes()
 *                      .toObjectNode()
 *             )),
 * ```
 *
 * Or if there is only one setting:
 *
 * ```kotlin
 *         serverIntegrationTest(
 *             model,
 *             IntegrationTestParams(
 *                  additionalSettings = AdditionalSettings.GenerateCodegenComments(true)
 *                      .toObjectNode()
 *             )),
 * ```
 */
sealed class AdditionalSettings {
    abstract fun toObjectNode(): ObjectNode

    class Builder {
        private val settings = mutableListOf<AdditionalSettings>()

        fun generateCodegenComments(debugMode: Boolean = true): Builder {
            settings.add(GenerateCodegenComments(debugMode))
            return this
        }

        fun publicConstrainedTypes(enabled: Boolean = true): Builder {
            settings.add(PublicConstraintType(enabled))
            return this
        }

        fun build(): AdditionalSettings =
            if (settings.size == 1) settings.first()
            else MergedSettings(settings)

        fun toObjectNode(): ObjectNode = build().toObjectNode()
    }

    data class GenerateCodegenComments(val debugMode: Boolean) : AdditionalSettings() {
        override fun toObjectNode(): ObjectNode =
            ObjectNode.builder().withMember(
                "codegen",
                ObjectNode.builder()
                    .withMember("debugMode", debugMode)
                    .build(),
            ).build()
    }

    data class PublicConstraintType(val enabled: Boolean) : AdditionalSettings() {
        override fun toObjectNode(): ObjectNode =
            ObjectNode.builder().withMember(
                "codegen",
                ObjectNode.builder()
                    .withMember("publicConstrainedTypes", enabled)
                    .build(),
            ).build()
    }

    /**
     * Is used for merging different settings.
     *
     * ```kotlin
     *  val debugSettings = AdditionalSettings.GenerateCodegenComments(true)
     *  val anotherSetting = AdditionalSettings.GenerateCodegenComments(false)
     *  val multiMergedSettings = debugSettings.merge(constraintSettings).merge(anotherSetting)
     * ```
     */
    private data class MergedSettings(val settings: List<AdditionalSettings>) : AdditionalSettings() {
        constructor(vararg settings: AdditionalSettings) : this(settings.toList())

        override fun toObjectNode(): ObjectNode {
            return settings.map { it.toObjectNode() }
                .reduce { acc, next -> acc.merge(next) }
        }
    }

    companion object {
        fun builder() = Builder()
    }
}

/**
 * Run cargo test on a true, end-to-end, codegen product of a given model.
 */
fun codegenIntegrationTest(
    model: Model,
    params: IntegrationTestParams,
    invokePlugin: (PluginContext) -> Unit,
): Path {
    val (ctx, testDir) =
        generatePluginContext(
            model,
            params.additionalSettings,
            params.addModuleToEventStreamAllowList,
            params.moduleVersion,
            params.service,
            params.runtimeConfig,
            params.overrideTestDir,
        )

    testDir.writeDotCargoConfigToml(listOf("--deny", "warnings"))

    invokePlugin(ctx)
    ctx.fileManifest.printGeneratedFiles()
    val logger = Logger.getLogger("CodegenIntegrationTest")
    val out = params.command?.invoke(testDir) ?: (params.cargoCommand ?: "cargo test --lib --tests").runCommand(testDir)
    logger.fine(out.toString())
    return testDir
}
