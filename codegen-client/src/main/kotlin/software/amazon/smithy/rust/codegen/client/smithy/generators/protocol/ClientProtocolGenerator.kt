/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointParamsInterceptorGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationRuntimePluginGenerator
import software.amazon.smithy.rust.codegen.client.smithy.protocols.HttpBoundProtocolTraitImplGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.util.inputShape

open class ClientProtocolGenerator(
    private val codegenContext: ClientCodegenContext,
    private val protocol: Protocol,
    /**
     * Operations generate a `make_operation(&config)` method to build a `aws_smithy_http::Operation` that can be dispatched
     * This is the serializer side of request dispatch
     */
    // TODO(enableNewSmithyRuntime): Remove the `makeOperationGenerator`
    private val makeOperationGenerator: MakeOperationGenerator,
    private val bodyGenerator: ProtocolPayloadGenerator,
    // TODO(enableNewSmithyRuntime): Remove the `traitGenerator`
    private val traitGenerator: HttpBoundProtocolTraitImplGenerator,
) : ProtocolGenerator(codegenContext, protocol) {
    /**
     * Render all code required for serializing requests and deserializing responses for the operation
     *
     * This primarily relies on two components:
     * 1. [traitGenerator]: Generate implementations of the `ParseHttpResponse` trait for the operations
     * 2. [makeOperationGenerator]: Generate the `make_operation()` method which is used to serialize operations
     *    to HTTP requests
     */
    fun renderOperation(
        operationWriter: RustWriter,
        // TODO(enableNewSmithyRuntime): Remove the `inputWriter` since `make_operation` generation is going away
        inputWriter: RustWriter,
        operationShape: OperationShape,
        codegenDecorator: ClientCodegenDecorator,
    ) {
        val operationCustomizations = codegenDecorator.operationCustomizations(codegenContext, operationShape, emptyList())
        val inputShape = operationShape.inputShape(model)

        // impl OperationInputShape { ... }
        inputWriter.implBlock(symbolProvider.toSymbol(inputShape)) {
            writeCustomizations(
                operationCustomizations,
                OperationSection.InputImpl(operationCustomizations, operationShape, inputShape, protocol),
            )
            makeOperationGenerator.generateMakeOperation(this, operationShape, operationCustomizations)
        }

        renderOperationStruct(operationWriter, operationShape, operationCustomizations, codegenDecorator)
    }

    private fun renderOperationStruct(
        operationWriter: RustWriter,
        operationShape: OperationShape,
        operationCustomizations: List<OperationCustomization>,
        codegenDecorator: ClientCodegenDecorator,
    ) {
        val operationName = symbolProvider.toSymbol(operationShape).name

        // pub struct Operation { ... }
        operationWriter.rust(
            """
            /// `ParseStrictResponse` impl for `$operationName`.
            """,
        )
        Attribute(derive(RuntimeType.Clone, RuntimeType.Default, RuntimeType.Debug)).render(operationWriter)
        Attribute.NonExhaustive.render(operationWriter)
        Attribute.DocHidden.render(operationWriter)
        operationWriter.rust("pub struct $operationName;")
        operationWriter.implBlock(symbolProvider.toSymbol(operationShape)) {
            Attribute.DocHidden.render(operationWriter)
            rustBlock("pub fn new() -> Self") {
                rust("Self")
            }

            writeCustomizations(operationCustomizations, OperationSection.OperationImplBlock(operationCustomizations))
        }
        traitGenerator.generateTraitImpls(operationWriter, operationShape, operationCustomizations)

        if (codegenContext.settings.codegenConfig.enableNewSmithyRuntime) {
            OperationRuntimePluginGenerator(codegenContext).render(
                operationWriter,
                operationShape,
                operationName,
                codegenDecorator.operationRuntimePluginCustomizations(codegenContext, operationShape, emptyList()),
            )

            ResponseDeserializerGenerator(codegenContext, protocol)
                .render(operationWriter, operationShape, operationCustomizations)
            RequestSerializerGenerator(codegenContext, protocol, bodyGenerator)
                .render(operationWriter, operationShape, operationCustomizations)

            EndpointParamsInterceptorGenerator(codegenContext)
                .render(operationWriter, operationShape)
        }
    }
}
