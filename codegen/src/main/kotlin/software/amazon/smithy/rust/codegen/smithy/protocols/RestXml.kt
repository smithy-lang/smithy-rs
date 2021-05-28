/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.parsers.RestXmlParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parsers.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parsers.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parsers.XmlBindingTraitSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RemoveEventStreamOperations
import software.amazon.smithy.rust.codegen.util.expectTrait

class RestXmlFactory(private val generator: (ProtocolConfig) -> Protocol = { RestXml(it) }) :
    ProtocolGeneratorFactory<HttpTraitProtocolGenerator> {
    override fun buildProtocolGenerator(
        protocolConfig: ProtocolConfig
    ): HttpTraitProtocolGenerator {
        return HttpTraitProtocolGenerator(protocolConfig, generator(protocolConfig))
    }

    override fun transformModel(model: Model): Model {
        return OperationNormalizer(model).transformModel(
            inputBodyFactory = OperationNormalizer.NoBody,
            outputBodyFactory = OperationNormalizer.NoBody
        ).let(RemoveEventStreamOperations::transform)
    }

    override fun support(): ProtocolSupport {
        return ProtocolSupport(
            requestSerialization = true,
            requestBodySerialization = true,
            responseDeserialization = true,
            errorDeserialization = true
        )
    }
}

open class RestXml(private val protocolConfig: ProtocolConfig) : Protocol {
    private val restXml = protocolConfig.serviceShape.expectTrait<RestXmlTrait>()
    private val runtimeConfig = protocolConfig.runtimeConfig
    protected val restXmlErrors: RuntimeType = when (restXml.isNoErrorWrapping) {
        true -> RuntimeType.unwrappedXmlErrors(runtimeConfig)
        false -> RuntimeType.wrappedXmlErrors(runtimeConfig)
    }

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator {
        return RestXmlParserGenerator(protocolConfig, restXmlErrors)
    }

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator {
        return XmlBindingTraitSerializerGenerator(protocolConfig)
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
