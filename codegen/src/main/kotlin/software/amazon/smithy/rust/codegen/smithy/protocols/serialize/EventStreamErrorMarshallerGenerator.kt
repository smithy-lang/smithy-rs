/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EventHeaderTrait
import software.amazon.smithy.model.traits.EventPayloadTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.generators.error.eventStreamErrorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.smithy.generators.unknownVariantError
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticEventStreamUnionTrait
import software.amazon.smithy.rust.codegen.smithy.transformers.eventStreamErrors
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.toPascalCase

class EventStreamErrorMarshallerGenerator(
    private val model: Model,
    private val target: CodegenTarget,
    runtimeConfig: RuntimeConfig,
    private val symbolProvider: RustSymbolProvider,
    private val unionShape: UnionShape,
    private val operationShape: OperationShape,
    private val serializerGenerator: StructuredDataSerializerGenerator,
    private val payloadContentType: String,
) : EventStreamMarshallerGenerator(model, target, runtimeConfig, symbolProvider, unionShape, serializerGenerator, payloadContentType) {
    private val smithyEventStream = CargoDependency.SmithyEventStream(runtimeConfig)
    private val operationErrorSymbol = if (target == CodegenTarget.SERVER && unionShape.eventStreamErrors().isEmpty()) {
        RuntimeType("MessageStreamError", smithyEventStream, "aws_smithy_http::event_stream").toSymbol()
    } else {
        unionShape.eventStreamErrorSymbol(symbolProvider).toSymbol()
    }
    private val eventStreamSerdeModule = RustModule.private("event_stream_serde")
    val errorsShape = unionShape.expectTrait<SyntheticEventStreamUnionTrait>()
    private val codegenScope = arrayOf(
        "MarshallMessage" to RuntimeType("MarshallMessage", smithyEventStream, "aws_smithy_eventstream::frame"),
        "Message" to RuntimeType("Message", smithyEventStream, "aws_smithy_eventstream::frame"),
        "Header" to RuntimeType("Header", smithyEventStream, "aws_smithy_eventstream::frame"),
        "HeaderValue" to RuntimeType("HeaderValue", smithyEventStream, "aws_smithy_eventstream::frame"),
        "Error" to RuntimeType("Error", smithyEventStream, "aws_smithy_eventstream::error"),
    )

    override fun render(): RuntimeType {
        val marshallerType = operationShape.eventStreamMarshallerType()
        val unionSymbol = symbolProvider.toSymbol(unionShape)

        return RuntimeType.forInlineFun("${marshallerType.name}::new", eventStreamSerdeModule) { inlineWriter ->
            inlineWriter.renderMarshaller(marshallerType, unionSymbol)
        }
    }

    private fun RustWriter.renderMarshaller(marshallerType: RuntimeType, unionSymbol: Symbol) {
        rust(
            """
            ##[non_exhaustive]
            ##[derive(Debug)]
            pub struct ${marshallerType.name};

            impl ${marshallerType.name} {
                pub fn new() -> Self {
                    ${marshallerType.name}
                }
            }
            """
        )

        rustBlockTemplate(
            "impl #{MarshallMessage} for ${marshallerType.name}",
            *codegenScope
        ) {
            rust("type Input = ${operationErrorSymbol.rustType().render(fullyQualified = true)};")

            rustBlockTemplate(
                "fn marshall(&self, input: Self::Input) -> std::result::Result<#{Message}, #{Error}>",
                *codegenScope
            ) {
                rust("let mut headers = Vec::new();")
                addStringHeader(":message-type", """"exception".into()""")
                val kind = when (target) {
                    CodegenTarget.CLIENT -> ".kind"
                    CodegenTarget.SERVER -> ""
                }
                if (errorsShape.errorMembers.isEmpty()) {
                    rustBlock("let payload = match input$kind") {
                        rust("_ => Vec::new()")
                    }
                } else {
                    rustBlock("let payload = match input$kind") {
                        val symbol = operationErrorSymbol
                        val errorName = when (target) {
                            CodegenTarget.CLIENT -> "${symbol}Kind"
                            CodegenTarget.SERVER -> "$symbol"
                        }

                        errorsShape.errorMembers.forEach { error ->
                            val errorSymbol = symbolProvider.toSymbol(error)
                            val errorString = error.memberName
                            val target = model.expectShape(error.target, StructureShape::class.java)
                            rustBlock("$errorName::${errorSymbol.name}(inner) => ") {
                                addStringHeader(":exception-type", "${errorString.dq()}.into()")
                                renderMarshallEvent(error, target)
                            }
                        }
                        if (target.renderUnknownVariant()) {
                            rustTemplate(
                                """
                                $errorName::Unhandled(_inner) => return Err(
                                    #{Error}::Marshalling(${unknownVariantError(unionSymbol.rustType().name).dq()}.to_owned())
                                ),
                                """,
                                *codegenScope
                            )
                        }
                    }
                }
                rustTemplate("; Ok(#{Message}::new_from_parts(headers, payload))", *codegenScope)
            }
        }
    }

    fun RustWriter.renderMarshallEvent(unionMember: MemberShape, eventStruct: StructureShape) {
        val headerMembers = eventStruct.members().filter { it.hasTrait<EventHeaderTrait>() }
        val payloadMember = eventStruct.members().firstOrNull { it.hasTrait<EventPayloadTrait>() }
        for (member in headerMembers) {
            val memberName = symbolProvider.toMemberName(member)
            val target = model.expectShape(member.target)
            renderMarshallEventHeader(memberName, member, target)
        }
        if (payloadMember != null) {
            val memberName = symbolProvider.toMemberName(payloadMember)
            val target = model.expectShape(payloadMember.target)
            val serializerFn = serializerGenerator.serverErrorSerializer(payloadMember.toShapeId())
            renderMarshallEventPayload("inner.$memberName", payloadMember, target, serializerFn)
        } else if (headerMembers.isEmpty()) {
            val serializerFn = serializerGenerator.serverErrorSerializer(unionMember.target.toShapeId())
            renderMarshallEventPayload("inner", unionMember, eventStruct, serializerFn)
        } else {
            rust("Vec::new()")
        }
    }

    private fun OperationShape.eventStreamMarshallerType(): RuntimeType {
        val symbol = symbolProvider.toSymbol(this)
        return RuntimeType("${symbol.name.toPascalCase()}ErrorMarshaller", null, "crate::event_stream_serde")
    }
}
