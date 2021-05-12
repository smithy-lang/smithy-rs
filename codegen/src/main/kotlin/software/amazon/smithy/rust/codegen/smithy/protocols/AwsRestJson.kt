/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolSupport
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RemoveEventStreamOperations
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class AwsRestJsonFactory : ProtocolGeneratorFactory<HttpTraitProtocolGenerator> {
    override fun buildProtocolGenerator(
        protocolConfig: ProtocolConfig
    ): HttpTraitProtocolGenerator = HttpTraitProtocolGenerator(protocolConfig, RestJson(protocolConfig))

    /** Create a synthetic awsJsonInputBody if specified
     * A body is created if any member of [shape] is bound to the `DOCUMENT` section of the `bindings.
     */
    private fun restJsonBody(shape: StructureShape?, bindings: Map<String, HttpBinding>): StructureShape? {
        if (shape == null) {
            return null
        }
        val bodyMembers = shape.members().filter { member ->
            bindings[member.memberName]?.location == HttpBinding.Location.DOCUMENT
        }

        return if (bodyMembers.isNotEmpty()) {
            shape.toBuilder().members(bodyMembers).build()
        } else {
            null
        }
    }

    override fun transformModel(model: Model): Model {
        val httpIndex = HttpBindingIndex.of(model)
        return OperationNormalizer(model).transformModel(
            inputBodyFactory = { op, input -> restJsonBody(input, httpIndex.getRequestBindings(op)) },
            outputBodyFactory = { op, output -> restJsonBody(output, httpIndex.getResponseBindings(op)) },
        ).let(RemoveEventStreamOperations::transform)
    }

    override fun support(): ProtocolSupport {
        // TODO: Support body for RestJson
        return ProtocolSupport(
            requestSerialization = true,
            requestBodySerialization = true,
            responseDeserialization = true,
            errorDeserialization = true
        )
    }

    override fun symbolProvider(model: Model, base: RustSymbolProvider): RustSymbolProvider {
        return JsonSerializerSymbolProvider(
            model,
            SyntheticBodySymbolProvider(model, base),
            TimestampFormatTrait.Format.EPOCH_SECONDS
        )
    }
}

class RestJson(private val protocolConfig: ProtocolConfig) : Protocol {
    private val runtimeConfig = protocolConfig.runtimeConfig
    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator {
        return RestJsonParserGenerator(protocolConfig)
    }

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator {
        return RestJsonSerializerGenerator(protocolConfig)
    }

    override fun parseGenericError(operationShape: OperationShape): RuntimeType {
        val awsJsonErrors = RuntimeType.awsJsonErrors(runtimeConfig)
        return RuntimeType.forInlineFun("parse_generic_error", "xml_deser") {
            it.rustBlockTemplate(
                "pub fn parse_generic_error(response: &#{Response}<#{Bytes}>) -> Result<#{Error}, #{SerdeError}>",
                "Response" to RuntimeType.http.member("Response"),
                "Bytes" to RuntimeType.Bytes,
                "Error" to RuntimeType.GenericError(runtimeConfig),
                "SerdeError" to RuntimeType.SerdeJson("Error")
            ) {
                rustTemplate(
                    """
                    let body = #{sj}::from_slice(response.body().as_ref())
                        .unwrap_or_else(|_|#{sj}::json!({}));
                    Ok(#{aws_json_errors}::parse_generic_error(&response, &body))
                """,
                    "sj" to RuntimeType.SJ, "aws_json_errors" to awsJsonErrors
                )
            }
        }
    }

    override fun documentContentType(): String = "application/json"

    override fun defaultContentType(): String = "application/json"
}

class RestJsonSerializerGenerator(protocolConfig: ProtocolConfig) : StructuredDataSerializerGenerator {
    private val model = protocolConfig.model
    private val symbolProvider = protocolConfig.symbolProvider
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val serializerError = RuntimeType.SerdeJson("error::Error")
    override fun payloadSerializer(member: MemberShape): RuntimeType {
        val target = model.expectShape(member.target)
        val fnName = "serialize_payload_${target.id.name.toSnakeCase()}_${member.container.name.toSnakeCase()}"
        return RuntimeType.forInlineFun(fnName, "operation_ser") {
            it.rustBlock(
                "pub fn $fnName(input: &#T) -> Result<#T, #T>",
                symbolProvider.toSymbol(target),
                RuntimeType.sdkBody(runtimeConfig),
                serializerError
            ) {
                rustTemplate(
                    "#{to_vec}(&input).map(#{SdkBody}::from)",
                    "to_vec" to RuntimeType.SerdeJson("to_vec"),
                    "SdkBody" to RuntimeType.sdkBody(runtimeConfig)
                )
            }
        }
    }

    override fun operationSeralizer(operationShape: OperationShape): RuntimeType? {
        val inputShape = operationShape.inputShape(model)
        val inputBody = inputShape.expectTrait(SyntheticInputTrait::class.java).body?.let {
            model.expectShape(
                it,
                StructureShape::class.java
            )
        } ?: return null
        val fnName = "synth_body_${inputBody.id.name.toSnakeCase()}"
        return RuntimeType.forInlineFun(fnName, "operation_ser") {
            it.rustBlock(
                "pub fn $fnName(input: &#T) -> Result<#T, #T>",
                symbolProvider.toSymbol(inputShape),
                RuntimeType.sdkBody(runtimeConfig),
                serializerError
            ) {
                withBlock("let body = ", ";") {
                    rustBlock("#T", symbolProvider.toSymbol(inputBody)) {
                        for (member in inputBody.members()) {
                            val name = symbolProvider.toMemberName(member)
                            write("$name: &input.$name,")
                        }
                    }
                }
                rustTemplate(
                    """#{serde_json}::to_vec(&body).map(#{SdkBody}::from)""",
                    "serde_json" to CargoDependency.SerdeJson.asType(),
                    "SdkBody" to RuntimeType.sdkBody(runtimeConfig)
                )
            }
        }
    }

    override fun documentSerializer(): RuntimeType {
        val fnName = "serialize_document"
        return RuntimeType.forInlineFun(fnName, "operation_ser") {
            it.rustBlock(
                "pub fn $fnName(input: &#T) -> Result<#T, #T>",
                RuntimeType.Document(runtimeConfig),
                RuntimeType.sdkBody(runtimeConfig),
                serializerError
            ) {
                rustTemplate(
                    """#{to_vec}(&#{doc_json}::SerDoc(&input)).map(#{SdkBody}::from)""",
                    "to_vec" to RuntimeType.SerdeJson("to_vec"),
                    "doc_json" to RuntimeType.DocJson,
                    "BuildError" to runtimeConfig.operationBuildError(),
                    "SdkBody" to RuntimeType.sdkBody(runtimeConfig)
                )
            }
        }
    }
}

class RestJsonParserGenerator(protocolConfig: ProtocolConfig) : StructuredDataParserGenerator {
    private val model = protocolConfig.model
    private val symbolProvider = protocolConfig.symbolProvider
    private val runtimeConfig = protocolConfig.runtimeConfig

    override fun payloadParser(member: MemberShape): RuntimeType {
        val shape = model.expectShape(member.target)
        check(shape is UnionShape || shape is StructureShape) { "payload parser should only be used on structures & unions" }
        val fnName =
            "parse_payload_" + shape.id.name.toString().toSnakeCase() + member.container.name.toString().toSnakeCase()
        return RuntimeType.forInlineFun(fnName, "json_deser") {
            it.rustBlock(
                "pub fn $fnName(inp: &[u8]) -> Result<#1T, #2T>",
                symbolProvider.toSymbol(shape),
                RuntimeType.SerdeJson("Error")
            ) {
                rust("#T(inp)", RuntimeType.SerdeJson("from_slice"))
            }
        }
    }

    override fun operationParser(operationShape: OperationShape): RuntimeType? {
        val outputShape = operationShape.outputShape(model)
        val fnName = operationShape.id.name.toString().toSnakeCase() + "_deser_operation"
        val bodyId = outputShape.expectTrait(SyntheticOutputTrait::class.java).body
        val bodyShape = bodyId?.let { model.expectShape(bodyId, StructureShape::class.java) } ?: return null
        val body = symbolProvider.toSymbol(bodyShape)
        if (bodyShape.members().isEmpty()) {
            return null
        }

        return RuntimeType.forInlineFun(fnName, "json_deser") {
            it.rustBlock(
                "pub fn $fnName(inp: &[u8], mut builder: #1T) -> Result<#1T, #2T>",
                outputShape.builderSymbol(symbolProvider),
                RuntimeType.SerdeJson("Error")
            ) {
                rustTemplate(
                    """
                    let parsed_body: #{Body} = if inp.is_empty() {
                        // To enable JSON parsing to succeed, replace an empty body
                        // with an empty JSON body. If a member was required, it will fail slightly later
                        // during the operation construction phase.
                        #{from_slice}(b"{}")?
                    } else {
                        #{from_slice}(inp)?
                    };
                """,
                    "Body" to body,
                    "from_slice" to RuntimeType.SerdeJson("from_slice")
                )
                bodyShape.members().forEach { member ->
                    rust("builder = builder.${member.setterName()}(parsed_body.${symbolProvider.toMemberName(member)});")
                }
                rust("Ok(builder)")
            }
        }
    }

    override fun errorParser(errorShape: StructureShape): RuntimeType? {
        if (errorShape.members().isEmpty()) {
            return null
        }
        val fnName = errorShape.id.name.toString().toSnakeCase()
        return RuntimeType.forInlineFun(fnName, "json_deser") {
            it.rustBlock(
                "pub fn $fnName(inp: &[u8], mut builder: #1T) -> Result<#1T, #2T>",
                errorShape.builderSymbol(symbolProvider),
                RuntimeType.SerdeJson("Error")
            ) {
                rustTemplate(
                    """
                    let parsed_body: #{Body} = if inp.is_empty() {
                        // To enable JSON parsing to succeed, replace an empty body
                        // with an empty JSON body. If a member was required, it will fail slightly later
                        // during the operation construction phase.
                        #{from_slice}(b"{}")?
                    } else {
                        #{from_slice}(inp)?
                    };
                """,
                    "Body" to symbolProvider.toSymbol(errorShape),
                    "from_slice" to RuntimeType.SerdeJson("from_slice")
                )
                errorShape.members().forEach { member ->
                    rust("builder = builder.${member.setterName()}(parsed_body.${symbolProvider.toMemberName(member)});")
                }
                rust("Ok(builder)")
            }
        }
    }

    override fun documentParser(operationShape: OperationShape): RuntimeType {
        val fnName = "parse_document"
        return RuntimeType.forInlineFun(fnName, "json_deser") {
            it.rustBlock(
                "pub fn $fnName(inp: &[u8]) -> Result<#1T, #2T>",
                RuntimeType.Document(runtimeConfig),
                RuntimeType.SerdeJson("Error")
            ) {
                rustTemplate(
                    """
                            #{serde_json}::from_slice::<#{doc_json}::DeserDoc>(inp).map(|d|d.0)
                        """,
                    "doc_json" to RuntimeType.DocJson,
                    "serde_json" to CargoDependency.SerdeJson.asType(),
                )
            }
        }
    }
}
