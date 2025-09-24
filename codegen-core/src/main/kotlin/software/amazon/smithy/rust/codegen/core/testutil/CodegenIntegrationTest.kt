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
 *             ServerAdditionalSettings()
 *                 .generateCodegenComments(true)
 *                 .publicConstrainedTypes(true)
 *                 .toObjectNode()
 * )),
 * ```
 */
open class AdditionalSettings<T : AdditionalSettings<T>> {
    private val codegenBuilderDelegate =
        lazy {
            ObjectNode.builder()
        }
    private val codegenBuilder: ObjectNode.Builder by codegenBuilderDelegate

    fun build(): ObjectNode {
        return if (codegenBuilderDelegate.isInitialized()) {
            ObjectNode.builder()
                .withMember("codegen", codegenBuilder.build())
                .build()
        } else {
            ObjectNode.builder().build()
        }
    }

    @Suppress("UNCHECKED_CAST")
    open fun generateCodegenComments(debugMode: Boolean = true): T {
        codegenBuilder.withMember("debugMode", debugMode)
        return this as T
    }

    @Suppress("UNCHECKED_CAST")
    protected fun withCodegenMember(
        key: String,
        value: Boolean,
    ): T {
        codegenBuilder.withMember(key, value)
        return this as T
    }

    @Suppress("UNCHECKED_CAST")
    protected fun withCodegenMember(
        key: String,
        value: String,
    ): T {
        codegenBuilder.withMember(key, value)
        return this as T
    }

    @Suppress("UNCHECKED_CAST")
    protected fun withCodegenMember(
        key: String,
        value: Number,
    ): T {
        codegenBuilder.withMember(key, value)
        return this as T
    }
}

class ServerAdditionalSettings : AdditionalSettings<ServerAdditionalSettings>() {
    fun publicConstrainedTypes(enabled: Boolean = true): ServerAdditionalSettings =
        withCodegenMember("publicConstrainedTypes", enabled)

    fun addValidationExceptionToConstrainedOperations(enabled: Boolean = true) =
        withCodegenMember("addValidationExceptionToConstrainedOperations", enabled)

    fun ignoreUnsupportedConstraints() = withCodegenMember("ignoreUnsupportedConstraints", true)
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
    val out = params.command?.invoke(testDir) ?: (params.cargoCommand ?: "cargo test --lib --tests").runCommand(testDir, environment = environment)
    logger.fine(out.toString())
    return testDir
}
