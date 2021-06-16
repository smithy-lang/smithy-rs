/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parse

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class SerdeJsonParserGenerator(protocolConfig: ProtocolConfig) : StructuredDataParserGenerator {
    private val model = protocolConfig.model
    private val symbolProvider = protocolConfig.symbolProvider
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val codegenScope = arrayOf("Error" to RuntimeType.SerdeJson("Error"), "serde_json" to RuntimeType.serdeJson)

    override fun payloadParser(member: MemberShape): RuntimeType {
        val shape = model.expectShape(member.target)
        check(shape is UnionShape || shape is StructureShape) { "payload parser should only be used on structures & unions" }
        val fnName =
            "parse_payload_" + shape.id.name.toString().toSnakeCase() + member.container.name.toString().toSnakeCase()
        return RuntimeType.forInlineFun(fnName, "json_deser") {
            it.rustTemplate(
                """
                    pub fn $fnName(inp: &[u8]) -> std::result::Result<#{Shape}, #{Error}> {
                        #{serde_json}::from_slice(inp)
                    }""",
                *codegenScope, "Shape" to symbolProvider.toSymbol(shape)
            )
        }
    }

    override fun operationParser(operationShape: OperationShape): RuntimeType? {
        val outputShape = operationShape.outputShape(model)
        val fnName = operationShape.id.name.toString().toSnakeCase() + "_deser_operation"
        val bodyId = outputShape.expectTrait<SyntheticOutputTrait>().body
        val bodyShape = bodyId?.let { model.expectShape(bodyId, StructureShape::class.java) } ?: return null
        val body = symbolProvider.toSymbol(bodyShape)
        if (bodyShape.members().isEmpty()) {
            return null
        }

        return RuntimeType.forInlineFun(fnName, "json_deser") {
            it.rustBlockTemplate(
                "pub fn $fnName(inp: &[u8], mut builder: #{Builder}) -> std::result::Result<#{Builder}, #{Error}>",
                "Builder" to outputShape.builderSymbol(symbolProvider),
                *codegenScope
            ) {
                rustTemplate(
                    """
                    let parsed_body: #{BodyShape} = if inp.is_empty() {
                        // To enable JSON parsing to succeed, replace an empty body
                        // with an empty JSON body. If a member was required, it will fail slightly later
                        // during the operation construction phase when a required field was missing.
                        #{serde_json}::from_slice(b"{}")?
                    } else {
                        #{serde_json}::from_slice(inp)?
                    };
                """,
                    "BodyShape" to body, *codegenScope
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
            it.rustBlockTemplate(
                "pub fn $fnName(inp: &[u8], mut builder: #{Builder}) -> std::result::Result<#{Builder}, #{Error}>",
                "Builder" to errorShape.builderSymbol(symbolProvider),
                *codegenScope
            ) {
                rustTemplate(
                    """
                    let parsed_body: #{BodyShape} = if inp.is_empty() {
                        // To enable JSON parsing to succeed, replace an empty body
                        // with an empty JSON body. If a member was required, it will fail slightly later
                        // during the operation construction phase.
                        #{serde_json}::from_slice(b"{}")?
                    } else {
                        #{serde_json}::from_slice(inp)?
                    };
                """,
                    "BodyShape" to symbolProvider.toSymbol(errorShape),
                    *codegenScope
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
            it.rustTemplate(
                """
                pub fn $fnName(inp: &[u8]) -> Result<#{Document}, #{Error}> {
                    #{serde_json}::from_slice::<#{doc_json}::DeserDoc>(inp).map(|d|d.0)
                }
            """,
                *codegenScope, "Document" to RuntimeType.Document(runtimeConfig), "doc_json" to RuntimeType.DocJson
            )
        }
    }
}
