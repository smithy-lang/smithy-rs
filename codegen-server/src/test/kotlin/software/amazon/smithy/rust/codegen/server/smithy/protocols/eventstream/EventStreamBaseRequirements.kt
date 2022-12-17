/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestRequirements
import software.amazon.smithy.rust.codegen.server.smithy.RustCodegenServerPlugin
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerSymbolProviders
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerTestSymbolVisitorConfig
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestRustSettings
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider

abstract class EventStreamBaseRequirements : EventStreamTestRequirements<ServerCodegenContext> {
    override fun createCodegenContext(
        model: Model,
        symbolProvider: RustSymbolProvider,
        serviceShape: ServiceShape,
        protocolShapeId: ShapeId,
        codegenTarget: CodegenTarget,
    ): ServerCodegenContext {
        val settings = serverTestRustSettings()
        val serverSymbolProviders = ServerSymbolProviders.from(
            model,
            serviceShape,
            ServerTestSymbolVisitorConfig,
            settings.codegenConfig.publicConstrainedTypes,
            RustCodegenServerPlugin::baseSymbolProvider,
        )
        return ServerCodegenContext(
            model,
            symbolProvider,
            serviceShape,
            protocolShapeId,
            settings,
            serverSymbolProviders.unconstrainedShapeSymbolProvider,
            serverSymbolProviders.constrainedShapeSymbolProvider,
            serverSymbolProviders.constraintViolationSymbolProvider,
            serverSymbolProviders.pubCrateConstrainedShapeSymbolProvider,
        )
    }

    override fun createSymbolProvider(model: Model): RustSymbolProvider =
        serverTestSymbolProvider(model)
}
