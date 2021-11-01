/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.orNull

typealias HttpLocation = HttpBinding.Location

/**
 * A generic subset of [HttpBinding] that is not specific to any protocol implementation.
 */
data class HttpBindingDescriptor(
    val member: MemberShape,
    val location: HttpLocation,
    val locationName: String,
) {
    constructor(httpBinding: HttpBinding) : this(httpBinding.member, httpBinding.location, httpBinding.locationName)

    val memberName: String get() = member.memberName
}

/**
 * Represents a protocol-specific implementation that resolves HTTP binding data.
 */
interface HttpBindingResolver {
    /**
     * Returns an [HttpTrait] for the given operation. Even if the protocol doesn't
     * support the HTTP traits, this should always return an [HttpTrait] value representing
     * what the protocol desires for request/response binding.
     */
    fun httpTrait(operationShape: OperationShape): HttpTrait

    /**
     * Returns a list of request HTTP bindings for a given [operationShape].
     */
    fun requestBindings(operationShape: OperationShape): List<HttpBindingDescriptor>

    /**
     * Returns a list of response HTTP bindings for a given [operationShape].
     */
    fun responseBindings(operationShape: OperationShape): List<HttpBindingDescriptor>

    /**
     * Returns a list of response HTTP bindings for an [errorShape].
     */
    fun errorResponseBindings(errorShape: ToShapeId): List<HttpBindingDescriptor>

    fun errorCode(errorShape: ToShapeId): String = errorShape.toShapeId().name

    /**
     * Returns a list of member shapes bound to a given request [location] for a given [operationShape]
     */
    fun requestMembers(operationShape: OperationShape, location: HttpLocation): List<MemberShape> =
        requestBindings(operationShape).filter { it.location == location }.map { it.member }

    /**
     * Returns a list of member shapes bound to a given response [location] for a given [operationShape]
     */
    fun responseMembers(operationShape: OperationShape, location: HttpLocation): List<MemberShape> =
        responseBindings(operationShape).filter { it.location == location }.map { it.member }

    /**
     * Determine the timestamp format based on the input parameters.
     */
    fun timestampFormat(
        memberShape: MemberShape,
        location: HttpLocation,
        defaultTimestampFormat: TimestampFormatTrait.Format,
    ): TimestampFormatTrait.Format =
        memberShape.getTrait<TimestampFormatTrait>()?.format ?: defaultTimestampFormat

    /**
     * Determines the request content type for given [operationShape].
     */
    fun requestContentType(operationShape: OperationShape): String?

    /**
     * Determines the response content type for given [operationShape].
     */
    fun responseContentType(operationShape: OperationShape): String?
}

/**
 * Content types a protocol uses.
 */
data class ProtocolContentTypes(
    /** Request content type override for when the shape is a Document */
    val requestDocument: String? = null,
    /** Response content type override for when the shape is a Document */
    val responseDocument: String? = null,
    /** EventStream content type */
    val eventStreamContentType: String? = null
) {
    companion object {
        /** Create an instance of [ProtocolContentTypes] where all content types are the same */
        fun consistent(type: String) = ProtocolContentTypes(type, type, type)
    }
}

/**
 * An [HttpBindingResolver] that relies on the HttpTrait data in the Smithy models.
 */
class HttpTraitHttpBindingResolver(
    model: Model,
    private val contentTypes: ProtocolContentTypes,
) : HttpBindingResolver {
    private val httpIndex: HttpBindingIndex = HttpBindingIndex.of(model)

    override fun httpTrait(operationShape: OperationShape): HttpTrait = operationShape.expectTrait()

    override fun requestBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        mappedBindings(httpIndex.getRequestBindings(operationShape))

    override fun responseBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        mappedBindings(httpIndex.getResponseBindings(operationShape))

    override fun errorResponseBindings(errorShape: ToShapeId): List<HttpBindingDescriptor> =
        mappedBindings(httpIndex.getResponseBindings(errorShape))

    override fun timestampFormat(
        memberShape: MemberShape,
        location: HttpLocation,
        defaultTimestampFormat: TimestampFormatTrait.Format
    ): TimestampFormatTrait.Format =
        httpIndex.determineTimestampFormat(memberShape, location, defaultTimestampFormat)

    override fun requestContentType(operationShape: OperationShape): String? =
        httpIndex.determineRequestContentType(operationShape, contentTypes.requestDocument, contentTypes.eventStreamContentType).orNull()

    override fun responseContentType(operationShape: OperationShape): String? =
        httpIndex.determineResponseContentType(operationShape, contentTypes.responseDocument, contentTypes.eventStreamContentType).orNull()

    // Sort the members after extracting them from the map to have a consistent order
    private fun mappedBindings(bindings: Map<String, HttpBinding>): List<HttpBindingDescriptor> =
        bindings.values.map(::HttpBindingDescriptor).sortedBy { it.memberName }
}

/**
 * Takes an [HttpTrait] value and content type, and provides bindings based on those.
 * All members will end up being document members.
 */
open class StaticHttpBindingResolver(
    private val model: Model,
    private val httpTrait: HttpTrait,
    private val requestContentType: String,
    private val responseContentType: String,
) : HttpBindingResolver {
    private fun bindings(shape: ToShapeId?) =
        shape?.let { model.expectShape(it.toShapeId()) }?.members()
            ?.map { HttpBindingDescriptor(it, HttpLocation.DOCUMENT, "document") }
            ?.toList()
            ?: emptyList()

    override fun httpTrait(operationShape: OperationShape): HttpTrait = httpTrait

    override fun requestBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        bindings(operationShape.input.orNull())

    override fun responseBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        bindings(operationShape.output.orNull())

    override fun errorResponseBindings(errorShape: ToShapeId): List<HttpBindingDescriptor> =
        bindings(errorShape)

    override fun requestContentType(operationShape: OperationShape): String = requestContentType

    override fun responseContentType(operationShape: OperationShape): String = responseContentType
}
