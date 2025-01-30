/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.ToNode
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
 * A helper class to allow setting `codegen` object keys to be passed to the `additionalSettings`
 * field of `IntegrationTestParams`.
 *
 * Usage:
 *
 * ```kotlin
 * serverIntegrationTest(
 *     model,
 *     IntegrationTestParams(
 *         additionalSettings =
 *             ServerAdditionalSettings.builder()
 *                 .generateCodegenComments()
 *                 .publicConstrainedTypes()
 *                 .toObjectNode()
 * )),
 * ```
 */
sealed class AdditionalSettings {
    abstract fun toObjectNode(): ObjectNode

    companion object {
        private fun Map<String, Any>.toCodegenObjectNode(): ObjectNode =
            ObjectNode.builder()
                .withMember(
                    "codegen",
                    ObjectNode.builder().apply {
                        forEach { (key, value) ->
                            when (value) {
                                is Boolean -> withMember(key, value)
                                is Number -> withMember(key, value)
                                is String -> withMember(key, value)
                                is ToNode -> withMember(key, value)
                                else -> throw IllegalArgumentException("Unsupported type for key $key: ${value::class}")
                            }
                        }
                    }.build(),
                )
                .build()
    }

    abstract class CoreAdditionalSettings protected constructor(
        private val settings: Map<String, Any>,
    ) : AdditionalSettings() {
        override fun toObjectNode(): ObjectNode = settings.toCodegenObjectNode()

        abstract class Builder<T : CoreAdditionalSettings> : AdditionalSettings() {
            protected val settings = mutableMapOf<String, Any>()

            fun generateCodegenComments(debugMode: Boolean = true) =
                apply {
                    settings["debugMode"] = debugMode
                }

            override fun toObjectNode(): ObjectNode = settings.toCodegenObjectNode()
        }
    }
}

class ServerAdditionalSettings private constructor(
    settings: Map<String, Any>,
) : AdditionalSettings.CoreAdditionalSettings(settings) {
    class Builder : CoreAdditionalSettings.Builder<ServerAdditionalSettings>() {
        fun publicConstrainedTypes(enabled: Boolean = true) =
            apply {
                settings["publicConstrainedTypes"] = enabled
            }

        fun addValidationExceptionToConstrainedOperations(enabled: Boolean = true) =
            apply {
                settings["addValidationExceptionToConstrainedOperations"] = enabled
            }

        fun replaceInvalidUtf8(enabled: Boolean = true) =
            apply {
                settings["replaceInvalidUtf8"] = enabled
            }
    }

    companion object {
        fun builder() = Builder()
    }
}

class ClientAdditionalSettings private constructor(
    settings: Map<String, Any>,
) : AdditionalSettings.CoreAdditionalSettings(settings) {
    class Builder : CoreAdditionalSettings.Builder<ClientAdditionalSettings>()

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
    environment: Map<String, String> = mapOf(),
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
    val out = params.command?.invoke(testDir) ?: (params.cargoCommand ?: "cargo test --lib --tests").runCommand(testDir, environment = environment)
    logger.fine(out.toString())
    return testDir
}
