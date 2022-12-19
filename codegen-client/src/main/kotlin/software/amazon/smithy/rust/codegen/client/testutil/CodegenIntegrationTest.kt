/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.testutil

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.build.SmithyBuildPlugin
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.RustClientCodegenPlugin
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.core.testutil.printGeneratedFiles
import software.amazon.smithy.rust.codegen.core.util.runCommand
import java.io.File
import java.nio.file.Path

/**
 * Run cargo test on a true, end-to-end, codegen product of a given model.
 *
 * For test purposes, additional codegen decorators can also be composed.
 */
fun clientIntegrationTest(
    model: Model,
    additionalDecorators: List<ClientCodegenDecorator> = listOf(),
    addModuleToEventStreamAllowList: Boolean = false,
    service: String? = null,
    runtimeConfig: RuntimeConfig? = null,
    additionalSettings: ObjectNode = ObjectNode.builder().build(),
    command: ((Path) -> Unit)? = null,
    test: (ClientCodegenContext, RustCrate) -> Unit = { _, _ -> },
): Path {
    return codegenIntegrationTest(
        model,
        RustClientCodegenPlugin(),
        additionalDecorators,
        addModuleToEventStreamAllowList = addModuleToEventStreamAllowList,
        service = service,
        runtimeConfig = runtimeConfig,
        additionalSettings = additionalSettings,
        test = test,
        command = command,
    )
}

/**
 * A Smithy BuildPlugin that accepts an additional decorator
 *
 * This exists to allow tests to easily customize the _real_ build without needing to list out customizations
 * or attempt to manually discover them from the path
 */
abstract class DecoratableBuildPlugin : SmithyBuildPlugin {
    abstract fun executeWithDecorator(
        context: PluginContext,
        vararg decorator: ClientCodegenDecorator,
    )

    override fun execute(context: PluginContext) {
        executeWithDecorator(context)
    }
}

// TODO(https://github.com/awslabs/smithy-rs/issues/1864): move to core once CodegenDecorator is in core
private fun codegenIntegrationTest(
    model: Model,
    buildPlugin: DecoratableBuildPlugin,
    additionalDecorators: List<ClientCodegenDecorator>,
    additionalSettings: ObjectNode = ObjectNode.builder().build(),
    addModuleToEventStreamAllowList: Boolean = false,
    service: String? = null,
    runtimeConfig: RuntimeConfig? = null,
    overrideTestDir: File? = null, test: (ClientCodegenContext, RustCrate) -> Unit,
    command: ((Path) -> Unit)? = null,
): Path {
    val (ctx, testDir) = generatePluginContext(
        model,
        additionalSettings,
        addModuleToEventStreamAllowList,
        service,
        runtimeConfig,
        overrideTestDir,
    )

    val codegenDecorator = object : ClientCodegenDecorator {
        override val name: String = "Add tests"
        override val order: Byte = 0

        override fun classpathDiscoverable(): Boolean = false

        override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
            test(codegenContext, rustCrate)
        }
    }
    buildPlugin.executeWithDecorator(ctx, codegenDecorator, *additionalDecorators.toTypedArray())
    ctx.fileManifest.printGeneratedFiles()
    command?.invoke(testDir) ?: "cargo test".runCommand(testDir, environment = mapOf("RUSTFLAGS" to "-D warnings"))
    return testDir
}
