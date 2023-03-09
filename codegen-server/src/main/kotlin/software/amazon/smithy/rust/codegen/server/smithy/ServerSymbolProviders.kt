/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator

/**
 * Just a handy class to centralize initialization all the symbol providers required by the server code generators, to
 * make the init blocks of the codegen visitors ([ServerCodegenVisitor] and [PythonServerCodegenVisitor]), and the
 * unit test setup code, shorter and DRYer.
 */
class ServerSymbolProviders private constructor(
    val symbolProvider: RustSymbolProvider,
    val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    val constrainedShapeSymbolProvider: RustSymbolProvider,
    val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider,
    val pubCrateConstrainedShapeSymbolProvider: PubCrateConstrainedShapeSymbolProvider,
) {
    companion object {
        fun from(
            settings: ServerRustSettings,
            model: Model,
            service: ServiceShape,
            rustSymbolProviderConfig: RustSymbolProviderConfig,
            publicConstrainedTypes: Boolean,
            codegenDecorator: ServerCodegenDecorator,
            baseSymbolProviderFactory: (settings: ServerRustSettings, model: Model, service: ServiceShape, rustSymbolProviderConfig: RustSymbolProviderConfig, publicConstrainedTypes: Boolean, includeConstraintShapeProvider: Boolean, codegenDecorator: ServerCodegenDecorator) -> RustSymbolProvider,
        ): ServerSymbolProviders {
            val baseSymbolProvider = baseSymbolProviderFactory(settings, model, service, rustSymbolProviderConfig, publicConstrainedTypes, publicConstrainedTypes, codegenDecorator)
            return ServerSymbolProviders(
                symbolProvider = baseSymbolProvider,
                constrainedShapeSymbolProvider = baseSymbolProviderFactory(
                    settings,
                    model,
                    service,
                    rustSymbolProviderConfig,
                    publicConstrainedTypes,
                    true,
                    codegenDecorator,
                ),
                unconstrainedShapeSymbolProvider = UnconstrainedShapeSymbolProvider(
                    baseSymbolProviderFactory(
                        settings,
                        model,
                        service,
                        rustSymbolProviderConfig,
                        false,
                        false,
                        codegenDecorator,
                    ),
                    publicConstrainedTypes, service,
                ),
                pubCrateConstrainedShapeSymbolProvider = PubCrateConstrainedShapeSymbolProvider(
                    baseSymbolProvider,
                    service,
                ),
                constraintViolationSymbolProvider = ConstraintViolationSymbolProvider(
                    baseSymbolProvider,
                    publicConstrainedTypes,
                    service,
                ),
            )
        }
    }
}
