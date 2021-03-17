/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
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
import software.amazon.smithy.rust.codegen.smithy.generators.HttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.generators.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.locatedIn
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.traits.InputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.traits.OutputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.StructureModifier
import software.amazon.smithy.rust.codegen.util.dq
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
        )
    }

    override fun symbolProvider(model: Model, base: RustSymbolProvider): RustSymbolProvider {
        return JsonSerializerSymbolProvider(
            model,
            SyntheticBodySymbolProvider(model, base),
            TimestampFormatTrait.Format.EPOCH_SECONDS
        )
    }

    override fun support(): ProtocolSupport = ProtocolSupport(
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
            is StructureShape -> if (shape.hasTrait(InputBodyTrait::class.java) || shape.hasTrait(OutputBodyTrait::class.java)) {
                initialSymbol.toBuilder().locatedIn(Serializers).build()
            } else null
            is MemberShape -> {
                val container = model.expectShape(shape.container)
                if (container.hasTrait(InputBodyTrait::class.java)) {
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

class BasicAwsJsonGenerator(
    private val protocolConfig: ProtocolConfig,
    private val awsJsonVersion: AwsJsonVersion
) : HttpProtocolGenerator(protocolConfig) {
    private val model = protocolConfig.model
    override fun traitImplementations(operationWriter: RustWriter, operationShape: OperationShape) {
        // All AWS JSON protocols do NOT support streaming shapes
        val outputSymbol = symbolProvider.toSymbol(operationShape.outputShape(model))
        val operationName = symbolProvider.toSymbol(operationShape).name
        operationWriter.rustTemplate(
            """
            impl #{parse_strict} for $operationName {
                type Output = Result<#{output}, #{error}>;
                fn parse(&self, response: &#{response}<#{bytes}>) -> Self::Output {
                    self.parse_response(response)
                }
            }
        """,
            "parse_strict" to RuntimeType.parseStrict(symbolProvider.config().runtimeConfig),
            "output" to outputSymbol,
            "error" to operationShape.errorSymbol(symbolProvider),
            "response" to RuntimeType.Http("Response"),
            "bytes" to RuntimeType.Bytes
        )
    }

    private val symbolProvider = protocolConfig.symbolProvider
    private val operationIndex = OperationIndex.of(model)
    override fun toHttpRequestImpl(
        implBlockWriter: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape
    ) {
        httpBuilderFun(implBlockWriter) {
            write("let builder = #T::new();", RuntimeType.HttpRequestBuilder)
            rust(
                """
                builder
                   .method("POST")
                   .header("Content-Type", "application/x-amz-json-${awsJsonVersion.value}")
                   .header("X-Amz-Target", "${protocolConfig.serviceShape.id.name}.${operationShape.id.name}")
               """
            )
        }
    }

    override fun toBodyImpl(
        implBlockWriter: RustWriter,
        inputShape: StructureShape,
        inputBody: StructureShape?,
        operationShape: OperationShape
    ) {
        if (inputBody == null) {
            bodyBuilderFun(implBlockWriter) {
                write("\"{}\".to_string().into()")
            }
            return
        }
        val bodySymbol = protocolConfig.symbolProvider.toSymbol(inputBody)
        implBlockWriter.rustBlock("fn body(&self) -> #T", bodySymbol) {
            rustBlock("#T", bodySymbol) {
                for (member in inputBody.members()) {
                    val name = protocolConfig.symbolProvider.toMemberName(member)
                    write("$name: &self.$name,")
                }
            }
        }
        bodyBuilderFun(implBlockWriter) {
            write("""#T(&self.body()).expect("serialization should succeed")""", RuntimeType.SerdeJson("to_vec"))
        }
    }

    override fun fromResponseImpl(implBlockWriter: RustWriter, operationShape: OperationShape) {
        val outputShape = operationIndex.getOutput(operationShape).get()
        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)
        val bodyId = outputShape.expectTrait(SyntheticOutputTrait::class.java).body
        val bodyShape = bodyId?.let { model.expectShape(bodyId, StructureShape::class.java) }
        val jsonErrors = RuntimeType.awsJsonErrors(protocolConfig.runtimeConfig)
        fromResponseFun(implBlockWriter, operationShape) {
            rustBlock("if #T::is_error(&response)", jsonErrors) {
                // TODO: experiment with refactoring this segment into `error_code.rs`. Currently it isn't
                // to avoid the need to double deserialize the body.
                rustTemplate(
                    """
                    let body = #{sj}::from_slice(response.body().as_ref())
                        .unwrap_or_else(|_|#{sj}::json!({}));
                    let generic = #{aws_json_errors}::parse_generic_error(&response, &body);
                    """,
                    "aws_json_errors" to jsonErrors, "sj" to RuntimeType.SJ
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
                    write("return Err(#T::unhandled(generic))", errorSymbol)
                }
            }
            // let body: OperationOutputBody = serde_json::from_slice(response.body()...);
            // Ok(OperationOutput {
            //    field1: body.field1,
            //    field2: body.field2
            // });
            deserializeBody(bodyShape, errorSymbol, outputSymbol)
        }
    }

    private fun RustWriter.deserializeBody(
        optionalBody: StructureShape?,
        errorSymbol: RuntimeType,
        outputSymbol: Symbol
    ) {
        optionalBody?.also { bodyShape ->
            rust(
                "let body: #T = #T(response.body().as_ref()).map_err(#T::unhandled)?;",
                symbolProvider.toSymbol(bodyShape),
                RuntimeType.SerdeJson("from_slice"),
                errorSymbol
            )
        }
        withBlock("Ok(", ")") {
            rustBlock("#T", outputSymbol) {
                optionalBody?.members().orEmpty().forEach { member ->
                    val name = symbolProvider.toMemberName(member)
                    write("$name: body.$name,")
                }
            }
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
                    "Ok(body) => #T::$variantName(body),",
                    errorSymbol
                )
                write("Err(e) => #T::unhandled(e)", errorSymbol)
            }
        }
        write("_ => #T::unhandled(generic)", errorSymbol)
    }
}
