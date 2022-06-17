/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.AwsQueryTrait
import software.amazon.smithy.aws.traits.protocols.Ec2QueryTrait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator

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

    /** Returns additional HTTP headers that should be included in HTTP requests for the given operation for this protocol. */
    fun additionalRequestHeaders(operationShape: OperationShape): List<Pair<String, String>> = emptyList()

    /**
     * Returns additional HTTP headers that should be included in HTTP responses for the given error shape.
     * These MUST all be lowercase, or the application will panic, as per
     * https://docs.rs/http/latest/http/header/struct.HeaderName.html#method.from_static
     */
    fun additionalErrorResponseHeaders(errorShape: StructureShape): List<Pair<String, String>> = emptyList()

    /** Returns a deserialization code generator for this protocol */
    fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator

    /** Returns a serialization code generator for this protocol */
    fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator

    /**
     * Generates a function signature like the following:
     * ```rust
     * fn parse_http_generic_error(response: &Response<Bytes>) -> aws_smithy_types::error::Error
     * ```
     */
    fun parseHttpGenericError(operationShape: OperationShape): RuntimeType

    /**
     * Generates a function signature like the following:
     * ```rust
     * fn parse_event_stream_generic_error(payload: &Bytes) -> aws_smithy_types::error::Error
     * ```
     *
     * Event Stream generic errors are almost identical to HTTP generic errors, except that
     * there are no response headers or statuses available to further inform the error parsing.
     */
    fun parseEventStreamGenericError(operationShape: OperationShape): RuntimeType
}

typealias ProtocolMap<C> = Map<ShapeId, ProtocolGeneratorFactory<ProtocolGenerator, C>>

interface ProtocolGeneratorFactory<out T : ProtocolGenerator, C : CoreCodegenContext> {
    fun protocol(codegenContext: C): Protocol
    fun buildProtocolGenerator(codegenContext: C): T
    fun transformModel(model: Model): Model
    fun symbolProvider(model: Model, base: RustSymbolProvider): RustSymbolProvider = base
    fun support(): ProtocolSupport
}

class ProtocolLoader<C : CoreCodegenContext>(private val supportedProtocols: ProtocolMap<C>) {
    fun protocolFor(
        model: Model,
        serviceShape: ServiceShape
    ): Pair<ShapeId, ProtocolGeneratorFactory<ProtocolGenerator, C>> {
        val protocols: MutableMap<ShapeId, Trait> = ServiceIndex.of(model).getProtocols(serviceShape)
        val matchingProtocols =
            protocols.keys.mapNotNull { protocolId -> supportedProtocols[protocolId]?.let { protocolId to it } }
        if (matchingProtocols.isEmpty()) {
            throw CodegenException("No matching protocol — service offers: ${protocols.keys}. We offer: ${supportedProtocols.keys}")
        }
        return matchingProtocols.first()
    }

    companion object {
        val DefaultProtocols = mapOf(
            AwsJson1_0Trait.ID to AwsJsonFactory(AwsJsonVersion.Json10),
            AwsJson1_1Trait.ID to AwsJsonFactory(AwsJsonVersion.Json11),
            AwsQueryTrait.ID to AwsQueryFactory(),
            Ec2QueryTrait.ID to Ec2QueryFactory(),
            RestJson1Trait.ID to RestJsonFactory(),
            RestXmlTrait.ID to RestXmlFactory(),
        )
        val Default = ProtocolLoader(DefaultProtocols)
    }
}
