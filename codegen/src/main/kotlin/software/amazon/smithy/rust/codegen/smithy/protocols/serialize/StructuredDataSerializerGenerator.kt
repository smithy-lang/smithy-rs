/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

interface StructuredDataSerializerGenerator {
    /**
     * Generate a serializer for a request payload. Expected signature:
     * ```rust
     * fn serialize_some_payload(input: &PayloadSmithyType) -> Result<Vec<u8>, Error> {
     *     ...
     * }
     * ```
     */
    fun payloadSerializer(member: MemberShape): RuntimeType

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
     * fn serialize_document(input: &Document) -> Result<SdkBody, Error> {
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
    fun serverOutputSerializer(operationShape: OperationShape): RuntimeType

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
