/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.docLink
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolTraitImplGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.util.inputShape

open class ClientProtocolGenerator(
    codegenContext: CodegenContext,
    private val protocol: Protocol,
    /**
     * Operations generate a `make_operation(&config)` method to build a `aws_smithy_http::Operation` that can be dispatched
     * This is the serializer side of request dispatch
     */
    private val makeOperationGenerator: MakeOperationGenerator,
    private val traitGenerator: ProtocolTraitImplGenerator,
) : ProtocolGenerator(codegenContext, protocol, traitGenerator) {
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
        inputWriter: RustWriter,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val inputShape = operationShape.inputShape(model)
        val builderGenerator = BuilderGenerator(model, symbolProvider, operationShape.inputShape(model))
        builderGenerator.render(inputWriter)

        // impl OperationInputShape { ... }
        val operationName = symbolProvider.toSymbol(operationShape).name
        inputWriter.implBlock(inputShape, symbolProvider) {
            writeCustomizations(
                customizations,
                OperationSection.InputImpl(customizations, operationShape, inputShape, protocol),
            )
            makeOperationGenerator.generateMakeOperation(this, operationShape, customizations)

            // pub fn builder() -> ... { }
            builderGenerator.renderConvenienceMethod(this)
        }

        // pub struct Operation { ... }
        val fluentBuilderName = FluentClientGenerator.clientOperationFnName(operationShape, symbolProvider)
        operationWriter.rust(
            """
            /// Operation shape for `$operationName`.
            ///
            /// This is usually constructed for you using the the fluent builder returned by
            /// [`$fluentBuilderName`](${docLink("crate::client::Client::$fluentBuilderName")}).
            ///
            /// See [`crate::client::fluent_builders::$operationName`] for more details about the operation.
            """,
        )
        Attribute(derive(RuntimeType.Clone, RuntimeType.Default, RuntimeType.Debug)).render(operationWriter)
        operationWriter.rustBlock("pub struct $operationName") {
            write("_private: ()")
        }
        operationWriter.implBlock(operationShape, symbolProvider) {
            builderGenerator.renderConvenienceMethod(this)

            rust("/// Creates a new `$operationName` operation.")
            rustBlock("pub fn new() -> Self") {
                rust("Self { _private: () }")
            }

            writeCustomizations(customizations, OperationSection.OperationImplBlock(customizations))
        }
        traitGenerator.generateTraitImpls(operationWriter, operationShape, customizations)
    }
}
