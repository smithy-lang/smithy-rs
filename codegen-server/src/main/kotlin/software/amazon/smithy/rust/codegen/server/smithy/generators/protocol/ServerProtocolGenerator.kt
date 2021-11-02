/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.MakeOperationGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolTraitImplGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol

/**
 * Class providing scaffolding for HTTP based protocols that must build an HTTP request (headers / URL) and a body.
 */
open class ServerProtocolGenerator(
    codegenContext: CodegenContext,
    /**
     * `Protocol` contains all protocol specific information. Each smithy protocol, eg. RestJson, RestXml, etc. will
     * have their own implementation of the protocol interface which defines how an input shape becomes and http::Request
     * and an output shape is build from an http::Response.
     */
    private val protocol: Protocol,
    /**
     * Operations generate a `make_operation(&config)` method to build a `aws_smithy_http::Operation` that can be dispatch.
     * While this is not run inside the server codegen, it has to be here to obey to the interface constraints.
     */
    private val makeOperationGenerator: MakeOperationGenerator,
    /**
     * Operations generate implementations of ParseHttpRequest, SerializeHttpResponse and SerializeHttpError.
     */
    private val traitGenerator: ProtocolTraitImplGenerator,
) : ProtocolGenerator(codegenContext, protocol, makeOperationGenerator, traitGenerator) {
    private val symbolProvider = codegenContext.symbolProvider

    /**
     * Render all code required for serializing responses and deserializing requests for the operation
     *
     * This primarily relies on the [traitGenerator] to generate implementations of the `ParseHttpRequest`,
     * `SerializeHttpResponse` and `SerializeHttpError` traits for the operations
     */
    fun serverRenderOperation(
        operationWriter: RustWriter,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>
    ) {
        // impl OperationInputShape { ... }
        val operationName = symbolProvider.toSymbol(operationShape).name
        // pub struct Operation { ... }
        operationWriter.rust(
            """
            /// Operation shape for `$operationName`.
            """
        )
        Attribute.Derives(setOf(RuntimeType.Clone, RuntimeType.Default, RuntimeType.Debug)).render(operationWriter)
        operationWriter.rustBlock("pub struct $operationName") {
            write("_private: ()")
        }
        operationWriter.implBlock(operationShape, symbolProvider) {
            rust("/// Creates a new `$operationName` operation.")
            rustBlock("pub fn new() -> Self") {
                rust("Self { _private: () }")
            }

            writeCustomizations(customizations, OperationSection.OperationImplBlock(customizations))
        }
        // Render all operation traits
        traitGenerator.generateTraitImpls(operationWriter, operationShape)
    }
}
