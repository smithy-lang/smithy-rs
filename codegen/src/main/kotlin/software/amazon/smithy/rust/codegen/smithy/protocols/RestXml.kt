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
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RemoveEventStreamOperations

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
}
