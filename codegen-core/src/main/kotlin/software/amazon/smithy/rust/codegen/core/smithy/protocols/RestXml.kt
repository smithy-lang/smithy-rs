/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.RestXmlParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.XmlBindingTraitSerializerGenerator
import software.amazon.smithy.rust.codegen.core.util.expectTrait

open class RestXml(val codegenContext: CodegenContext) : Protocol {
    private val restXml = codegenContext.serviceShape.expectTrait<RestXmlTrait>()
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
            "Headers" to RuntimeType.headers(runtimeConfig),
            "XmlDecodeError" to RuntimeType.smithyXml(runtimeConfig).resolve("decode::XmlDecodeError"),
            *RuntimeType.preludeScope
        )

    protected val restXmlErrors: RuntimeType =
        when (restXml.isNoErrorWrapping) {
            true -> RuntimeType.unwrappedXmlErrors(runtimeConfig)
            false -> RuntimeType.wrappedXmlErrors(runtimeConfig)
        }

    override val httpBindingResolver: HttpBindingResolver =
        HttpTraitHttpBindingResolver(
            codegenContext.model,
            ProtocolContentTypes(
                requestDocument = "application/xml",
                responseDocument = "application/xml",
                eventStreamContentType = "application/vnd.amazon.eventstream",
                eventStreamMessageContentType = "application/xml",
            ),
        )

    override val defaultTimestampFormat: TimestampFormatTrait.Format =
        TimestampFormatTrait.Format.DATE_TIME

    override fun structuredDataParser(): StructuredDataParserGenerator =
        RestXmlParserGenerator(codegenContext, restXmlErrors)

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        XmlBindingTraitSerializerGenerator(codegenContext, httpBindingResolver)

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_http_error_metadata") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(_response_status: u16, _response_headers: &#{Headers}, response_body: &[u8]) -> #{Result}<#{ErrorMetadataBuilder}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rust("#T::parse_error_metadata(response_body)", restXmlErrors)
            }
        }

    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_event_stream_error_metadata") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(payload: &#{Bytes}) -> #{Result}<#{ErrorMetadataBuilder}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rust("#T::parse_error_metadata(payload.as_ref())", restXmlErrors)
            }
        }
}
