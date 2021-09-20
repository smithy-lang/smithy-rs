package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.IdempotencyTokenTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.protocoltests.traits.HttpResponseTestCase
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.http.ResponseBindingGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.deserializeFunctionName
import software.amazon.smithy.rust.codegen.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.findMemberWithTrait
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase
import java.util.logging.Logger

class HttpDeserializerGenerator(
    private val protocolConfig: ProtocolConfig,
    private val httpBindingResolver: HttpTraitHttpBindingResolver,
) {
    fun render(writer: RustWriter, operationShape: OperationShape) {
        HttpRequestDeserializerGenerator(
            protocolConfig,
            httpBindingResolver,
            operationShape
        ).render(writer)
    }
}

class HttpRequestDeserializerGenerator(
    protocolConfig: ProtocolConfig,
    private val httpBindingResolver: HttpTraitHttpBindingResolver,
    private val operationShape: OperationShape,
) {
    private val logger = Logger.getLogger(javaClass.name)
    private val deserFnName = "deser_${operationShape.id.name.toSnakeCase()}_request"
    private val serde = RuntimeType("json_deser", null, "crate")
    private val error = RuntimeType("error", null, "crate")
    private val operation = RuntimeType("operation", null, "crate")
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val model = protocolConfig.model
    private val index = HttpBindingIndex.of(model)
    private val service = protocolConfig.serviceShape
    private val symbolProvider = protocolConfig.symbolProvider
    private val httpBindingGenerator = ResponseBindingGenerator(
        RestJson(protocolConfig),
        protocolConfig,
        operationShape,
    )
    private val httpTrait = httpBindingResolver.httpTrait(operationShape)
    private val instantiator = with(protocolConfig) {
        Instantiator(symbolProvider, model, runtimeConfig)
    }
    private val defaultTimestampFormat = TimestampFormatTrait.Format.EPOCH_SECONDS
    private val codegenScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "Result" to RuntimeType.std.member("result::Result"),
        "FromStr" to RuntimeType.std.member("str::FromStr"),
        "Request" to RuntimeType.Http("Request"),
        "Instant" to CargoDependency.SmithyTypes(runtimeConfig).asType().member("instant"),
        "Static" to CargoDependency("lazy_static", CratesIo("1.4")).asType(),
        "Regex" to CargoDependency("regex", CratesIo("1.0")).asType(),
        "PercentEncoding" to CargoDependency("percent-encoding", CratesIo("2.1.0")).asType(),
        "JsonSerdeError" to error.member("Error"),
    )

    fun render(writer: RustWriter) {
        renderRequestDeserializer(writer)
        renderRequestDeserializerTests(writer)
    }

    private fun renderRequestDeserializer(writer: RustWriter) {
        val inputShape = operationShape.inputShape(model)
        if (inputShape.hasStreamingMember(model)) {
            logger.warning("$operationShape: request deserialization does not currently support streaming shapes")
            return
        }
        val inputSymbol = symbolProvider.toSymbol(inputShape)
        writer.write("")
        writer.rustBlockTemplate(
            "pub fn $deserFnName(request: &#{Request}<#{Bytes}>) -> #{Result}<#{I}, #{JsonSerdeError}>",
            *codegenScope,
            "I" to inputSymbol,
        ) {
            val deserializerSymbol = operation.member(symbolProvider.deserializeFunctionName(inputShape))
            rust(
                "let mut input = #T::default();",
                inputShape.builderSymbol(symbolProvider)
            )
            rust(
                "input = #T(request.body().as_ref(), input)?;",
                deserializerSymbol,
            )
            httpBindingResolver.requestBindings(operationShape).forEach { binding ->
                httpBindingDeserializer(binding)?.let { deserializer ->
                    withBlock("input = input.${binding.member.setterName()}(", ");") {
                        deserializer(this)
                    }
                }
            }
            renderPathDeserializer(writer)
            rustTemplate("input.build().map_err(#{JsonSerdeError}::from)", *codegenScope)
        }
    }

    private fun httpBindingDeserializer(binding: HttpBindingDescriptor): Writable? {
        return when (val location = binding.location) {
            HttpLocation.HEADER -> writable { renderHeaderDeserializer(this, binding) }
            HttpLocation.LABEL -> { null }
            HttpLocation.DOCUMENT -> { null }
            else -> {
                logger.warning("$operationShape: request deserialization does not currently support $location bindings")
                null
            }
        }
    }

    private fun renderPathDeserializer(writer: RustWriter) {
        val pathBindings = httpBindingResolver
            .requestBindings(operationShape)
            .filter { it.location == HttpLocation.LABEL }
        if (pathBindings.isEmpty()) {
            return
        }
        val pattern = StringBuilder()
        httpTrait.uri.segments.forEach {
            pattern.append("/")
            if (it.isLabel) {
                pattern.append("(?P<${it.content}>")
                if (it.isGreedyLabel) {
                    pattern.append(".+")
                } else {
                    pattern.append("[^/]+")
                }
                pattern.append(")")
            } else {
                pattern.append(it.content)
            }
        }
        val errorShape = operationShape.errorSymbol(symbolProvider)
        with(writer) {
            rustTemplate(
                """
                    #{Static}::lazy_static! {
                        static ref RE: #{Regex}::Regex = #{Regex}::Regex::new("$pattern").unwrap();
                    }
                """.trimIndent(),
                *codegenScope,
            )
            rustBlock("if let Some(captures) = RE.captures(request.uri().path())") {
                pathBindings.forEach {
                    val deserializer = generateDeserializeLabelFn(it)
                    rustTemplate(
                        """
                            if let Some(m) = captures.name("${it.locationName}") {
                                input = input.${it.member.setterName()}(
                                    #{deserializer}(m.as_str())?
                                );
                            }
                        """.trimIndent(),
                        "deserializer" to deserializer,
                        "E" to errorShape,
                    )
                }
            }
        }
    }

    private fun renderHeaderDeserializer(writer: RustWriter, binding: HttpBindingDescriptor) {
        val deserializer = httpBindingGenerator.generateDeserializeHeaderFn(binding)
        writer.rust(
            """
                #T(request.headers())?
            """.trimIndent(),
            deserializer,
        )
    }

    private fun generateDeserializeLabelFn(binding: HttpBindingDescriptor): RuntimeType {
        check(binding.location == HttpLocation.LABEL)
        val target = model.expectShape(binding.member.target)
        return when {
            target.isStringShape -> generateDeserializeLabelStringFn(binding)
            target.isTimestampShape -> generateDeserializeLabelTimestampFn(binding)
            else -> generateDeserializeLabelPrimitiveFn(binding)
        }
    }

    private fun generateDeserializeLabelStringFn(binding: HttpBindingDescriptor): RuntimeType {
        val output = symbolProvider.toSymbol(binding.member)
        val fnName = generateDeserializeLabelFnName(binding)
        return RuntimeType.forInlineFun(fnName, "operation") { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(value: &str) -> #{Result}<#{O}, #{JsonSerdeError}>",
                *codegenScope,
                "O" to output,
            ) {
                rustTemplate(
                    """
                        let value = #{PercentEncoding}::percent_decode_str(value)
                            .decode_utf8()
                            .map_err(|err| #{JsonSerdeError}::DeserializeLabel(err.to_string()))?;
                        Ok(Some(value.into_owned()))
                    """.trimIndent(),
                    *codegenScope,
                )
            }
        }
    }

    private fun generateDeserializeLabelTimestampFn(binding: HttpBindingDescriptor): RuntimeType {
        val output = symbolProvider.toSymbol(binding.member)
        val fnName = generateDeserializeLabelFnName(binding)
        val timestampFormat = index.determineTimestampFormat(
            binding.member,
            binding.location,
            defaultTimestampFormat,
        )
        val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
        return RuntimeType.forInlineFun(fnName, "operation") { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(value: &str) -> #{Result}<#{O}, #{JsonSerdeError}>",
                *codegenScope,
                "O" to output,
            ) {
                rustTemplate(
                    """
                        let value = #{PercentEncoding}::percent_decode_str(value)
                            .decode_utf8()
                            .map_err(|err| #{JsonSerdeError}::DeserializeLabel(err.to_string()))?;
                        let value = #{Instant}::Instant::from_str(&value, #{format})
                            .map_err(|err| #{JsonSerdeError}::DeserializeLabel(err.to_string()))?;
                        Ok(Some(value))
                    """.trimIndent(),
                    *codegenScope,
                    "format" to timestampFormatType,
                )
            }
        }
    }

    private fun generateDeserializeLabelPrimitiveFn(binding: HttpBindingDescriptor): RuntimeType {
        val output = symbolProvider.toSymbol(binding.member)
        val fnName = generateDeserializeLabelFnName(binding)
        return RuntimeType.forInlineFun(fnName, "operation") { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(value: &str) -> #{Result}<#{O}, #{JsonSerdeError}>",
                *codegenScope,
                "O" to output,
            ) {
                rustTemplate(
                    """
                        let value = #{FromStr}::from_str(value)
                            .map_err(|_| #{JsonSerdeError}::DeserializeLabel(${"label parse error".dq()}.to_string()))?;
                        Ok(Some(value))
                    """.trimIndent(),
                    *codegenScope,
                )
            }
        }
    }

    private fun generateDeserializeLabelFnName(binding: HttpBindingDescriptor): String {
        val opName = operationShape.id.getName(service).toSnakeCase()
        val containerName = binding.member.container.name.toSnakeCase()
        val memberName = binding.memberName.toSnakeCase()
        return "deser_label_${opName}_${containerName}_${memberName}"
    }

    private fun renderRequestDeserializerTests(writer: RustWriter) {
        val testCases = operationShape.getTrait<HttpRequestTestsTrait>()
            ?.getTestCasesFor(AppliesTo.SERVER)
            ?: return
        val testModuleName = "deser_${operationShape.id.name.toSnakeCase()}_test"
        val moduleMeta = RustMetadata(
            public = false,
            additionalAttributes = listOf(
                Attribute.Cfg("test"),
                Attribute.Custom("allow(unreachable_code, unused_variables)")
            )
        )
        writer.write("")
        writer.withModule(testModuleName, moduleMeta) {
            testCases.forEach {
                renderRequestDeserializerTestCase(it)
            }
        }
    }

    private fun RustWriter.renderRequestDeserializerTestCase(testCase: HttpRequestTestCase) {
        Attribute.Custom("test").render(this)
        rustBlock("fn ${testCase.id.toSnakeCase()}()") {
            val inputShape = operationShape.inputShape(model)
            val customToken = inputShape
                .findMemberWithTrait<IdempotencyTokenTrait>(model)
                ?.let { """.make_token("00000000-0000-4000-8000-000000000000")""" }
                ?: ""
            rust("let config = #T::Config::builder()$customToken.build();", RuntimeType.Config)
            writeInline("let expected = ")
            instantiator.render(this, inputShape, testCase.params)
            write(";")
            rust("""let op = expected.make_operation(&config).expect("failed to build operation");""")
            rust("let (request, parts) = op.into_request_response().0.into_parts();")
            rustTemplate("let request = request.map(|body| #{Bytes}::from(body.bytes().unwrap().to_vec()));", *codegenScope)
            rust("""let actual = #T(&request).expect("failed to parse request");""", operation.member(deserFnName))
            rust("assert_eq!(expected, actual);")
        }
    }
}
