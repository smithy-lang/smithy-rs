/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.testutil

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.build.SmithyBuildPlugin
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.RustClientCodegenPlugin
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.codegenIntegrationTest
import java.nio.file.Path

fun clientIntegrationTest(
    model: Model,
    params: IntegrationTestParams =
        IntegrationTestParams(cargoCommand = "cargo test --features behavior-version-latest"),
    additionalDecorators: List<ClientCodegenDecorator> = listOf(),
    buildPlugin: ClientDecoratableBuildPlugin = RustClientCodegenPlugin(),
    environment: Map<String, String> = mapOf(),
    test: (ClientCodegenContext, RustCrate) -> Unit = { _, _ -> },
): Path {
    fun invokeRustCodegenPlugin(ctx: PluginContext) {
        val codegenDecorator =
            object : ClientCodegenDecorator {
                override val name: String = "Add tests"
                override val order: Byte = 0

                override fun classpathDiscoverable(): Boolean = false

                override fun extras(
                    codegenContext: ClientCodegenContext,
                    rustCrate: RustCrate,
                ) {
                    test(codegenContext, rustCrate)
                }
            }
        buildPlugin.executeWithDecorator(ctx, codegenDecorator, *additionalDecorators.toTypedArray())
    }
    return codegenIntegrationTest(model, params, invokePlugin = ::invokeRustCodegenPlugin, environment)
}

/**
 * A `SmithyBuildPlugin` that accepts an additional decorator.
 *
 * This exists to allow tests to easily customize the _real_ build without needing to list out customizations
 * or attempt to manually discover them from the path.
 */
abstract class ClientDecoratableBuildPlugin : SmithyBuildPlugin {
    abstract fun executeWithDecorator(
        context: PluginContext,
        vararg decorator: ClientCodegenDecorator,
    )

    override fun execute(context: PluginContext) {
        executeWithDecorator(context)
    }
}
