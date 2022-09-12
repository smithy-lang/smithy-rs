/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.testutil

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.server.smithy.RustCodegenServerPlugin
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGenerator
import software.amazon.smithy.rust.codegen.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.PubCrateConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenConfig
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.smithy.ServerRustSettings
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig

// These are the settings we default to if the user does not override them in their `smithy-build.json`.
val ServerTestSymbolVisitorConfig = SymbolVisitorConfig(
    runtimeConfig = TestRuntimeConfig,
    renameExceptions = false,
    handleRustBoxing = true,
    handleRequired = true,
)

fun serverTestSymbolProvider(
    model: Model,
    serviceShape: ServiceShape? = null,
    publicConstrainedTypesEnabled: Boolean = true,
): RustSymbolProvider =
    RustCodegenServerPlugin.baseSymbolProvider(
        model,
        serviceShape ?: ServiceShape.builder().version("test").id("test#Service").build(),
        ServerTestSymbolVisitorConfig,
        publicConstrainedTypes = publicConstrainedTypesEnabled,
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
    customizationConfig,
)

fun serverTestCodegenContext(
    model: Model,
    serviceShape: ServiceShape? = null,
    settings: ServerRustSettings = serverTestRustSettings(),
    protocolShapeId: ShapeId? = null,
): ServerCodegenContext {
    val service =
        serviceShape
            ?: model.serviceShapes.firstOrNull()
            ?: ServiceShape.builder().version("test").id("test#Service").build()
    val symbolProvider = serverTestSymbolProvider(model, serviceShape)
    val unconstrainedShapeSymbolProvider =
        UnconstrainedShapeSymbolProvider(symbolProvider, model, service, settings.codegenConfig.publicConstrainedTypes)
    val constrainedShapeSymbolProvider = serverTestSymbolProvider(model, publicConstrainedTypesEnabled = true)
    val constraintViolationSymbolProvider = ConstraintViolationSymbolProvider(symbolProvider, model, service)
    val protocol = protocolShapeId ?: ShapeId.from("test#Protocol")
    return ServerCodegenContext(
        model,
        symbolProvider,
        service,
        protocol,
        settings,
        unconstrainedShapeSymbolProvider,
        constrainedShapeSymbolProvider,
        constraintViolationSymbolProvider,
    )
}

/**
 * In tests, we frequently need to generate a struct, a builder, and an impl block to access said builder.
 */
fun StructureShape.serverRenderWithModelBuilder(model: Model, symbolProvider: RustSymbolProvider, writer: RustWriter) {
    StructureGenerator(model, symbolProvider, writer, this).render(CodegenTarget.SERVER)
    val serverCodegenContext = serverTestCodegenContext(model)
    val pubCrateConstrainedShapeSymbolProvider =
        PubCrateConstrainedShapeSymbolProvider(symbolProvider, model, serverCodegenContext.serviceShape)
    val modelBuilder = ServerBuilderGenerator(
        serverCodegenContext,
        this,
        pubCrateConstrainedShapeSymbolProvider,
    )
    modelBuilder.render(writer)
    writer.implBlock(this, symbolProvider) {
        modelBuilder.renderConvenienceMethod(this)
    }
}
