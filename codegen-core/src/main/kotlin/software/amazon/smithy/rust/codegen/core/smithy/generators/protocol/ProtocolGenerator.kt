/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol

/**
 * Payload Body Generator.
 *
 * Used to generate payloads that will go into HTTP bodies for HTTP requests (used by clients)
 * and responses (used by servers).
 *
 * **Note:** There is only one real implementation of this interface. The other implementation is test-only.
 * All protocols use the same class.
 *
 * Different protocols (e.g. JSON vs. XML) need to use different functionality to generate payload bodies.
 */
interface ProtocolPayloadGenerator {
    data class PayloadMetadata(val takesOwnership: Boolean)

    /**
     * Code generation needs to handle whether [generatePayload] takes ownership of the input or output
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
 * Class providing scaffolding for HTTP based protocols that must build an HTTP request (headers / URL) and a body.
 */
abstract class ProtocolGenerator(
    codegenContext: CodegenContext,
    /**
     * `Protocol` contains all protocol specific information. Each smithy protocol, e.g. RestJson, RestXml, etc. will
     * have their own implementation of the protocol interface which defines how an input shape becomes and http::Request
     * and an output shape is build from an `http::Response`.
     */
    private val protocol: Protocol,
) {
    protected val symbolProvider = codegenContext.symbolProvider
    protected val model = codegenContext.model
}
