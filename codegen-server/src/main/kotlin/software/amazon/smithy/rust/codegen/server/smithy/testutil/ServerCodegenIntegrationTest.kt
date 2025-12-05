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
import java.util.concurrent.CompletableFuture

/**
 * Specifies which HTTP version(s) to test in [serverIntegrationTest].
 */
sealed class HttpTestType {
    /**
     * Run the test for all HTTP versions: HTTP 0.x and HTTP 1.x.
     * This is the default to ensure comprehensive coverage across both HTTP versions.
     */
    object ALL : HttpTestType()

    /**
     * Run the test only for a specific HTTP version.
     * Use this for tests that are specific to either HTTP 0.x or HTTP 1.x behavior.
     */
    data class Only(val version: HttpTestVersion) : HttpTestType()

    /**
     * Run the test once with whatever http-1x value is provided in the params.
     * The framework will not override the http-1x flag.
     * Use this when you want to explicitly control the http-1x setting or test default behavior.
     */
    object AsConfigured : HttpTestType()
}

/**
 * Represents a generated server with its associated HTTP test version.
 */
data class GeneratedServer(
    val path: Path,
    val httpVersion: HttpTestVersion,
)

/**
 * This file is entirely analogous to [software.amazon.smithy.rust.codegen.client.testutil.ClientCodegenIntegrationTest.kt].
 */

fun serverIntegrationTest(
    model: Model,
    params: IntegrationTestParams = IntegrationTestParams(),
    additionalDecorators: List<ServerCodegenDecorator> = listOf(),
    testCoverage: HttpTestType = HttpTestType.ALL,
    test: (ServerCodegenContext, RustCrate) -> Unit = { _, _ -> },
): List<GeneratedServer> {
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
    if (testCoverage is HttpTestType.AsConfigured) {
        val path = codegenIntegrationTest(model, params, invokePlugin = ::invokeRustCodegenPlugin)
        // Determine version from params - check if http-1x is enabled
        val http1xNode =
            params.additionalSettings
                .expectObjectNode()
                .getMember("codegen")
                .orElse(Node.objectNode())
                .expectObjectNode()
                .getBooleanMember(ServerCodegenConfig.HTTP_1X_CONFIG_KEY)
        val http1xEnabled = http1xNode.map { it.value }.orElse(false)
        val version = if (http1xEnabled) HttpTestVersion.HTTP_1_X else HttpTestVersion.HTTP_0_X
        return listOf(GeneratedServer(path, version))
    }

    // Determine which HTTP versions to test
    val shouldTestHttp0 =
        when (testCoverage) {
            is HttpTestType.ALL -> true
            is HttpTestType.Only -> testCoverage.version == HttpTestVersion.HTTP_0_X
            is HttpTestType.AsConfigured -> false // Already handled above
        }
    val shouldTestHttp1 =
        when (testCoverage) {
            is HttpTestType.ALL -> true
            is HttpTestType.Only -> testCoverage.version == HttpTestVersion.HTTP_1_X
            is HttpTestType.AsConfigured -> false // Already handled above
        }

    // Check if parallel execution is enabled via environment variable
    val runInParallel = System.getenv("PARALLEL_HTTP_TESTS")?.toBoolean() ?: false

    val generatedServers = mutableListOf<GeneratedServer>()
    val errors = mutableListOf<MultiVersionTestFailure.HttpVersionFailure>()

    // Helper function to run test for a specific HTTP version
    val runTestForVersion: (HttpTestVersion, Boolean) -> Result<Path> = { version, http1xEnabled ->
        try {
            // Deep merge the codegen settings to preserve existing keys
            val existingCodegenSettings =
                params.additionalSettings
                    .expectObjectNode()
                    .getMember("codegen")
                    .orElse(Node.objectNode())
                    .expectObjectNode()

            val mergedCodegenSettings =
                existingCodegenSettings.toBuilder()
                    .withMember(ServerCodegenConfig.HTTP_1X_CONFIG_KEY, http1xEnabled)
                    .build()

            val versionParams =
                params.copy(
                    additionalSettings =
                        params.additionalSettings.merge(
                            Node.objectNodeBuilder()
                                .withMember("codegen", mergedCodegenSettings)
                                .build(),
                        ),
                )
            Result.success(codegenIntegrationTest(model, versionParams, invokePlugin = ::invokeRustCodegenPlugin))
        } catch (e: Throwable) {
            Result.failure(e)
        }
    }

    // Helper function to handle test results
    val handleResult: (HttpTestVersion, Result<Path>) -> Unit = { version, result ->
        result.onSuccess { generatedServers.add(GeneratedServer(it, version)) }
            .onFailure { errors.add(MultiVersionTestFailure.HttpVersionFailure(version, it)) }
    }

    if (runInParallel) {
        // Run tests in parallel
        val http0Future =
            if (shouldTestHttp0) {
                CompletableFuture.supplyAsync {
                    runTestForVersion(HttpTestVersion.HTTP_0_X, false)
                }
            } else {
                null
            }

        val http1Future =
            if (shouldTestHttp1) {
                CompletableFuture.supplyAsync {
                    runTestForVersion(HttpTestVersion.HTTP_1_X, true)
                }
            } else {
                null
            }

        // Wait for all futures to complete and collect results
        http0Future?.get()?.let { handleResult(HttpTestVersion.HTTP_0_X, it) }
        http1Future?.get()?.let { handleResult(HttpTestVersion.HTTP_1_X, it) }
    } else {
        // Run tests sequentially (original behavior)
        if (shouldTestHttp0) {
            handleResult(HttpTestVersion.HTTP_0_X, runTestForVersion(HttpTestVersion.HTTP_0_X, false))
        }

        if (shouldTestHttp1) {
            handleResult(HttpTestVersion.HTTP_1_X, runTestForVersion(HttpTestVersion.HTTP_1_X, true))
        }
    }

    // If there were any errors, throw a combined exception with clear version information
    if (errors.isNotEmpty()) {
        throw MultiVersionTestFailure(errors)
    }

    return generatedServers.ifEmpty { throw IllegalStateException("No tests were run") }
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
