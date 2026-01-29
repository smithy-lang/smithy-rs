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

    abstract class CoreAdditionalSettings protected constructor(val settings: List<AdditionalSettings>) :
        AdditionalSettings() {
            override fun toObjectNode(): ObjectNode {
                val merged =
                    settings.map { it.toObjectNode() }
                        .reduce { acc, next -> acc.merge(next) }

                return ObjectNode.builder()
                    .withMember("codegen", merged)
                    .build()
            }

            abstract class Builder<T : CoreAdditionalSettings> : AdditionalSettings() {
                protected val settings = mutableListOf<AdditionalSettings>()

                fun generateCodegenComments(debugMode: Boolean = true): Builder<T> {
                    settings.add(GenerateCodegenComments(debugMode))
                    return this
                }

                abstract fun build(): T

                override fun toObjectNode(): ObjectNode = build().toObjectNode()
            }

            // Core settings that are common to both Servers and Clients should be defined here.
            data class GenerateCodegenComments(val debugMode: Boolean) : AdditionalSettings() {
                override fun toObjectNode(): ObjectNode =
                    ObjectNode.builder()
                        .withMember("debugMode", debugMode)
                        .build()
            }
        }
}

class ClientAdditionalSettings private constructor(settings: List<AdditionalSettings>) :
    AdditionalSettings.CoreAdditionalSettings(settings) {
        class Builder : CoreAdditionalSettings.Builder<ClientAdditionalSettings>() {
            override fun build(): ClientAdditionalSettings = ClientAdditionalSettings(settings)
        }

        // Additional settings that are specific to client generation should be defined here.

        companion object {
            fun builder() = Builder()
        }
    }

class ServerAdditionalSettings private constructor(settings: List<AdditionalSettings>) :
    AdditionalSettings.CoreAdditionalSettings(settings) {
        class Builder : CoreAdditionalSettings.Builder<ServerAdditionalSettings>() {
            fun publicConstrainedTypes(enabled: Boolean = true): Builder {
                settings.add(PublicConstrainedTypes(enabled))
                return this
            }

            fun addValidationExceptionToConstrainedOperations(enabled: Boolean = true): Builder {
                settings.add(AddValidationExceptionToConstrainedOperations(enabled))
                return this
            }

            fun alwaysSendEventStreamInitialResponse(enabled: Boolean = true): Builder {
                settings.add(AlwaysSendEventStreamInitialResponse(enabled))
                return this
            }

            fun withHttp1x(enabled: Boolean = true): Builder {
                settings.add(Http1x(enabled))
                return this
            }

            override fun build(): ServerAdditionalSettings = ServerAdditionalSettings(settings)
        }

        private data class PublicConstrainedTypes(val enabled: Boolean) : AdditionalSettings() {
            override fun toObjectNode(): ObjectNode =
                ObjectNode.builder()
                    .withMember("publicConstrainedTypes", enabled)
                    .build()
        }

        private data class AddValidationExceptionToConstrainedOperations(val enabled: Boolean) : AdditionalSettings() {
            override fun toObjectNode(): ObjectNode =
                ObjectNode.builder()
                    .withMember("addValidationExceptionToConstrainedOperations", enabled)
                    .build()
        }

        private data class AlwaysSendEventStreamInitialResponse(val enabled: Boolean) : AdditionalSettings() {
            override fun toObjectNode(): ObjectNode =
                ObjectNode.builder()
                    .withMember("alwaysSendEventStreamInitialResponse", enabled)
                    .build()
        }

        private data class Http1x(val enabled: Boolean) : AdditionalSettings() {
            override fun toObjectNode(): ObjectNode =
                ObjectNode.builder()
                    .withMember("http-1x", enabled)
                    .build()
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
    environment: Map<String, String> = mapOf(),
): Path {
    val (ctx, testDir) =
        generatePluginContext(
            model,
            params.additionalSettings,
            params.moduleVersion,
            params.service,
            params.runtimeConfig,
            params.overrideTestDir,
        )

    testDir.writeDotCargoConfigToml(listOf("--deny", "warnings"))

    invokePlugin(ctx)
    ctx.fileManifest.printGeneratedFiles()
    val logger = Logger.getLogger("CodegenIntegrationTest")
    val out =
        params.command?.invoke(testDir) ?: (params.cargoCommand ?: "cargo test --lib --tests").runCommand(
            testDir,
            environment = environment,
        )
    logger.fine(out.toString())
    return testDir
}
