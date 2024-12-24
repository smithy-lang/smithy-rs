/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.parse

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

interface StructuredDataParserGenerator {
    /**
     * Generate a parse function for a given shape targeted with `@httpPayload`.
     * Entry point for payload-based parsing.
     *
     * Roughly:
     * ```rust
     * fn parse_my_struct(input: &[u8]) -> Result<MyStruct, XmlDecodeError> {
     *      ...
     * }
     * ```
     */
    fun payloadParser(member: MemberShape): RuntimeType

    /**
     * Generate a parser for operation input
     * Because only a subset of fields of the operation may be impacted by the document, a builder is passed
     * through:
     *
     * ```rust
     * fn parse_some_operation(inp: &[u8], builder: my_operation::Builder) -> Result<my_operation::Builder, XmlDecodeError> {
     *   ...
     * }
     * ```
     */
    fun operationParser(operationShape: OperationShape): RuntimeType?

    /**
     * Because only a subset of fields of the operation may be impacted by the document, a builder is passed
     * through:
     *
     * ```rust
     * fn parse_some_error(inp: &[u8], builder: my_operation::Builder) -> Result<my_operation::Builder, XmlDecodeError> {
     *   ...
     * }
     */
    fun errorParser(errorShape: StructureShape): RuntimeType?

    /**
     * Generate a parser for a server operation input structure
     *
     * ```rust
     * fn deser_operation_crate_operation_my_operation_input(
     *    value: &[u8], builder: my_operation_input::Builder
     * ) -> Result<my_operation_input::Builder, Error> {
     *    ..
     * }
     * ```
     */
    fun serverInputParser(operationShape: OperationShape): RuntimeType?
}
