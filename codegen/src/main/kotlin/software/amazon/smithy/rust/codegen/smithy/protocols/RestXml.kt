/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.parsers.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parsers.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parsers.XmlBindingTraitParserGenerator
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RemoveEventStreamOperations
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class RestXmlFactory : ProtocolGeneratorFactory<HttpTraitProtocolGenerator> {
    override fun buildProtocolGenerator(protocolConfig: ProtocolConfig): HttpTraitProtocolGenerator {
        return HttpTraitProtocolGenerator(protocolConfig, RestXml(protocolConfig))
    }

    override fun transformModel(model: Model): Model {
        return OperationNormalizer(model).transformModel(
            inputBodyFactory = OperationNormalizer.NoBody,
            outputBodyFactory = OperationNormalizer.NoBody
        ).let(RemoveEventStreamOperations::transform)
    }

    override fun support(): ProtocolSupport {
        return ProtocolSupport(
            requestSerialization = false,
            requestBodySerialization = false,
            responseDeserialization = true,
            errorDeserialization = true
        )
    }
}

class RestXml(private val protocolConfig: ProtocolConfig) : Protocol {
    private val restXml = protocolConfig.serviceShape.expectTrait(RestXmlTrait::class.java)
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val restXmlErrors: RuntimeType = when (restXml.isNoErrorWrapping) {
        true -> RuntimeType.unwrappedXmlErrors(runtimeConfig)
        false -> RuntimeType.wrappedXmlErrors(runtimeConfig)
    }

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator {
        return XmlBindingTraitParserGenerator(protocolConfig, restXmlErrors)
    }

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator {
        return RestXmlSerializer(protocolConfig)
    }

    override fun parseGenericError(operationShape: OperationShape): RuntimeType {
        /**
         fn parse_generic(response: &Response<Bytes>) -> Result<smithy_types::error::Generic, T: Error>
         **/
        return RuntimeType.forInlineFun("parse_generic_error", "xml_deser") {
            it.rustBlockTemplate(
                "pub fn parse_generic_error(response: &#{Response}<#{Bytes}>) -> Result<#{Error}, #{XmlError}>",
                "Response" to RuntimeType.http.member("Response"),
                "Bytes" to RuntimeType.Bytes,
                "Error" to RuntimeType.GenericError(runtimeConfig),
                "XmlError" to CargoDependency.smithyXml(runtimeConfig).asType().member("decode::XmlError")
            ) {
                rust("#T::parse_generic_error(response.body().as_ref())", restXmlErrors)
            }
        }
    }

    override fun documentContentType(): String? {
        return null
    }

    override fun defaultContentType(): String = "application/xml"
}

class RestXmlSerializer(protocolConfig: ProtocolConfig) : StructuredDataSerializerGenerator {
    private val symbolProvider = protocolConfig.symbolProvider
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val model = protocolConfig.model
    override fun payloadSerializer(member: MemberShape): RuntimeType {
        val target = model.expectShape(member.target)
        val fnName = "serialize_payload_${target.id.name.toSnakeCase()}_${member.container.name.toSnakeCase()}"
        return RuntimeType.forInlineFun(fnName, "operation_ser") {
            val t = symbolProvider.toSymbol(member).rustType().stripOuter<RustType.Option>().render(true)
            it.rustBlock(
                "pub fn $fnName(_input: &$t) -> Result<#T, String>",

                RuntimeType.sdkBody(runtimeConfig),
            ) {
                rust("todo!()")
            }
        }
    }

    override fun operationSeralizer(operationShape: OperationShape): RuntimeType? {
        return null
    }

    override fun documentSerializer(): RuntimeType {
        // RestXML does not support documents
        TODO("Not yet implemented")
    }
}
