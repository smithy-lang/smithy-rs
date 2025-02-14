/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.AwsQueryErrorTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.AwsQueryParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.AwsQuerySerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.util.getTrait

private val awsQueryHttpTrait =
    HttpTrait.builder()
        .code(200)
        .method("POST")
        .uri(UriPattern.parse("/"))
        .build()

class AwsQueryBindingResolver(private val model: Model) :
    StaticHttpBindingResolver(model, awsQueryHttpTrait, "application/x-www-form-urlencoded", "text/xml") {
    override fun errorCode(errorShape: ToShapeId): String {
        val error = model.expectShape(errorShape.toShapeId())
        return error.getTrait<AwsQueryErrorTrait>()?.code ?: errorShape.toShapeId().name
    }
}

class AwsQueryProtocol(private val codegenContext: CodegenContext) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val awsQueryErrors: RuntimeType = RuntimeType.wrappedXmlErrors(runtimeConfig)
    private val errorScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
            "Headers" to RuntimeType.headers(runtimeConfig),
            "XmlDecodeError" to RuntimeType.smithyXml(runtimeConfig).resolve("decode::XmlDecodeError"),
            *RuntimeType.preludeScope
        )

    override val httpBindingResolver: HttpBindingResolver = AwsQueryBindingResolver(codegenContext.model)

    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.DATE_TIME

    override fun structuredDataParser(): StructuredDataParserGenerator =
        AwsQueryParserGenerator(codegenContext, awsQueryErrors)

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        AwsQuerySerializerGenerator(codegenContext)

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_http_error_metadata") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(_response_status: u16, _response_headers: &#{Headers}, response_body: &[u8]) -> #{Result}<#{ErrorMetadataBuilder}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rust("#T::parse_error_metadata(response_body)", awsQueryErrors)
            }
        }

    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_event_stream_error_metadata") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(payload: &#{Bytes}) -> #{Result}<#{ErrorMetadataBuilder}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rust("#T::parse_error_metadata(payload.as_ref())", awsQueryErrors)
            }
        }
}
