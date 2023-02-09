/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.testutil

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.build.SmithyBuildPlugin
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.codegenIntegrationTest
import software.amazon.smithy.rust.codegen.server.smithy.RustServerCodegenPlugin
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import java.nio.file.Path

/**
 * This file is entirely analogous to [software.amazon.smithy.rust.codegen.client.testutil.ClientCodegenIntegrationTest.kt].
 */

fun serverIntegrationTest(
    model: Model,
    params: IntegrationTestParams = IntegrationTestParams(),
    additionalDecorators: List<ServerCodegenDecorator> = listOf(),
    test: (ServerCodegenContext, RustCrate) -> Unit = { _, _ -> },
): Path {
    fun invokeRustCodegenPlugin(ctx: PluginContext) {
        val codegenDecorator = object : ServerCodegenDecorator {
            override val name: String = "Add tests"
            override val order: Byte = 0

            override fun classpathDiscoverable(): Boolean = false

            override fun extras(codegenContext: ServerCodegenContext, rustCrate: RustCrate) {
                test(codegenContext, rustCrate)
            }
        }
        RustServerCodegenPlugin().executeWithDecorator(ctx, codegenDecorator, *additionalDecorators.toTypedArray())
    }
    return codegenIntegrationTest(model, params, invokePlugin = ::invokeRustCodegenPlugin)
}

abstract class ServerDecoratableBuildPlugin : SmithyBuildPlugin {
    abstract fun executeWithDecorator(
        context: PluginContext,
        vararg decorator: ServerCodegenDecorator,
    )

    override fun execute(context: PluginContext) {
        executeWithDecorator(context)
    }
}
