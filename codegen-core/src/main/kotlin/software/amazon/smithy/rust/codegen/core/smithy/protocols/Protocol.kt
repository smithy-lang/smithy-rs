/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator

/**
 * Describes a protocol to the [HttpBoundProtocolGenerator].
 *
 * Each protocol (e.g. RestXml, RestJson, etc.) will provide its own implementation of the [Protocol] interface.
 */
interface Protocol {
    /** Resolves HTTP bindings (which part of a request fields are mapped to) */
    val httpBindingResolver: HttpBindingResolver

    /** The timestamp format that should be used if no override is specified in the model */
    val defaultTimestampFormat: TimestampFormatTrait.Format

    /**
     * Returns the headers that this *protocol itself* requires on every request — its framing.
     *
     * These are a function of the protocol alone, never of the operation or the service, so the
     * schema-serde request path deliberately does **not** emit them: the runtime `ClientProtocol`
     * sets them inside `serialize_request`, which keeps them correct when a customer selects a
     * different protocol at runtime via `Config::builder().protocol(..)`. Emitting them from
     * codegen would both leave them behind when swapping this protocol out and omit them when
     * swapping it in.
     *
     * The legacy (non-schema) request path has no runtime protocol to delegate to, so it emits
     * these itself via [additionalRequestHeaders].
     *
     * Contrast [serviceRequestHeaders], which are emitted by both paths.
     *
     * These MUST all be lowercase, or the application will panic, as per
     * https://docs.rs/http/latest/http/header/struct.HeaderName.html#method.from_static
     */
    fun protocolFramingHeaders(operationShape: OperationShape): List<Pair<String, String>> = emptyList()

    /**
     * Returns additional request headers determined by the *service* — typically by a trait applied
     * to the service shape — rather than by the protocol.
     *
     * Both codegen paths emit these, because their value does not change when the protocol does.
     * `x-amzn-query-mode` is the motivating example: it comes from `@awsQueryCompatible` on the
     * service, and the same service sends it under either awsJson or rpcv2Cbor.
     *
     * These MUST all be lowercase, or the application will panic, as per
     * https://docs.rs/http/latest/http/header/struct.HeaderName.html#method.from_static
     */
    fun serviceRequestHeaders(operationShape: OperationShape): List<Pair<String, String>> = emptyList()

    /**
     * Returns every additional HTTP header that should be included in HTTP requests for the given
     * operation: [protocolFramingHeaders] plus [serviceRequestHeaders].
     *
     * Prefer the two more specific accessors when generating code, so that the runtime protocol
     * remains the single owner of protocol framing. This method exists for callers that need the
     * complete set, such as the legacy request path.
     *
     * These MUST all be lowercase, or the application will panic, as per
     * https://docs.rs/http/latest/http/header/struct.HeaderName.html#method.from_static
     */
    fun additionalRequestHeaders(operationShape: OperationShape): List<Pair<String, String>> =
        protocolFramingHeaders(operationShape) + serviceRequestHeaders(operationShape)

    /**
     * Returns additional HTTP headers that should be included in HTTP responses for the given operation for this protocol.
     *
     * These MUST all be lowercase, or the application will panic, as per
     * https://docs.rs/http/latest/http/header/struct.HeaderName.html#method.from_static
     */
    fun additionalResponseHeaders(operationShape: OperationShape): List<Pair<String, String>> = emptyList()

    /**
     * Returns additional HTTP headers that should be included in HTTP responses for the given error shape.
     * These headers are added to responses _in addition_ to those returned by `additionalResponseHeaders`; if a header
     * added by this function has the same header name as one added by `additionalResponseHeaders`, the one added by
     * `additionalResponseHeaders` takes precedence.
     *
     * These MUST all be lowercase, or the application will panic, as per
     * https://docs.rs/http/latest/http/header/struct.HeaderName.html#method.from_static
     */
    fun additionalErrorResponseHeaders(errorShape: StructureShape): List<Pair<String, String>> = emptyList()

    /** Returns a deserialization code generator for this protocol */
    fun structuredDataParser(): StructuredDataParserGenerator

    /** Returns a serialization code generator for this protocol */
    fun structuredDataSerializer(): StructuredDataSerializerGenerator

    /**
     * Generates a function signature like the following:
     * ```rust
     * fn parse_http_error_metadata(response_status: u16, response_headers: HeaderMap, response_body: &[u8]) -> aws_smithy_types::error::Builder
     * ```
     */
    fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType

    /**
     * Generates a function that extracts the error body content from a response body.
     *
     * For protocols with error envelopes (e.g., REST XML's `<ErrorResponse><Error>...</Error></ErrorResponse>`),
     * this returns the inner error content. For protocols without envelopes (e.g., JSON),
     * this returns the full body unchanged.
     *
     * Generated function signature:
     * ```rust
     * fn error_body_contents(body: &[u8]) -> &[u8]
     * ```
     *
     * Default: returns the full body (no envelope stripping).
     */
    fun errorBodyContents(operationShape: OperationShape): RuntimeType? = null

    /**
     * Generates a function signature like the following:
     * ```rust
     * fn parse_event_stream_error_metadata(payload: &Bytes) -> aws_smithy_types::error::Error
     * ```
     *
     * Event Stream generic errors are almost identical to HTTP generic errors, except that
     * there are no response headers or statuses available to further inform the error parsing.
     */
    fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType

    /**
     * Determines whether the `Content-Length` header should be set in an HTTP request.
     */
    fun needsRequestContentLength(operationShape: OperationShape): Boolean =
        httpBindingResolver.requestBindings(operationShape)
            .any { it.location == HttpLocation.DOCUMENT || it.location == HttpLocation.PAYLOAD }
}

typealias ProtocolMap<T, C> = Map<ShapeId, ProtocolGeneratorFactory<T, C>>

interface ProtocolGeneratorFactory<out T, C : CodegenContext> {
    fun protocol(codegenContext: C): Protocol

    fun buildProtocolGenerator(codegenContext: C): T

    fun support(): ProtocolSupport
}
