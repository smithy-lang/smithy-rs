/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.Ec2QueryParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.Ec2QuerySerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator

class Ec2QueryFactory : ProtocolGeneratorFactory<HttpBoundProtocolGenerator, ClientCodegenContext> {
    override fun protocol(codegenContext: ClientCodegenContext): Protocol = Ec2QueryProtocol(codegenContext)

    override fun buildProtocolGenerator(codegenContext: ClientCodegenContext): HttpBoundProtocolGenerator =
        HttpBoundProtocolGenerator(codegenContext, protocol(codegenContext))

    override fun transformModel(model: Model): Model = model

    override fun support(): ProtocolSupport {
        return ProtocolSupport(
            /* Client support */
            requestSerialization = true,
            requestBodySerialization = true,
            responseDeserialization = true,
            errorDeserialization = true,
            /* Server support */
            requestDeserialization = false,
            requestBodyDeserialization = false,
            responseSerialization = false,
            errorSerialization = false
        )
    }
}

class Ec2QueryProtocol(private val coreCodegenContext: CoreCodegenContext) : Protocol {
    private val runtimeConfig = coreCodegenContext.runtimeConfig
    private val ec2QueryErrors: RuntimeType = RuntimeType.ec2QueryErrors(runtimeConfig)
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "Error" to RuntimeType.GenericError(runtimeConfig),
        "HeaderMap" to RuntimeType.http.member("HeaderMap"),
        "Response" to RuntimeType.http.member("Response"),
        "XmlError" to CargoDependency.smithyXml(runtimeConfig).asType().member("decode::XmlError")
    )
    private val xmlDeserModule = RustModule.private("xml_deser")

    override val httpBindingResolver: HttpBindingResolver = StaticHttpBindingResolver(
        coreCodegenContext.model,
        HttpTrait.builder()
            .code(200)
            .method("POST")
            .uri(UriPattern.parse("/"))
            .build(),
        "application/x-www-form-urlencoded",
        "text/xml"
    )

    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.DATE_TIME

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator =
        Ec2QueryParserGenerator(coreCodegenContext, ec2QueryErrors)

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator =
        Ec2QuerySerializerGenerator(coreCodegenContext)

    override fun parseHttpGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_generic_error", xmlDeserModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn parse_http_generic_error(response: &#{Response}<#{Bytes}>) -> Result<#{Error}, #{XmlError}>",
                *errorScope
            ) {
                rust("#T::parse_generic_error(response.body().as_ref())", ec2QueryErrors)
            }
        }

    override fun parseEventStreamGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_event_stream_generic_error", xmlDeserModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn parse_event_stream_generic_error(payload: &#{Bytes}) -> Result<#{Error}, #{XmlError}>",
                *errorScope
            ) {
                rust("#T::parse_generic_error(payload.as_ref())", ec2QueryErrors)
            }
        }
}
