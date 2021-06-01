package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.HttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.AwsQueryParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.AwsQuerySerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RemoveEventStreamOperations
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class AwsQueryFactory : ProtocolGeneratorFactory<AwsQueryProtocolGenerator> {
    override fun buildProtocolGenerator(protocolConfig: ProtocolConfig): AwsQueryProtocolGenerator {
        return AwsQueryProtocolGenerator(protocolConfig)
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
            errorDeserialization = true,
        )
    }
}

class AwsQueryProtocolGenerator(private val protocolConfig: ProtocolConfig) : HttpProtocolGenerator(protocolConfig) {
    private val model = protocolConfig.model
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val symbolProvider = protocolConfig.symbolProvider
    private val awsQueryErrors: RuntimeType = RuntimeType.wrappedXmlErrors(runtimeConfig)
    private val codegenScope = arrayOf(
        "ParseStrict" to RuntimeType.parseStrict(runtimeConfig),
        "Response" to RuntimeType.Http("Response"),
        "Bytes" to RuntimeType.Bytes,
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
    )

    override fun RustWriter.body(self: String, operationShape: OperationShape): BodyMetadata {
        val serializerGenerator = structuredDataSerializer()
        serializerGenerator.operationSerializer(operationShape)?.let { serializer ->
            rust(
                "#T(&self).map_err(|err|#T::SerializationError(err.into()))?",
                serializer,
                runtimeConfig.operationBuildError()
            )
        } ?: rustTemplate("#{SdkBody}::from(\"\")", *codegenScope)
        return BodyMetadata(takesOwnership = false)
    }

    override fun traitImplementations(operationWriter: RustWriter, operationShape: OperationShape) {
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))
        val operationName = symbolProvider.toSymbol(operationShape).name

        with(operationWriter) {
            rustTemplate(
                """
                impl #{ParseStrict} for $operationName {
                    type Output = Result<#{O}, #{E}>;
                    fn parse(&self, response: &#{Response}<#{Bytes}>) -> Self::Output {
                         if !response.status().is_success() && response.status().as_u16() != 200 {
                            #{parse_error}(response)
                         } else {
                            #{parse_response}(response)
                         }
                    }
                }""",
                *codegenScope,
                "O" to outputSymbol,
                "E" to operationShape.errorSymbol(symbolProvider),
                "parse_error" to parseError(operationShape),
                "parse_response" to parseResponse(operationShape)
            )
        }
    }

    override fun toHttpRequestImpl(
        implBlockWriter: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape
    ) {
        httpBuilderFun(implBlockWriter) {
            rust(
                """
                Ok(
                    #T::new()
                        .method("POST")
                        .header("Content-Type", "application/x-www-form-urlencoded")
                )
                """,
                RuntimeType.HttpRequestBuilder
            )
        }
    }

    private fun parseError(operationShape: OperationShape): RuntimeType {
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}_error"
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        return RuntimeType.forInlineFun(fnName, "operation_deser") {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)
            it.rustBlockTemplate(
                "pub fn $fnName(response: &#{Response}<#{Bytes}>) -> Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol
            ) {

                rust("let generic = #T(&response).map_err(#T::unhandled)?;", parseGenericError(), errorSymbol)
                if (operationShape.errors.isNotEmpty()) {
                    rustTemplate(
                        """
                        let error_code = match generic.code() {
                            Some(code) => code,
                            None => return Err(#{error_symbol}::unhandled(generic))
                        };""",
                        "error_symbol" to errorSymbol,
                    )
                    withBlock("Err(match error_code {", "})") {
                        operationShape.errors.forEach { error ->
                            val errorShape = model.expectShape(error, StructureShape::class.java)
                            val variantName = symbolProvider.toSymbol(model.expectShape(error)).name
                            withBlock(
                                "${error.name.dq()} => #1T { meta: generic, kind: #1TKind::$variantName({",
                                "})},",
                                errorSymbol
                            ) {
                                renderShapeParser(operationShape, errorShape, errorSymbol)
                            }
                        }
                        rust("_ => #T::generic(generic)", errorSymbol)
                    }
                } else {
                    rust("Err(#T::generic(generic))", errorSymbol)
                }
            }
        }
    }

    private fun parseResponse(operationShape: OperationShape): RuntimeType {
        val fnName = "parse_${operationShape.id.name.toSnakeCase()}_response"
        val outputShape = operationShape.outputShape(model)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        return RuntimeType.forInlineFun(fnName, "operation_deser") {
            Attribute.Custom("allow(clippy::unnecessary_wraps)").render(it)
            it.rustBlockTemplate(
                "pub fn $fnName(response: &#{Response}<#{Bytes}>) -> Result<#{O}, #{E}>",
                *codegenScope,
                "O" to outputSymbol,
                "E" to errorSymbol
            ) {
                withBlock("Ok({", "})") {
                    renderShapeParser(operationShape, outputShape, errorSymbol)
                }
            }
        }
    }

    private fun parseGenericError(): RuntimeType {
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

    private fun RustWriter.renderShapeParser(
        operationShape: OperationShape,
        outputShape: StructureShape,
        errorSymbol: RuntimeType,
    ) {
        // avoid non-usage warnings for response
        rust("let _ = response;")

        val structuredDataParser = structuredDataParser()
        Attribute.AllowUnusedMut.render(this)
        rust("let mut output = #T::default();", outputShape.builderSymbol(symbolProvider))
        if (outputShape.id == operationShape.output.get()) {
            structuredDataParser.operationParser(operationShape)?.also { parser ->
                rust(
                    "output = #T(response.body().as_ref(), output).map_err(#T::unhandled)?;",
                    parser,
                    errorSymbol
                )
            }
        } else {
            check(outputShape.hasTrait<ErrorTrait>()) { "should only be called on outputs or errors $outputShape" }
            structuredDataParser.errorParser(outputShape)?.also { parser ->
                rust(
                    "output = #T(response.body().as_ref(), output).map_err(#T::unhandled)?;",
                    parser,
                    errorSymbol
                )
            }
        }

        val err = if (StructureGenerator.fallibleBuilder(outputShape, symbolProvider)) {
            ".map_err(|s|${format(errorSymbol)}::unhandled(s))?"
        } else ""
        rust("output.build()$err")
    }

    private fun structuredDataParser(): StructuredDataParserGenerator =
        AwsQueryParserGenerator(protocolConfig, awsQueryErrors)

    private fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        AwsQuerySerializerGenerator(protocolConfig)
}
