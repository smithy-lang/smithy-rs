/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.Ec2QueryParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.Ec2QuerySerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator

class Ec2QueryProtocol(private val codegenContext: CodegenContext) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val ec2QueryErrors: RuntimeType = RuntimeType.ec2QueryErrors(runtimeConfig)
    private val errorScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
            "Headers" to RuntimeType.headers(runtimeConfig),
            "XmlDecodeError" to RuntimeType.smithyXml(runtimeConfig).resolve("decode::XmlDecodeError"),
            *RuntimeType.preludeScope,
        )

    override val httpBindingResolver: HttpBindingResolver =
        StaticHttpBindingResolver(
            codegenContext.model,
            HttpTrait.builder()
                .code(200)
                .method("POST")
                .uri(UriPattern.parse("/"))
                .build(),
            "application/x-www-form-urlencoded",
            "text/xml",
        )

    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.DATE_TIME

    override fun structuredDataParser(): StructuredDataParserGenerator =
        Ec2QueryParserGenerator(codegenContext, ec2QueryErrors)

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        Ec2QuerySerializerGenerator(codegenContext)

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_http_error_metadata") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(_response_status: u16, _response_headers: &#{Headers}, response_body: &[u8]) -> #{Result}<#{ErrorMetadataBuilder}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rust("#T::parse_error_metadata(response_body)", ec2QueryErrors)
            }
        }

    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_event_stream_error_metadata") {
            rustBlockTemplate(
                "pub fn parse_event_stream_error_metadata(payload: &#{Bytes}) -> #{Result}<#{ErrorMetadataBuilder}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rust("#T::parse_error_metadata(payload.as_ref())", ec2QueryErrors)
            }
        }
}
