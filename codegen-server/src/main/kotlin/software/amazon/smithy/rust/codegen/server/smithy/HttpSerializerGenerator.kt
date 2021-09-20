package software.amazon.smithy.rust.codegen.server.smithy

import java.util.logging.Logger
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.HttpErrorTrait
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpResponseTestCase
import software.amazon.smithy.protocoltests.traits.HttpResponseTestsTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.serializeFunctionName
import software.amazon.smithy.rust.codegen.smithy.transformers.errorMessageMember
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class HttpSerializerGenerator(
        protocolConfig: ProtocolConfig,
        private val httpBindingResolver: HttpTraitHttpBindingResolver,
) {
    private val logger = Logger.getLogger(javaClass.name)
    private val ser = RuntimeType("json_ser", null, "crate")
    private val error = RuntimeType("error", null, "crate")
    private val operation = RuntimeType("operation", null, "crate")
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val model = protocolConfig.model
    private val symbolProvider = protocolConfig.symbolProvider
    private val instantiator =
            with(protocolConfig) { Instantiator(symbolProvider, model, runtimeConfig) }
    private val smithyJson = CargoDependency.smithyJson(runtimeConfig).asType()
    private val smithyHttp = CargoDependency.SmithyHttp(runtimeConfig).asType()
    private val codegenScope =
            arrayOf(
                    "Bytes" to RuntimeType.Bytes,
                    "Result" to RuntimeType.std.member("result::Result"),
                    "Convert" to RuntimeType.std.member("convert"),
                    "Response" to RuntimeType.Http("Response"),
                    "build_error" to runtimeConfig.operationBuildError(),
                    "JsonSerdeError" to error.member("Error"),
                    "JsonObjectWriter" to smithyJson.member("serialize::JsonObjectWriter"),
                    "parse_http_response" to smithyHttp.member("response::ParseHttpResponse"),
                    "sdk_body" to RuntimeType.sdkBody(runtimeConfig = runtimeConfig),
            )

    fun render(writer: RustWriter, operationShape: OperationShape) {
        renderResponseSerializer(writer, operationShape)
        renderErrorSerializer(writer, operationShape)
        renderTests(writer, operationShape)
    }

    private fun renderResponseSerializer(writer: RustWriter, operationShape: OperationShape) {
        val fnName = "serialize_${operationShape.id.name.toSnakeCase()}_response"
        val outputShape = operationShape.outputShape(model)
        if (outputShape.hasStreamingMember(model)) {
            logger.warning(
                    "$operationShape: response serialization does not currently support streaming shapes"
            )
            return
        }
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        writer.write("")
        writer.rustBlockTemplate(
                "pub fn $fnName(output: &#{O}) -> #{Result}<#{Response}<#{Bytes}>, #{JsonSerdeError}>",
                *codegenScope,
                "O" to outputSymbol,
        ) {
            val serializerSymbol = operation.member(symbolProvider.serializeFunctionName(outputShape))
            rust(
                    "let payload = #T(output)?;",
                    serializerSymbol,
            )
            Attribute.AllowUnusedMut.render(this)
            rustTemplate("let mut response = #{Response}::builder();", *codegenScope)
            httpBindingResolver.responseBindings(operationShape).forEach {
                when (val location = it.location) {
                    HttpLocation.RESPONSE_CODE -> {
                        rustTemplate(
                                """
                                let status = output.${it.memberName}
                                    .ok_or(#{JsonSerdeError}::generic(${(it.member.memberName + " missing or empty").dq()}))?;
                                let http_status: u16 = #{Convert}::TryFrom::<i32>::try_from(status)
                                    .map_err(|_| #{JsonSerdeError}::generic(${("invalid status code").dq()}))?;
                            """.trimIndent(),
                                *codegenScope,
                        )
                        rust("let response = response.status(http_status);")
                    }
                    HttpLocation.HEADER, HttpLocation.PREFIX_HEADERS, HttpLocation.PAYLOAD -> {
                        logger.warning(
                                "$operationShape: response serialization does not currently support $location bindings"
                        )
                    }
                    else -> {}
                }
            }
            rustTemplate(
                    """
                    response.body(#{Bytes}::from(payload))
                        .map_err(#{JsonSerdeError}::BuildResponse)
                """.trimIndent(),
                    *codegenScope,
            )
        }
    }

    private fun renderErrorSerializer(writer: RustWriter, operationShape: OperationShape) {
        val fnName = "serialize_${operationShape.id.name.toSnakeCase()}_error"
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        writer.write("")
        writer.rustBlockTemplate(
                "pub fn $fnName(error: &#{E}Kind) -> #{Result}<#{Response}<#{Bytes}>, #{JsonSerdeError}>",
                *codegenScope,
                "E" to errorSymbol,
        ) {
            rustTemplate("let mut response = #{Response}::builder();", *codegenScope)
            rust("let mut out = String::new();")
            rustTemplate("let mut object = #{JsonObjectWriter}::new(&mut out);", *codegenScope)
            withBlock("match error {", "};") {
                operationShape.errors.forEach {
                    val variantShape = model.expectShape(it, StructureShape::class.java)
                    val errorTrait = variantShape.expectTrait<ErrorTrait>()
                    val variantSymbol = symbolProvider.toSymbol(variantShape)
                    val data = safeName("var")
                    val serializerSymbol =
                            ser.member(symbolProvider.serializeFunctionName(variantShape))
                    rustBlock("#TKind::${variantSymbol.name}($data) =>", errorSymbol) {
                        rust(
                                """
                                #T(&mut object, &$data);
                                object.key(${"code".dq()}).string(${httpBindingResolver.errorCode(variantShape).dq()});
                            """.trimIndent(),
                                serializerSymbol
                        )
                        if (variantShape.errorMessageMember() != null) {
                            rust(
                                    """
                                    if let Some(message) = ${data}.message() {
                                        object.key(${"message".dq()}).string(message);
                                    }
                                """.trimIndent()
                            )
                        }
                        val bindings = httpBindingResolver.errorResponseBindings(it)
                        bindings.forEach { binding ->
                            when (val location = binding.location) {
                                HttpLocation.RESPONSE_CODE, HttpLocation.DOCUMENT -> {}
                                else -> {
                                    logger.warning(
                                            "$operationShape: response serialization does not currently support $location bindings"
                                    )
                                }
                            }
                        }
                        val status =
                                variantShape.getTrait<HttpErrorTrait>()?.let { trait -> trait.code }
                                        ?: errorTrait.defaultHttpStatusCode
                        rust("response = response.status($status);")
                    }
                }
                rust(
                        """
                        #TKind::Unhandled(_) => {
                            object.key(${"code".dq()}).string(${"Unhandled".dq()});
                            response = response.status(500);
                        }
                    """.trimIndent(),
                        errorSymbol
                )
            }
            rust("object.finish();")
            rustTemplate(
                    """
                    response.body(#{Bytes}::from(out))
                        .map_err(#{JsonSerdeError}::BuildResponse)
                """.trimIndent(),
                    *codegenScope
            )
        }
    }

    private fun renderTests(writer: RustWriter, operationShape: OperationShape) {
        val operationIndex = OperationIndex.of(model)
        val outputShape = operationShape.outputShape(model)
        val responseTests =
                operationShape
                        .getTrait<HttpResponseTestsTrait>()
                        ?.getTestCasesFor(AppliesTo.SERVER)
                        .orEmpty()
                        .map { it to outputShape }
        val errorTests =
                operationIndex.getErrors(operationShape).flatMap { error ->
                    error.getTrait<HttpResponseTestsTrait>()?.testCases.orEmpty().map {
                        it to error
                    }
                }
        if (responseTests.isEmpty() && errorTests.isEmpty()) {
            return
        }
        val testModuleName = "serialize_${operationShape.id.name.toSnakeCase()}_test"
        val moduleMeta =
                RustMetadata(
                        public = false,
                        additionalAttributes =
                                listOf(
                                        Attribute.Cfg("test"),
                                        Attribute.Custom(
                                                "allow(unreachable_code, unused_variables)"
                                        )
                                )
                )
        writer.write("")
        writer.withModule(testModuleName, moduleMeta) {
            responseTests.forEach {
                renderSerializeResponseTestCase(operationShape, it.first, it.second)
            }
            errorTests.forEach {
                renderSerializeResponseTestCase(operationShape, it.first, it.second)
            }
        }
    }

    private fun RustWriter.renderSerializeResponseTestCase(
            operationShape: OperationShape,
            testCase: HttpResponseTestCase,
            shape: StructureShape
    ) {
        val isError = shape.hasTrait<ErrorTrait>()
        val fnName =
                if (isError) "serialize_${operationShape.id.name.toSnakeCase()}_error"
                else "serialize_${operationShape.id.name.toSnakeCase()}_response"
        val variantName =
                if (isError)
                        "${format(operationShape.errorSymbol(symbolProvider))}Kind::${symbolProvider.toSymbol(shape).name}"
                else ""
        Attribute.Custom("test").render(this)
        rustBlock("fn ${testCase.id.toSnakeCase()}()") {
            rust("let config = #T::Config::builder().build();", RuntimeType.Config)
            writeInline("let expected = ")
            instantiator.render(this, shape, testCase.params)
            write(";")
            if (isError) {
                rust("let expected = ${variantName}(expected);")
            }
            rust(
                    """let response = #T(&expected).expect("failed to serialize response");""",
                    ser.member(fnName)
            )
            rust("assert_eq!(response.status(), ${testCase.code});")

            rustTemplate("let mut response = response.map(#{sdk_body}::from);", *codegenScope)
            rustTemplate(
                    """
                    use #{parse_http_response};
                    let parser = #{op}::new();
                    let actual = parser.parse_unloaded(&mut response);
                    let actual = actual.unwrap_or_else(|| {
                        let response = response.map(|body|#{Bytes}::copy_from_slice(body.bytes().unwrap()));
                        <#{op} as #{parse_http_response}<#{sdk_body}>>::parse_loaded(&parser, &response)
                    });
                """.trimIndent(),
                    *codegenScope,
                    "op" to symbolProvider.toSymbol(operationShape),
            )
            if (isError) {
                rust("""let actual = actual.expect_err("failed to parse error");""")
                rust(
                        """
                        match (&expected, &actual.kind) {
                            (${variantName}(expected), ${variantName}(actual)) => assert_eq!(expected, actual),
                            _ => panic!("incorrect error type"),
                        };
                    """.trimIndent()
                )
            } else {
                rust("""let actual = actual.expect("failed to parse error");""")
                rust("assert_eq!(expected, actual);")
            }
        }
    }
}
