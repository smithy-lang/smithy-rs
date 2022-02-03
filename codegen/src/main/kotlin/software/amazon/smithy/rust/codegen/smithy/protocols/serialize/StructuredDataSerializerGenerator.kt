/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

interface StructuredDataSerializerGenerator {
    // TODO Update docs, they now return Vec<u8>.
    // Double-check we need `Result`s

    /**
     * Generate a serializer for a request payload. Expected signature:
     * ```rust
     * fn serialize_some_payload(input: &PayloadSmithyType) -> Result<Vec<u8>, Error> {
     *     ...
     * }
     * ```
     */
    // TODO Does this need to return `Result`?
    fun payloadSerializer(member: MemberShape): RuntimeType

    /**
     * Generate the correct data when attempting to serialize a structure that is unset
     */
    fun unsetStructure(structure: StructureShape): RuntimeType

    /**
     * Generate a serializer for an operation input.
     * ```rust
     * fn serialize_some_operation(input: &SomeSmithyType) -> Result<SdkBody, Error> {
     *     ...
     * }
     * ```
     */
    fun operationSerializer(operationShape: OperationShape): RuntimeType?

    /**
     * Generate a serializer for a document.
     * ```rust
     * fn serialize_document(input: &Document) -> Vec<u8> {
     *     ...
     * }
     * ```
     */
    fun documentSerializer(): RuntimeType

    /**
     * Generate a serializer for a server operation output structure
     * ```rust
     * fn serialize_structure_crate_output_my_output_structure(value: &SomeSmithyType) -> Result<String, Error> {
     *     ...
     * }
     * ```
     */
    fun serverOutputSerializer(operationShape: OperationShape): RuntimeType?

    /**
     * Generate a serializer for a server operation error structure
     * ```rust
     * fn serialize_structure_crate_output_my_error_structure(value: &SomeSmithyType) -> Result<String, Error> {
     *     ...
     * }
     * ```
     */
    fun serverErrorSerializer(shape: ShapeId): RuntimeType
}
