/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.testutil

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructSettings
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.testutil.TestModuleDocProvider
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.server.smithy.RustServerCodegenPlugin
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenConfig
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerModuleProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustSettings
import software.amazon.smithy.rust.codegen.server.smithy.ServerSymbolProviders
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.customize.CombinedServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerProtocolLoader

/**
 * Server-specific test RuntimeConfig that uses the same HTTP version default as production server code.
 *
 * This is used for server unit tests to ensure they test the default HTTP version behavior.
 * Integration tests using serverIntegrationTest() can override this via additionalSettings.
 *
 * The HTTP version is derived from ServerCodegenConfig.DEFAULT_HTTP_1X to maintain a single source of truth.
 *
 * Note: Client tests use TestRuntimeConfig which defaults to HTTP 1.x.
 */
val ServerTestRuntimeConfig =
    RuntimeConfig(
        runtimeCrateLocation = TestRuntimeConfig.runtimeCrateLocation,
        httpVersion = if (ServerCodegenConfig.DEFAULT_HTTP_1X) HttpVersion.Http1x else HttpVersion.Http0x,
    )

// These are the settings we default to if the user does not override them in their `smithy-build.json`.
val ServerTestRustSymbolProviderConfig =
    RustSymbolProviderConfig(
        runtimeConfig = ServerTestRuntimeConfig,
        renameExceptions = false,
        nullabilityCheckMode = NullableIndex.CheckMode.SERVER,
        moduleProvider = ServerModuleProvider,
    )

private fun testServiceShapeFor(model: Model) =
    model.serviceShapes.firstOrNull() ?: ServiceShape.builder().version("test").id("test#Service").build()

fun serverTestSymbolProvider(
    model: Model,
    serviceShape: ServiceShape? = null,
) = serverTestSymbolProviders(model, serviceShape).symbolProvider

fun serverTestSymbolProviders(
    model: Model,
    serviceShape: ServiceShape? = null,
    settings: ServerRustSettings? = null,
    decorators: List<ServerCodegenDecorator> = emptyList(),
) = ServerSymbolProviders.from(
    serverTestRustSettings(),
    model,
    serviceShape ?: testServiceShapeFor(model),
    ServerTestRustSymbolProviderConfig,
    (
        settings ?: serverTestRustSettings(
            (serviceShape ?: testServiceShapeFor(model)).id,
        )
    ).codegenConfig.publicConstrainedTypes,
    CombinedServerCodegenDecorator(decorators),
    RustServerCodegenPlugin::baseSymbolProvider,
)

fun serverTestRustSettings(
    service: ShapeId = ShapeId.from("notrelevant#notrelevant"),
    moduleName: String = "test-module",
    moduleVersion: String = "0.0.1",
    moduleAuthors: List<String> = listOf("notrelevant"),
    moduleDescription: String = "not relevant",
    moduleRepository: String? = null,
    runtimeConfig: RuntimeConfig = TestRuntimeConfig,
    codegenConfig: ServerCodegenConfig = ServerCodegenConfig(),
    license: String? = null,
    examplesUri: String? = null,
    minimumSupportedRustVersion: String? = null,
    customizationConfig: ObjectNode? = null,
) = ServerRustSettings(
    service,
    moduleName,
    moduleVersion,
    moduleAuthors,
    moduleDescription,
    moduleRepository,
    runtimeConfig,
    codegenConfig,
    license,
    examplesUri,
    minimumSupportedRustVersion,
    customizationConfig,
)

fun serverTestCodegenContext(
    model: Model,
    serviceShape: ServiceShape? = null,
    settings: ServerRustSettings = serverTestRustSettings(),
    protocolShapeId: ShapeId? = null,
    decorators: List<ServerCodegenDecorator> = emptyList(),
): ServerCodegenContext {
    val service = serviceShape ?: testServiceShapeFor(model)
    val protocol = protocolShapeId ?: ShapeId.from("test#Protocol")
    val serverSymbolProviders =
        ServerSymbolProviders.from(
            settings,
            model,
            service,
            ServerTestRustSymbolProviderConfig,
            settings.codegenConfig.publicConstrainedTypes,
            CombinedServerCodegenDecorator(decorators),
            RustServerCodegenPlugin::baseSymbolProvider,
        )

    return ServerCodegenContext(
        model,
        serverSymbolProviders.symbolProvider,
        TestModuleDocProvider,
        service,
        protocol,
        settings,
        serverSymbolProviders.unconstrainedShapeSymbolProvider,
        serverSymbolProviders.constrainedShapeSymbolProvider,
        serverSymbolProviders.constraintViolationSymbolProvider,
        serverSymbolProviders.pubCrateConstrainedShapeSymbolProvider,
    )
}

fun loadServerProtocol(model: Model): ServerProtocol {
    val codegenContext = serverTestCodegenContext(model)
    val (_, protocolGeneratorFactory) =
        ServerProtocolLoader(ServerProtocolLoader.DefaultProtocols).protocolFor(model, codegenContext.serviceShape)
    return protocolGeneratorFactory.buildProtocolGenerator(codegenContext).protocol
}

/**
 * In tests, we frequently need to generate a struct, a builder, and an impl block to access said builder.
 */
fun StructureShape.serverRenderWithModelBuilder(
    rustCrate: RustCrate,
    model: Model,
    symbolProvider: RustSymbolProvider,
    writer: RustWriter,
    protocol: ServerProtocol? = null,
) {
    StructureGenerator(model, symbolProvider, writer, this, emptyList(), StructSettings(false)).render()
    val serverCodegenContext = serverTestCodegenContext(model)
    // Note that this always uses `ServerBuilderGenerator` and _not_ `ServerBuilderGeneratorWithoutPublicConstrainedTypes`,
    // regardless of the `publicConstrainedTypes` setting.
    val modelBuilder =
        ServerBuilderGenerator(
            serverCodegenContext,
            this,
            SmithyValidationExceptionConversionGenerator(serverCodegenContext),
            protocol ?: loadServerProtocol(model),
        )
    modelBuilder.render(rustCrate, writer)
    writer.implBlock(symbolProvider.toSymbol(this)) {
        modelBuilder.renderConvenienceMethod(this)
    }
}
