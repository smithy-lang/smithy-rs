/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parsers

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

interface StructuredDataSerializerGenerator {
    /**
     * Generate a parse function for a given targeted as a payload.
     * Entry point for payload-based parsing.
     * Roughly:
     * ```rust
     * ```
     */
    fun payloadSerializer(member: MemberShape): RuntimeType

    /** Generate a serializer for operation input
     * Because only a subset of fields of the operation may be impacted by the document, a builder is passed
     * through:
     *
     * ```rust
     * fn parse_some_operation(inp: &[u8], builder: my_operation::Builder) -> Result<my_operation::Builder, XmlError> {
     *   ...
     * }
     * ```
     */
    fun operationSerializer(operationShape: OperationShape): RuntimeType?

    /**
     * ```rust
     * fn parse_document(inp: &[u8]) -> Result<Document, Error> {
     *   ...
     * }
     * ```
     */
    fun documentSerializer(): RuntimeType
}
