/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.HttpErrorTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.http.ResponseBindingGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.transformers.errorMessageMember
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase
import java.util.logging.Logger

class RestJson1HttpSerializerGenerator(
    codegenContext: CodegenContext,
    private val httpBindingResolver: HttpTraitHttpBindingResolver,
) {
    private val logger = Logger.getLogger(javaClass.name)
    private val error = RuntimeType("error", null, "crate")
    private val operation = RuntimeType("operation", null, "crate")
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val index = HttpBindingIndex.of(model)
    private val runtimeConfig = codegenContext.runtimeConfig
    private val instantiator = with(codegenContext) { Instantiator(symbolProvider, model, runtimeConfig) }
    private val smithyJson = CargoDependency.smithyJson(runtimeConfig).asType()
    private val smithyHttp = CargoDependency.SmithyHttp(runtimeConfig).asType()
    private val jsonSerializerGenerator = JsonSerializerGenerator(codegenContext, httpBindingResolver)
    private val codegenScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "Result" to RuntimeType.std.member("result::Result"),
            "Convert" to RuntimeType.std.member("convert"),
            "Response" to RuntimeType.Http("Response"),
            "build_error" to runtimeConfig.operationBuildError(),
            "JsonSerdeError" to error.member("Error"),
            "JsonObjectWriter" to smithyJson.member("serialize::JsonObjectWriter"),
            "ParseHttpResponse" to smithyHttp.member("response::ParseHttpResponse"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig = runtimeConfig),
        )

    fun render(writer: RustWriter, operationShape: OperationShape) {
        renderResponseSerializer(writer, operationShape)
        renderErrorSerializer(writer, operationShape)
    }

    private fun renderResponseSerializer(writer: RustWriter, operationShape: OperationShape) {
        val fnName = "serialize_${operationShape.id.name.toSnakeCase()}_response"
        val outputShape = operationShape.outputShape(model)
        if (outputShape.hasStreamingMember(model)) {
            logger.warning(
                "[rust-server-codegen] $operationShape: response serialization does not currently support streaming shapes"
            )
            return
        }
        val serializerSymbol = jsonSerializerGenerator.serverOutputSerializer(operationShape)
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        writer.write("")
        writer.rustBlockTemplate(
            "##[allow(dead_code)] pub fn $fnName(output: &#{O}) -> #{Result}<#{Response}<#{Bytes}>, #{JsonSerdeError}>",
            *codegenScope,
            "O" to outputSymbol,
        ) {
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
                            let status = output.${it.memberName.toLowerCase()}
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
                            "[rust-server-codegen] $operationShape: response serialization does not currently support $location bindings"
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
            "##[allow(dead_code)] pub fn $fnName(error: &#{E}Kind) -> #{Result}<#{Response}<#{Bytes}>, #{JsonSerdeError}>",
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
                    val serializerSymbol = jsonSerializerGenerator.serverErrorSerializer(it)
                    rustBlock("#TKind::${variantSymbol.name}($data) =>", errorSymbol) {
                        rust(
                            """
                            #T(&$data)?;
                            object.key(${"code".dq()}).string(${httpBindingResolver.errorCode(variantShape).dq()});
                            """.trimIndent(),
                            serializerSymbol
                        )
                        if (variantShape.errorMessageMember() != null) {
                            rust(
                                """
                                if let Some(message) = $data.message() {
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
}

class RestJson1HttpDeserializerGenerator(
    private val codegenContext: CodegenContext,
    private val httpBindingResolver: HttpTraitHttpBindingResolver,
) {
    private val logger = Logger.getLogger(javaClass.name)
    private val error = RuntimeType("error", null, "crate")
    private val operation = RuntimeType("operation", null, "crate")
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val index = HttpBindingIndex.of(model)
    private val runtimeConfig = codegenContext.runtimeConfig
    private val instantiator = with(codegenContext) { Instantiator(symbolProvider, model, runtimeConfig) }
    private val jsonParserGenerator = JsonParserGenerator(codegenContext, httpBindingResolver)
    private val codegenScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "Result" to RuntimeType.std.member("result::Result"),
            "FromStr" to RuntimeType.std.member("str::FromStr"),
            "Request" to RuntimeType.Http("Request"),
            "Instant" to
                CargoDependency.SmithyTypes(runtimeConfig).asType().member("instant"),
            "Static" to CargoDependency("lazy_static", CratesIo("1.4")).asType(),
            "Regex" to CargoDependency("regex", CratesIo("1.0")).asType(),
            "PercentEncoding" to
                CargoDependency("percent-encoding", CratesIo("2.1.0")).asType(),
            "JsonSerdeError" to error.member("Error"),
        )
    private val operationDeserModule = RustModule.public("operation_deser")

    fun render(writer: RustWriter, operationShape: OperationShape) {
        renderRequestDeserializer(writer, operationShape)
        // renderRequestDeserializerTests(writer, operationShape)
    }

    private fun renderRequestDeserializer(writer: RustWriter, operationShape: OperationShape) {
        val inputShape = operationShape.inputShape(model)
        val deserFnName = "deser_${operationShape.id.name.toSnakeCase()}_request"
        if (inputShape.hasStreamingMember(model)) {
            logger.warning(
                "[rust-server-codegen] $operationShape: request deserialization does not currently support streaming shapes"
            )
            return
        }
        val deserializerSymbol = jsonParserGenerator.serverInputParser(operationShape)
        val inputSymbol = symbolProvider.toSymbol(inputShape)
        writer.write("")
        writer.rustBlockTemplate(
            "##[allow(dead_code)] pub fn $deserFnName(request: &#{Request}<#{Bytes}>) -> #{Result}<#{I}, #{JsonSerdeError}>",
            *codegenScope,
            "I" to inputSymbol,
        ) {
            rust("let mut input = #T::default();", inputShape.builderSymbol(symbolProvider))
            rust(
                "input = #T(request.body().as_ref(), input)?;",
                deserializerSymbol,
            )
            httpBindingResolver.requestBindings(operationShape).forEach { binding ->
                httpBindingDeserializer(binding, operationShape)?.let { deserializer ->
                    withBlock("input = input.${binding.member.setterName()}(", ");") {
                        deserializer(this)
                    }
                }
            }
            renderPathDeserializer(writer, operationShape)
            rustTemplate("input.build().map_err(#{JsonSerdeError}::from)", *codegenScope)
        }
    }

    private fun httpBindingDeserializer(binding: HttpBindingDescriptor, operationShape: OperationShape): Writable? {
        return when (val location = binding.location) {
            HttpLocation.HEADER -> writable { renderHeaderDeserializer(this, binding, operationShape) }
            HttpLocation.LABEL -> {
                null
            }
            HttpLocation.DOCUMENT -> {
                null
            }
            else -> {
                logger.warning(
                    "[rust-server-codegen] $operationShape: request deserialization does not currently support $location bindings"
                )
                null
            }
        }
    }

    private fun renderPathDeserializer(writer: RustWriter, operationShape: OperationShape) {
        val pathBindings =
            httpBindingResolver.requestBindings(operationShape).filter {
                it.location == HttpLocation.LABEL
            }
        if (pathBindings.isEmpty()) {
            return
        }
        val pattern = StringBuilder()
        val httpTrait = httpBindingResolver.httpTrait(operationShape)
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

    private fun renderHeaderDeserializer(writer: RustWriter, binding: HttpBindingDescriptor, operationShape: OperationShape) {
        val httpBindingGenerator =
            ResponseBindingGenerator(
                RestJson(codegenContext),
                codegenContext,
                operationShape,
            )
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
        return RuntimeType.forInlineFun(fnName, operationDeserModule) { writer ->
            writer.rustBlockTemplate(
                "##[allow(dead_code)] pub fn $fnName(value: &str) -> #{Result}<#{O}, #{JsonSerdeError}>",
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
        val timestampFormat =
            index.determineTimestampFormat(
                binding.member,
                binding.location,
                TimestampFormatTrait.Format.EPOCH_SECONDS
            )
        val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, timestampFormat)
        return RuntimeType.forInlineFun(fnName, operationDeserModule) { writer ->
            writer.rustBlockTemplate(
                "##[allow(dead_code)] pub fn $fnName(value: &str) -> #{Result}<#{O}, #{JsonSerdeError}>",
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
        return RuntimeType.forInlineFun(fnName, operationDeserModule) { writer ->
            writer.rustBlockTemplate(
                "##[allow(dead_code)] pub fn $fnName(value: &str) -> #{Result}<#{O}, #{JsonSerdeError}>",
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
        val containerName = binding.member.container.name.toSnakeCase()
        val memberName = binding.memberName.toSnakeCase()
        return "deser_label_${containerName}_$memberName"
    }
}
