/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

interface StructuredDataSerializerGenerator {
    /**
     * Generate a serializer for a request payload.
     *
     * ```rust
     * fn serialize_some_payload(input: &PayloadSmithyType) -> Result<Vec<u8>, Error> {
     *     ...
     * }
     * ```
     */
    fun payloadSerializer(member: MemberShape): RuntimeType

    /**
     * Generate the correct data when attempting to serialize a structure that is unset.
     *
     * ```rust
     * fn rest_json_unset_struct_payload() -> Vec<u8> {
     *     ...
     * }
     * ```
     */
    fun unsetStructure(structure: StructureShape): RuntimeType

    /**
     * Generate the correct data when attempting to serialize a union that is unset
     *
     * ```rust
     * fn rest_json_unset_union_payload() -> Vec<u8> {
     *     ...
     * }
     * ```
     *
     * This method is only invoked when serializing an `@httpPayload`.
     */
    fun unsetUnion(union: UnionShape): RuntimeType

    /**
     * Generate a serializer for an operation input structure.
     * This serializer is only used by clients.
     * The serialized data is returned in an `SdkBody` that is used in HTTP requests.
     * Returns `null` if there's nothing to serialize.
     *
     * ```rust
     * fn serialize_some_operation(input: &SomeSmithyType) -> Result<SdkBody, Error> {
     *     ...
     * }
     * ```
     */
    fun operationInputSerializer(operationShape: OperationShape): RuntimeType?

    /**
     * Generate a serializer for a document.
     *
     * ```rust
     * fn serialize_document(input: &Document) -> Vec<u8> {
     *     ...
     * }
     * ```
     */
    fun documentSerializer(): RuntimeType

    /**
     * Generate a serializer for an operation output structure.
     * This serializer is only used by servers.
     * The serialized data is returned in a `String` that is used in HTTP response bodies.
     * Returns `null` if there's nothing to serialize.
     *
     * ```rust
     * fn serialize_structure_crate_output_my_output_structure(value: &SomeSmithyType) -> Result<String, Error> {
     *     ...
     * }
     * ```
     */
    fun operationOutputSerializer(operationShape: OperationShape): RuntimeType?

    /**
     * Generate a serializer for a server operation error structure.
     *
     * ```rust
     * fn serialize_structure_crate_output_my_error_structure(value: &SomeSmithyType) -> Result<String, Error> {
     *     ...
     * }
     * ```
     */
    fun serverErrorSerializer(shape: ShapeId): RuntimeType
}
