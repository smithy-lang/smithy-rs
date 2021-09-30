/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator

/**
 * Describes a protocol to the [HttpBoundProtocolGenerator].
 */
interface Protocol {
    /** Resolves HTTP bindings (which part of a request fields are mapped to) */
    val httpBindingResolver: HttpBindingResolver

    /** The timestamp format that should be used if no override is specified in the model */
    val defaultTimestampFormat: TimestampFormatTrait.Format

    /** Returns additional HTTP headers that should be included for the given operation for this protocol */
    fun additionalHeaders(operationShape: OperationShape): List<Pair<String, String>> = emptyList()

    /** Returns a deserialization code generator for this protocol */
    fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator

    /** Returns a serialization code generator for this protocol */
    fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator

    /**
     * Generates a function signature like the following:
     * ```rust
     * fn parse_http_generic_error(response: &Response<Bytes>) -> smithy_types::error::Error
     * ```
     */
    fun parseHttpGenericError(operationShape: OperationShape): RuntimeType

    /**
     * Generates a function signature like the following:
     * ```rust
     * fn parse_event_stream_generic_error(payload: &Bytes) -> smithy_types::error::Error
     * ```
     *
     * Event Stream generic errors are almost identical to HTTP generic errors, except that
     * there are no response headers or statuses available to further inform the error parsing.
     */
    fun parseEventStreamGenericError(operationShape: OperationShape): RuntimeType
}
