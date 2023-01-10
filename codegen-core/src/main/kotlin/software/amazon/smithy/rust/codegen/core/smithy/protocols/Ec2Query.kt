/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.Ec2QueryParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.Ec2QuerySerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator

class Ec2QueryProtocol(private val codegenContext: CodegenContext) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val ec2QueryErrors: RuntimeType = RuntimeType.ec2QueryErrors(runtimeConfig)
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "Error" to RuntimeType.genericError(runtimeConfig),
        "HeaderMap" to RuntimeType.HttpHeaderMap,
        "Response" to RuntimeType.HttpResponse,
        "XmlDecodeError" to RuntimeType.smithyXml(runtimeConfig).resolve("decode::XmlDecodeError"),
    )
    private val xmlDeserModule = RustModule.private("xml_deser")

    override val httpBindingResolver: HttpBindingResolver = StaticHttpBindingResolver(
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

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator {
        fun builderSymbol(shape: StructureShape): Symbol =
            shape.builderSymbol(codegenContext.symbolProvider)
        return Ec2QueryParserGenerator(codegenContext, ec2QueryErrors, ::builderSymbol)
    }

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator =
        Ec2QuerySerializerGenerator(codegenContext)

    override fun parseHttpGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_generic_error", xmlDeserModule) {
            rustBlockTemplate(
                "pub fn parse_http_generic_error(response: &#{Response}<#{Bytes}>) -> Result<#{Error}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rust("#T::parse_generic_error(response.body().as_ref())", ec2QueryErrors)
            }
        }

    override fun parseEventStreamGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_event_stream_generic_error", xmlDeserModule) {
            rustBlockTemplate(
                "pub fn parse_event_stream_generic_error(payload: &#{Bytes}) -> Result<#{Error}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rust("#T::parse_generic_error(payload.as_ref())", ec2QueryErrors)
            }
        }
}
