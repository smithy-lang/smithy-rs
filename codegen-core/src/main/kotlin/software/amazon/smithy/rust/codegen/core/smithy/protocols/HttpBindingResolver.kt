/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.orNull

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
    fun requestMembers(
        operationShape: OperationShape,
        location: HttpLocation,
    ): List<MemberShape> = requestBindings(operationShape).filter { it.location == location }.map { it.member }

    /**
     * Returns a list of member shapes bound to a given response [location] for a given [operationShape]
     */
    fun responseMembers(
        operationShape: OperationShape,
        location: HttpLocation,
    ): List<MemberShape> = responseBindings(operationShape).filter { it.location == location }.map { it.member }

    /**
     * Determine the timestamp format based on the input parameters.
     *
     * By default, this uses the timestamp trait, either on the member or on the target.
     */
    fun timestampFormat(
        memberShape: MemberShape,
        location: HttpLocation,
        defaultTimestampFormat: TimestampFormatTrait.Format,
        model: Model,
    ): TimestampFormatTrait.Format =
        memberShape.getMemberTrait(model, TimestampFormatTrait::class.java).map { it.format }
            .orElse(defaultTimestampFormat)

    /**
     * Determines the request content type for given [operationShape].
     */
    fun requestContentType(operationShape: OperationShape): String?

    /**
     * Determines the response content type for given [operationShape].
     */
    fun responseContentType(operationShape: OperationShape): String?

    /**
     * Due to an initial implementation bug, were accepting `application/cbor` when we should have been accepting
     * an event stream header. This allows configuring an optional fallback header to support backwards compatibility
     * in these cases.
     */
    fun legacyBackwardsCompatContentType(operationShape: OperationShape): String? = null

    /**
     * Determines the value of the event stream `:content-type` header based on union member
     */
    fun eventStreamMessageContentType(memberShape: MemberShape): String?

    /**
     * Determines whether event stream initial-request needs to be handled for given [shape]
     */
    fun handlesEventStreamInitialRequest(shape: Shape): Boolean

    /**
     * Determines whether event stream initial-response needs to be handled for given [shape]
     */
    fun handlesEventStreamInitialResponse(shape: Shape): Boolean

    /**
     * Returns true if this is an RPC protocol (e.g., awsJson, rpcv2Cbor), false for RESTful protocols
     */
    fun isRpcProtocol(): Boolean
}

/**
 * Content types a protocol uses.
 */
data class ProtocolContentTypes(
    /** Request content type override for when the shape is a Document */
    val requestDocument: String? = null,
    /** Response content type override for when the shape is a Document */
    val responseDocument: String? = null,
    /** EventStream content type initial request/response content-type */
    val eventStreamContentType: String? = null,
    /** EventStream content type for struct message shapes (for `:content-type`) */
    val eventStreamMessageContentType: String? = null,
) {
    companion object {
        /** Create an instance of [ProtocolContentTypes] where all content types are the same */
        fun consistent(type: String) = ProtocolContentTypes(type, type, type, type)

        /**
         * Returns the event stream message `:content-type` for the given event stream union member shape.
         *
         * The `protocolContentType` is the content-type to use for non-string/non-blob shapes.
         */
        fun eventStreamMemberContentType(
            model: Model,
            memberShape: MemberShape,
            protocolContentType: String?,
        ): String? =
            when (model.expectShape(memberShape.target)) {
                is StringShape -> "text/plain"
                is BlobShape -> "application/octet-stream"
                else -> protocolContentType
            }
    }
}

/**
 * An [HttpBindingResolver] that relies on the HttpTrait data in the Smithy models.
 */
open class HttpTraitHttpBindingResolver(
    private val model: Model,
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
        defaultTimestampFormat: TimestampFormatTrait.Format,
        model: Model,
    ): TimestampFormatTrait.Format = httpIndex.determineTimestampFormat(memberShape, location, defaultTimestampFormat)

    /**
     * Note that `null` will be returned and hence `Content-Type` will not be set when operation input has no members.
     * This is in line with what protocol tests assert.
     */
    override fun requestContentType(operationShape: OperationShape): String? =
        httpIndex.determineRequestContentType(
            operationShape,
            contentTypes.requestDocument,
            contentTypes.eventStreamContentType,
        ).orNull()

    /**
     * Note that `null` will be returned and hence `Content-Type` will not be set when operation output has no members.
     * This is in line with what protocol tests assert.
     */
    override fun responseContentType(operationShape: OperationShape): String? =
        httpIndex.determineResponseContentType(
            operationShape,
            contentTypes.responseDocument,
            contentTypes.eventStreamContentType,
        ).orNull()

    override fun eventStreamMessageContentType(memberShape: MemberShape): String? =
        ProtocolContentTypes.eventStreamMemberContentType(
            model,
            memberShape,
            contentTypes.eventStreamMessageContentType,
        )

    // Sort the members after extracting them from the map to have a consistent order
    private fun mappedBindings(bindings: Map<String, HttpBinding>): List<HttpBindingDescriptor> =
        bindings.values.map(::HttpBindingDescriptor).sortedBy { it.memberName }

    override fun handlesEventStreamInitialRequest(shape: Shape) = false

    override fun handlesEventStreamInitialResponse(shape: Shape) = false

    override fun isRpcProtocol() = false
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
    private val eventStreamMessageContentType: String? = null,
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

    override fun errorResponseBindings(errorShape: ToShapeId): List<HttpBindingDescriptor> = bindings(errorShape)

    override fun requestContentType(operationShape: OperationShape): String = requestContentType

    override fun responseContentType(operationShape: OperationShape): String = responseContentType

    override fun eventStreamMessageContentType(memberShape: MemberShape): String? =
        ProtocolContentTypes.eventStreamMemberContentType(model, memberShape, eventStreamMessageContentType)

    override fun handlesEventStreamInitialRequest(shape: Shape) = false

    override fun handlesEventStreamInitialResponse(shape: Shape) = false

    override fun isRpcProtocol() = false
}
