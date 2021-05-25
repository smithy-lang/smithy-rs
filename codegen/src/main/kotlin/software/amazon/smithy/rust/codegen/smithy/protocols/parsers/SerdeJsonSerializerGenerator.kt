/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parsers

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class SerdeJsonSerializerGenerator(protocolConfig: ProtocolConfig) : StructuredDataSerializerGenerator {
    private val model = protocolConfig.model
    private val symbolProvider = protocolConfig.symbolProvider
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val serializerError = RuntimeType.SerdeJson("error::Error")
    private val codegenScope = arrayOf(
        "Error" to serializerError,
        "serde_json" to RuntimeType.serdeJson,
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig)
    )

    override fun payloadSerializer(member: MemberShape): RuntimeType {
        val target = model.expectShape(member.target)
        val fnName = "serialize_payload_${target.id.name.toSnakeCase()}_${member.container.name.toSnakeCase()}"
        return RuntimeType.forInlineFun(fnName, "operation_ser") {
            it.rustTemplate(
                """
                pub fn $fnName(input: &#{target}) -> Result<#{SdkBody}, #{Error}> {
                    #{serde_json}::to_vec(&input).map(#{SdkBody}::from)
                }
            """,
                *codegenScope, "target" to symbolProvider.toSymbol(target)
            )
        }
    }

    override fun operationSerializer(operationShape: OperationShape): RuntimeType? {
        // Currently, JSON shapes are serialized via a synthetic body structure that gets generated during model
        // transformation
        val inputShape = operationShape.inputShape(model)
        val inputBody = inputShape.expectTrait<SyntheticInputTrait>().body?.let {
            model.expectShape(
                it,
                StructureShape::class.java
            )
        } ?: return null
        val fnName = "serialize_operation_${inputBody.id.name.toSnakeCase()}"
        return RuntimeType.forInlineFun(fnName, "operation_ser") {
            it.rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(inputShape)
            ) {
                // copy the input (via references) into the synthetic body:
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
                    *codegenScope
                )
            }
        }
    }

    override fun documentSerializer(): RuntimeType {
        val fnName = "serialize_document"
        return RuntimeType.forInlineFun(fnName, "operation_ser") {
            it.rustTemplate(
                """
                pub fn $fnName(input: &#{Document}) -> Result<#{SdkBody}, #{Error}> {
                    #{serde_json}::to_vec(&#{doc_json}::SerDoc(&input)).map(#{SdkBody}::from)
                }

            """,
                "Document" to RuntimeType.Document(runtimeConfig), "doc_json" to RuntimeType.DocJson, *codegenScope
            )
        }
    }
}
