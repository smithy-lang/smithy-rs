/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolContentTypes
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.RestXmlParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.XmlBindingTraitSerializerGenerator
import software.amazon.smithy.rust.codegen.util.expectTrait

class ServerRestXmlFactory(private val generator: (CodegenContext) -> Protocol = { ServerRestXml(it) }) :
    ProtocolGeneratorFactory<ServerHttpProtocolGenerator> {
    override fun protocol(codegenContext: CodegenContext): Protocol = generator(codegenContext)

    override fun buildProtocolGenerator(codegenContext: CodegenContext): ServerHttpProtocolGenerator =
        ServerHttpProtocolGenerator(codegenContext, ServerRestXml(codegenContext))

    override fun transformModel(model: Model): Model = model

    override fun support(): ProtocolSupport {
        return ProtocolSupport(
            /* Client support */
            requestSerialization = false,
            requestBodySerialization = false,
            responseDeserialization = false,
            errorDeserialization = false,
            /* Server support */
            requestDeserialization = true,
            requestBodyDeserialization = true,
            responseSerialization = true,
            errorSerialization = true
        )
    }
}

open class ServerRestXml(private val codegenContext: CodegenContext) : Protocol {
    private val restXml = codegenContext.serviceShape.expectTrait<RestXmlTrait>()
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "Error" to RuntimeType.GenericError(runtimeConfig),
        "HeaderMap" to RuntimeType.http.member("HeaderMap"),
        "Response" to RuntimeType.http.member("Response"),
        "XmlError" to CargoDependency.smithyXml(runtimeConfig).asType().member("decode::XmlError")
    )
    private val xmlDeserModule = RustModule.private("xml_deser")

    protected val restXmlErrors: RuntimeType = when (restXml.isNoErrorWrapping) {
        true -> RuntimeType.unwrappedXmlErrors(runtimeConfig)
        false -> RuntimeType.wrappedXmlErrors(runtimeConfig)
    }

    override val httpBindingResolver: HttpBindingResolver =
        HttpTraitHttpBindingResolver(codegenContext.model, ProtocolContentTypes.consistent("application/xml"))

    override val defaultTimestampFormat: TimestampFormatTrait.Format =
        TimestampFormatTrait.Format.DATE_TIME

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator {
        return RestXmlParserGenerator(codegenContext, restXmlErrors)
    }

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator {
        return XmlBindingTraitSerializerGenerator(codegenContext, httpBindingResolver)
    }

    override fun parseHttpGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_generic_error", xmlDeserModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn parse_http_generic_error(response: &#{Response}<#{Bytes}>) -> Result<#{Error}, #{XmlError}>",
                *errorScope
            ) {
                rust("#T::parse_generic_error(response.body().as_ref())", restXmlErrors)
            }
        }

    override fun parseEventStreamGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_event_stream_generic_error", xmlDeserModule) { writer ->
            writer.rustBlockTemplate(
                "pub fn parse_event_stream_generic_error(payload: &#{Bytes}) -> Result<#{Error}, #{XmlError}>",
                *errorScope
            ) {
                rust("#T::parse_generic_error(payload.as_ref())", restXmlErrors)
            }
        }
}
