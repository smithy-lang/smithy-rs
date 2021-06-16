/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.Serializers
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.generators.HttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.smithy.locatedIn
import software.amazon.smithy.rust.codegen.smithy.meta
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.traits.InputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.traits.OutputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RemoveEventStreamOperations
import software.amazon.smithy.rust.codegen.smithy.transformers.StructureModifier
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.outputShape

sealed class AwsJsonVersion {
    abstract val value: String

    object Json10 : AwsJsonVersion() {
        override val value = "1.0"
    }

    object Json11 : AwsJsonVersion() {
        override val value = "1.1"
    }
}

class BasicAwsJsonFactory(private val version: AwsJsonVersion) : ProtocolGeneratorFactory<BasicAwsJsonGenerator> {
    override fun buildProtocolGenerator(
        protocolConfig: ProtocolConfig
    ): BasicAwsJsonGenerator = BasicAwsJsonGenerator(protocolConfig, version)

    private val shapeIfHasMembers: StructureModifier = { _, shape: StructureShape? ->
        if (shape?.members().isNullOrEmpty()) {
            null
        } else {
            shape
        }
    }

    override fun transformModel(model: Model): Model {
        // For AwsJson10, the body matches 1:1 with the input
        return OperationNormalizer(model).transformModel(
            inputBodyFactory = shapeIfHasMembers,
            outputBodyFactory = shapeIfHasMembers
        ).let(RemoveEventStreamOperations::transform)
    }

    override fun symbolProvider(model: Model, base: RustSymbolProvider): RustSymbolProvider {
        return JsonSerializerSymbolProvider(
            model,
            SyntheticBodySymbolProvider(model, base),
            TimestampFormatTrait.Format.EPOCH_SECONDS
        )
    }

    override fun support(): ProtocolSupport = ProtocolSupport(
        requestSerialization = true,
        requestBodySerialization = true,
        responseDeserialization = true,
        errorDeserialization = true
    )
}

/**
 * SyntheticBodySymbolProvider makes two modifications:
 * 1. Body shapes are moved to `serializer.rs`
 * 2. Body shapes take a reference to all of their members:
 * If the base structure was:
 * ```rust
 * struct {
 *   field: Option<u64>
 * }
 * ```
 * The body will generate:
 * ```rust
 * struct<'a> {
 *   field: &'a Option<u64>
 * }
 *
 * This enables the creation of a body from a reference to an input without cloning.
 */
class SyntheticBodySymbolProvider(private val model: Model, private val base: RustSymbolProvider) :
    WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val initialSymbol = base.toSymbol(shape)
        val override = when (shape) {
            is StructureShape -> when {
                shape.hasTrait<InputBodyTrait>() ->
                    initialSymbol.toBuilder().locatedIn(Serializers).build()
                shape.hasTrait<OutputBodyTrait>() ->
                    initialSymbol.toBuilder().locatedIn(Serializers).meta(
                        initialSymbol.expectRustMetadata().withDerives(RuntimeType("Default", null, "std::default"))
                    ).build()
                else -> null
            }
            is MemberShape -> {
                val container = model.expectShape(shape.container)
                if (container.hasTrait<InputBodyTrait>()) {
                    initialSymbol.toBuilder().rustType(
                        RustType.Reference(
                            lifetime = "a",
                            member = initialSymbol.rustType().stripOuter<RustType.Box>()
                        )
                    ).build()
                } else {
                    null
                }
            }
            else -> null
        }
        return override ?: initialSymbol
    }
}

class AwsJsonHttpBindingResolver(
    private val model: Model,
    private val awsJsonVersion: AwsJsonVersion,
) : HttpBindingResolver {
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

    override fun requestContentType(operationShape: OperationShape): String =
        "application/x-amz-json-${awsJsonVersion.value}"
}

// TODO: Refactor to use HttpBoundProtocolGenerator
class BasicAwsJsonGenerator(
    private val protocolConfig: ProtocolConfig,
    private val awsJsonVersion: AwsJsonVersion
) : HttpProtocolGenerator(protocolConfig) {
    private val model = protocolConfig.model
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val symbolProvider = protocolConfig.symbolProvider
    private val operationIndex = OperationIndex.of(model)

    override fun traitImplementations(operationWriter: RustWriter, operationShape: OperationShape) {
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))
        val operationName = symbolProvider.toSymbol(operationShape).name
        operationWriter.rustTemplate(
            """
            impl #{parse_strict} for $operationName {
                type Output = std::result::Result<#{output}, #{error}>;
                fn parse(&self, response: &#{response}<#{bytes}>) -> Self::Output {
                    self.parse_response(response)
                }
            }
        """,
            "parse_strict" to RuntimeType.parseStrict(runtimeConfig),
            "output" to outputSymbol,
            "error" to operationShape.errorSymbol(symbolProvider),
            "response" to RuntimeType.Http("Response"),
            "bytes" to RuntimeType.Bytes
        )
    }

    override fun toHttpRequestImpl(
        implBlockWriter: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape
    ) {
        httpBuilderFun(implBlockWriter) {
            write("let builder = #T::new();", RuntimeType.HttpRequestBuilder)
            rust(
                """
                Ok(
                    builder
                       .method("POST")
                       .header("Content-Type", "application/x-amz-json-${awsJsonVersion.value}")
                       .header("X-Amz-Target", "${protocolConfig.serviceShape.id.name}.${operationShape.id.name}")
               )
               """
            )
        }
    }

    override fun RustWriter.body(self: String, operationShape: OperationShape): BodyMetadata {
        val generator = JsonSerializerGenerator(protocolConfig, AwsJsonHttpBindingResolver(model, awsJsonVersion))
        val serializer = generator.operationSerializer(operationShape)
        serializer?.also { sym ->
            rustTemplate(
                "#{serialize}(&$self).map_err(|err|#{BuildError}::SerializationError(err.into()))?",
                "serialize" to sym,
                "BuildError" to runtimeConfig.operationBuildError()
            )
        } ?: rustTemplate("""#{SdkBody}::from("{}")""", "SdkBody" to RuntimeType.sdkBody(runtimeConfig))
        return BodyMetadata(takesOwnership = false)
    }

    override fun operationImplBlock(implBlockWriter: RustWriter, operationShape: OperationShape) {
        val outputShape = operationIndex.getOutput(operationShape).get()
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        val jsonErrors = RuntimeType.awsJsonErrors(protocolConfig.runtimeConfig)
        val generator = JsonParserGenerator(protocolConfig, AwsJsonHttpBindingResolver(model, awsJsonVersion))

        fromResponseFun(implBlockWriter, operationShape) {
            rustBlock("if #T::is_error(&response)", jsonErrors) {
                rustTemplate(
                    """
                    let body = #{sj}::from_slice(response.body().as_ref())
                        .unwrap_or_else(|_|#{sj}::json!({}));
                    let generic = #{aws_json_errors}::parse_generic_error(&response, &body);
                    """,
                    "aws_json_errors" to jsonErrors, "sj" to RuntimeType.serdeJson
                )
                if (operationShape.errors.isNotEmpty()) {
                    rustTemplate(
                        """

                    let error_code = match generic.code() {
                        Some(code) => code,
                        None => return Err(#{error_symbol}::unhandled(generic))
                    };""",
                        "error_symbol" to errorSymbol
                    )
                    withBlock("return Err(match error_code {", "})") {
                        // approx:
                        /*
                            match error_code {
                                "Code1" => deserialize<Code1>(body),
                                "Code2" => deserialize<Code2>(body)
                            }
                         */
                        parseErrorVariants(operationShape, errorSymbol)
                    }
                } else {
                    write("return Err(#T::generic(generic))", errorSymbol)
                }
            }
            val parser = generator.operationParser(operationShape)
            Attribute.AllowUnusedMut.render(this)
            rust("let mut builder = #T::default();", outputShape.builderSymbol(symbolProvider))
            parser?.also {
                rustTemplate(
                    "builder = #{parse}(response.body().as_ref(), builder).map_err(#{error_symbol}::unhandled)?;",
                    "parse" to it,
                    "error_symbol" to errorSymbol
                )
            }
            withBlock("Ok(builder.build()", ")") {
                if (StructureGenerator.fallibleBuilder(outputShape, symbolProvider)) {
                    rust(""".map_err(#T::unhandled)?""", errorSymbol)
                }
            }
        }
    }

    private fun fromResponseFun(
        implBlockWriter: RustWriter,
        operationShape: OperationShape,
        block: RustWriter.() -> Unit
    ) {
        Attribute.Custom("allow(clippy::unnecessary_wraps)").render(implBlockWriter)
        Attribute.AllowUnused.render(implBlockWriter)
        implBlockWriter.rustBlock(
            "fn parse_response(&self, response: & #T<#T>) -> std::result::Result<#T, #T>",
            RuntimeType.Http("response::Response"),
            RuntimeType.Bytes,
            symbolProvider.toSymbol(operationShape.outputShape(model)),
            operationShape.errorSymbol(symbolProvider)
        ) {
            block(this)
        }
    }

    private fun RustWriter.parseErrorVariants(
        operationShape: OperationShape,
        errorSymbol: RuntimeType
    ) {
        operationShape.errors.forEach { error ->
            rustBlock("${error.name.dq()} => match #T(body)", RuntimeType.SerdeJson("from_value")) {
                val variantName = symbolProvider.toSymbol(model.expectShape(error)).name
                write(
                    "Ok(body) => #1T { kind: #1TKind::$variantName(body), meta: generic },",
                    errorSymbol
                )
                write("Err(e) => #T::unhandled(e)", errorSymbol)
            }
        }
        write("_ => #T::generic(generic)", errorSymbol)
    }
}
