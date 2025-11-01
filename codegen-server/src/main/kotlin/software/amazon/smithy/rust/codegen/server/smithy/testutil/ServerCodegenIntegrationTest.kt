/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.testutil

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.build.SmithyBuildPlugin
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.codegenIntegrationTest
import software.amazon.smithy.rust.codegen.server.smithy.RustServerCodegenPlugin
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenConfig
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import java.nio.file.Path

/**
 * Specifies which HTTP version(s) to test in [serverIntegrationTest].
 */
enum class HttpTestType {
    /**
     * Run the test twice: once with http-1x=false (HTTP 0.x) and once with http-1x=true (HTTP 1.x).
     * This is the default to ensure comprehensive coverage across both HTTP versions.
     */
    BOTH,

    /**
     * Run the test once with http-1x=false (HTTP 0.x only).
     * Use this for tests that are specific to HTTP 0.x behavior.
     */
    HTTP_0_ONLY,

    /**
     * Run the test once with http-1x=true (HTTP 1.x only).
     * Use this for tests that are specific to HTTP 1.x behavior.
     */
    HTTP_1_ONLY,

    /**
     * Run the test once with whatever http-1x value is provided in the params.
     * The framework will not override the http-1x flag.
     * Use this when you want to explicitly control the http-1x setting or test default behavior.
     */
    AS_CONFIGURED
}

/**
 * This file is entirely analogous to [software.amazon.smithy.rust.codegen.client.testutil.ClientCodegenIntegrationTest.kt].
 */

fun serverIntegrationTest(
    model: Model,
    params: IntegrationTestParams = IntegrationTestParams(),
    additionalDecorators: List<ServerCodegenDecorator> = listOf(),
    testCoverage: HttpTestType = HttpTestType.BOTH,
    test: (ServerCodegenContext, RustCrate) -> Unit = { _, _ -> },
): List<Path> {
    fun invokeRustCodegenPlugin(ctx: PluginContext) {
        val codegenDecorator =
            object : ServerCodegenDecorator {
                override val name: String = "Add tests"
                override val order: Byte = 0

                override fun classpathDiscoverable(): Boolean = false

                override fun extras(
                    codegenContext: ServerCodegenContext,
                    rustCrate: RustCrate,
                ) {
                    test(codegenContext, rustCrate)
                }
            }
        RustServerCodegenPlugin().executeWithDecorator(ctx, codegenDecorator, *additionalDecorators.toTypedArray())
    }

    // Handle AS_CONFIGURED case separately - run once without modifying params
    if (testCoverage == HttpTestType.AS_CONFIGURED) {
        return listOf(codegenIntegrationTest(model, params, invokePlugin = ::invokeRustCodegenPlugin))
    }

    // Determine which HTTP versions to test
    val shouldTestHttp0 = testCoverage == HttpTestType.BOTH || testCoverage == HttpTestType.HTTP_0_ONLY
    val shouldTestHttp1 = testCoverage == HttpTestType.BOTH || testCoverage == HttpTestType.HTTP_1_ONLY

    val paths = mutableListOf<Path>()
    val errors = mutableListOf<Pair<String, Throwable>>()

    // Run test for HTTP 0 if needed
    if (shouldTestHttp0) {
        try {
            val http0Params = params.copy(
                additionalSettings = params.additionalSettings.merge(
                    Node.objectNodeBuilder()
                        .withMember(
                            "codegen",
                            Node.objectNodeBuilder()
                                .withMember(ServerCodegenConfig.HTTP_1X_CONFIG_KEY, false)
                                .build()
                        )
                        .build()
                )
            )
            paths.add(codegenIntegrationTest(model, http0Params, invokePlugin = ::invokeRustCodegenPlugin))
        } catch (e: Throwable) {
            errors.add("HTTP 0.x" to e)
        }
    }

    // Run test for HTTP 1 if needed
    if (shouldTestHttp1) {
        try {
            val http1Params = params.copy(
                additionalSettings = params.additionalSettings.merge(
                    Node.objectNodeBuilder()
                        .withMember(
                            "codegen",
                            Node.objectNodeBuilder()
                                .withMember(ServerCodegenConfig.HTTP_1X_CONFIG_KEY, true)
                                .build()
                        )
                        .build()
                )
            )
            paths.add(codegenIntegrationTest(model, http1Params, invokePlugin = ::invokeRustCodegenPlugin))
        } catch (e: Throwable) {
            errors.add("HTTP 1.x" to e)
        }
    }

    // If there were any errors, throw a combined exception with clear version information
    if (errors.isNotEmpty()) {
        val errorMessage = buildString {
            appendLine("Test failed for ${errors.size} HTTP version(s):")
            errors.forEach { (version, error) ->
                appendLine()
                appendLine("=== Failure in $version ===")
                appendLine(error.message ?: error.toString())
            }
        }

        // Throw the first exception with an enhanced message
        val enhancedException = AssertionError(errorMessage, errors.first().second)
        // Add all other exceptions as suppressed
        errors.drop(1).forEach { (_, error) ->
            enhancedException.addSuppressed(error)
        }
        throw enhancedException
    }

    return paths.ifEmpty { throw IllegalStateException("No tests were run") }
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
