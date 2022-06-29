/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.docLink
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.client.FluentClientGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.util.inputShape

/**
 * Payload Body Generator.
 *
 * Used to generate payloads that will go into HTTP bodies for HTTP requests (used by clients)
 * and responses (used by servers).
 *
 * **Note:** There is only one real implementation of this interface. The other implementation is test only.
 * All protocols use the same class.
 *
 * Different protocols (e.g. JSON vs. XML) need to use different functionality to generate payload bodies.
 */
interface ProtocolPayloadGenerator {
    data class PayloadMetadata(val takesOwnership: Boolean)

    /**
     * Code generation needs to handle whether or not [generatePayload] takes ownership of the input or output
     * for a given operation shape.
     *
     * Most operations will use the HTTP payload as a reference, but for operations that will consume the entire stream
     * later,they will need to take ownership and different code needs to be generated.
     */
    fun payloadMetadata(operationShape: OperationShape): PayloadMetadata

    /**
     * Write the payload into [writer].
     *
     * [self] is the name of the variable binding for the Rust struct that is to be serialized into the payload.
     *
     * This should be an expression that returns bytes:
     *     - a `Vec<u8>` for non-streaming operations; or
     *     - a `ByteStream` for streaming operations.
     */
    fun generatePayload(writer: RustWriter, self: String, operationShape: OperationShape)
}

/**
 * Protocol Trait implementation generator
 *
 * **Note:** There is only one real implementation of this interface. The other implementation is test only.
 * All protocols use the same class.
 *
 * Protocols implement one of two traits to enable parsing HTTP responses:
 * 1. `ParseHttpResponse`: Streaming binary operations
 * 2. `ParseStrictResponse`: Non-streaming operations for the body must be "strict" (as in, not lazy) where the parser
 *                           must have the complete body to return a result.
 */
interface ProtocolTraitImplGenerator {
    fun generateTraitImpls(operationWriter: RustWriter, operationShape: OperationShape)
}

/**
 * Class providing scaffolding for HTTP based protocols that must build an HTTP request (headers / URL) and a body.
 */
open class ProtocolGenerator(
    coreCodegenContext: CoreCodegenContext,
    /**
     * `Protocol` contains all protocol specific information. Each smithy protocol, e.g. RestJson, RestXml, etc. will
     * have their own implementation of the protocol interface which defines how an input shape becomes and http::Request
     * and an output shape is build from an http::Response.
     */
    private val protocol: Protocol,
    /**
     * Operations generate a `make_operation(&config)` method to build a `aws_smithy_http::Operation` that can be dispatched
     * This is the serializer side of request dispatch
     */
    private val makeOperationGenerator: MakeOperationGenerator,
    /**
     * Operations generate implementations of ParseHttpResponse or ParseStrictResponse.
     * This is the deserializer side of request dispatch (parsing the response)
     */
    private val traitGenerator: ProtocolTraitImplGenerator,
) {
    private val symbolProvider = coreCodegenContext.symbolProvider
    private val model = coreCodegenContext.model

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
        customizations: List<OperationCustomization>
    ) {
        val inputShape = operationShape.inputShape(model)
        val builderGenerator = BuilderGenerator(model, symbolProvider, operationShape.inputShape(model))
        builderGenerator.render(inputWriter)

        // generate type aliases for the fluent builders
        renderTypeAliases(inputWriter, operationShape, customizations, inputShape)

        // impl OperationInputShape { ... }
        val operationName = symbolProvider.toSymbol(operationShape).name
        inputWriter.implBlock(inputShape, symbolProvider) {
            writeCustomizations(
                customizations,
                OperationSection.InputImpl(customizations, operationShape, inputShape, protocol)
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
            """
        )
        Attribute.Derives(setOf(RuntimeType.Clone, RuntimeType.Default, RuntimeType.Debug)).render(operationWriter)
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
        traitGenerator.generateTraitImpls(operationWriter, operationShape)
    }

    /**
     * The server implementation uses this method to generate implementations of the `FromRequest` and `IntoResponse`
     * traits for operation input and output shapes, respectively.
     */
    fun serverRenderOperation(
        operationWriter: RustWriter,
        operationShape: OperationShape,
    ) {
        traitGenerator.generateTraitImpls(operationWriter, operationShape)
    }

    private fun renderTypeAliases(
        inputWriter: RustWriter,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
        inputShape: StructureShape
    ) {
        // TODO(https://github.com/awslabs/smithy-rs/issues/976): Callers should be able to invoke
        // buildOperationType* directly to get the type rather than depending on these aliases.
        // These are used in fluent clients.
        val operationTypeOutput = buildOperationTypeOutput(inputWriter, operationShape)
        val operationTypeRetry = buildOperationTypeRetry(inputWriter, customizations)
        val inputPrefix = symbolProvider.toSymbol(inputShape).name

        inputWriter.rust(
            """
            ##[doc(hidden)] pub type ${inputPrefix}OperationOutputAlias = $operationTypeOutput;
            ##[doc(hidden)] pub type ${inputPrefix}OperationRetryAlias = $operationTypeRetry;
            """
        )
    }

    private fun buildOperationTypeOutput(writer: RustWriter, shape: OperationShape): String =
        writer.format(symbolProvider.toSymbol(shape))

    private fun buildOperationTypeRetry(writer: RustWriter, customizations: List<OperationCustomization>): String =
        customizations.mapNotNull { it.retryType() }.firstOrNull()?.let { writer.format(it) } ?: "()"
}
