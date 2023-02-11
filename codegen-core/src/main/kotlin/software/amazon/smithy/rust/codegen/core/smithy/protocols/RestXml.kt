/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.RestXmlParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.XmlBindingTraitSerializerGenerator
import software.amazon.smithy.rust.codegen.core.util.expectTrait

open class RestXml(val codegenContext: CodegenContext) : Protocol {
    private val restXml = codegenContext.serviceShape.expectTrait<RestXmlTrait>()
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
        "HeaderMap" to RuntimeType.HttpHeaderMap,
        "Response" to RuntimeType.HttpResponse,
        "XmlDecodeError" to RuntimeType.smithyXml(runtimeConfig).resolve("decode::XmlDecodeError"),
    )
    private val xmlDeserModule = RustModule.private("xml_deser")

    protected val restXmlErrors: RuntimeType = when (restXml.isNoErrorWrapping) {
        true -> RuntimeType.unwrappedXmlErrors(runtimeConfig)
        false -> RuntimeType.wrappedXmlErrors(runtimeConfig)
    }

    override val httpBindingResolver: HttpBindingResolver =
        HttpTraitHttpBindingResolver(codegenContext.model, ProtocolContentTypes("application/xml", "application/xml", "application/vnd.amazon.eventstream"))

    override val defaultTimestampFormat: TimestampFormatTrait.Format =
        TimestampFormatTrait.Format.DATE_TIME

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator {
        fun builderSymbol(shape: StructureShape): Symbol =
            shape.builderSymbol(codegenContext.symbolProvider)
        return RestXmlParserGenerator(codegenContext, restXmlErrors, ::builderSymbol)
    }

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator {
        return XmlBindingTraitSerializerGenerator(codegenContext, httpBindingResolver)
    }

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_error_metadata", xmlDeserModule) {
            rustBlockTemplate(
                "pub fn parse_http_error_metadata(response: &#{Response}<#{Bytes}>) -> Result<#{ErrorMetadataBuilder}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rust("#T::parse_error_metadata(response.body().as_ref())", restXmlErrors)
            }
        }

    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_event_stream_error_metadata", xmlDeserModule) {
            rustBlockTemplate(
                "pub fn parse_event_stream_error_metadata(payload: &#{Bytes}) -> Result<#{ErrorMetadataBuilder}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rust("#T::parse_error_metadata(payload.as_ref())", restXmlErrors)
            }
        }
}
