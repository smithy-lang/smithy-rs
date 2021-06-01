/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.AwsQueryParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.AwsQuerySerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RemoveEventStreamOperations
import software.amazon.smithy.rust.codegen.util.orNull

class AwsQueryFactory : ProtocolGeneratorFactory<HttpBoundProtocolGenerator> {
    override fun buildProtocolGenerator(protocolConfig: ProtocolConfig): HttpBoundProtocolGenerator =
        HttpBoundProtocolGenerator(protocolConfig, AwsQueryProtocol(protocolConfig))

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
            errorDeserialization = true,
        )
    }
}

class AwsQueryHttpBindingResolver(private val model: Model) : HttpBindingResolver {
    private val httpTrait = HttpTrait.builder()
        .code(200)
        .method("POST")
        .uri(UriPattern.parse("/"))
        .build()

    private fun bindings(shape: ToShapeId?) =
        shape?.let { model.expectShape(it.toShapeId()) }?.members()
            ?.map { HttpBindingDescriptor(it, HttpLocation.DOCUMENT, "document") }
            ?.toList()
            ?: emptyList()

    override fun httpTrait(operationShape: OperationShape): HttpTrait = httpTrait

    override fun requestBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        bindings(operationShape.input.orNull())

    override fun responseBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        bindings(operationShape.output.orNull())

    override fun errorResponseBindings(errorShape: ToShapeId): List<HttpBindingDescriptor> =
        bindings(errorShape)

    override fun requestContentType(operationShape: OperationShape): String = "application/x-www-form-urlencoded"
}

class AwsQueryProtocol(private val protocolConfig: ProtocolConfig) : Protocol {
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val awsQueryErrors: RuntimeType = RuntimeType.wrappedXmlErrors(runtimeConfig)
    override val httpBindingResolver: HttpBindingResolver = AwsQueryHttpBindingResolver(protocolConfig.model)

    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.DATE_TIME

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator =
        AwsQueryParserGenerator(protocolConfig, awsQueryErrors)

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator =
        AwsQuerySerializerGenerator(protocolConfig)

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
                rust("#T::parse_generic_error(response.body().as_ref())", awsQueryErrors)
            }
        }
    }
}
