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

/**
 * A helper class holding common data with defaults that is threaded through several functions, to make their
 * signatures shorter.
 */
data class IntegrationTestParams(
    val addModuleToEventStreamAllowList: Boolean = false,
    val service: String? = null,
    val runtimeConfig: RuntimeConfig? = null,
    val additionalSettings: ObjectNode = ObjectNode.builder().build(),
    val overrideTestDir: File? = null,
    val command: ((Path) -> Unit)? = null,
    val cargoCommand: String? = null,
)

/**
 * Run cargo test on a true, end-to-end, codegen product of a given model.
 */
fun codegenIntegrationTest(model: Model, params: IntegrationTestParams, invokePlugin: (PluginContext) -> Unit): Path {
    val (ctx, testDir) = generatePluginContext(
        model,
        params.additionalSettings,
        params.addModuleToEventStreamAllowList,
        params.service,
        params.runtimeConfig,
        params.overrideTestDir,
    )
    invokePlugin(ctx)
    ctx.fileManifest.printGeneratedFiles()
    params.command?.invoke(testDir) ?: (params.cargoCommand ?: "cargo test").runCommand(testDir, environment = mapOf("RUSTFLAGS" to "-D warnings"))
    return testDir
}
