/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EventHeaderTrait
import software.amazon.smithy.model.traits.EventPayloadTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.toPascalCase

// TODO(EventStream): [TEST] Unit test EventStreamMarshallerGenerator
class EventStreamMarshallerGenerator(
    private val model: Model,
    runtimeConfig: RuntimeConfig,
    private val symbolProvider: RustSymbolProvider,
    private val unionShape: UnionShape,
    private val serializerGenerator: StructuredDataSerializerGenerator,
    private val payloadContentType: String,
) {
    private val smithyEventStream = CargoDependency.SmithyEventStream(runtimeConfig)
    private val codegenScope = arrayOf(
        "MarshallMessage" to RuntimeType("MarshallMessage", smithyEventStream, "smithy_eventstream::frame"),
        "Message" to RuntimeType("Message", smithyEventStream, "smithy_eventstream::frame"),
        "Header" to RuntimeType("Header", smithyEventStream, "smithy_eventstream::frame"),
        "HeaderValue" to RuntimeType("HeaderValue", smithyEventStream, "smithy_eventstream::frame"),
        "Error" to RuntimeType("Error", smithyEventStream, "smithy_eventstream::error"),
    )

    fun render(): RuntimeType {
        val marshallerType = unionShape.eventStreamMarshallerType()
        val unionSymbol = symbolProvider.toSymbol(unionShape)

        return RuntimeType.forInlineFun("${marshallerType.name}::new", "event_stream_serde") { inlineWriter ->
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
            rust("type Input = ${unionSymbol.rustType().render(fullyQualified = true)};")

            rustBlockTemplate(
                "fn marshall(&self, input: Self::Input) -> std::result::Result<#{Message}, #{Error}>",
                *codegenScope
            ) {
                rust("let mut headers = Vec::new();")
                addStringHeader(":message-type", "\"event\".into()")
                rustBlock("let payload = match input") {
                    for (member in unionShape.members()) {
                        val eventType = member.memberName // must be the original name, not the Rust-safe name
                        rustBlock("Self::Input::${member.memberName.toPascalCase()}(inner) => ") {
                            addStringHeader(":event-type", "${eventType.dq()}.into()")
                            val target = model.expectShape(member.target, StructureShape::class.java)
                            serializeEvent(target)
                        }
                    }
                }
                rustTemplate("; Ok(#{Message}::new_from_parts(headers, payload))", *codegenScope)
            }
        }
    }

    private fun RustWriter.serializeEvent(struct: StructureShape) {
        for (member in struct.members()) {
            val memberName = symbolProvider.toMemberName(member)
            val target = model.expectShape(member.target)
            if (member.hasTrait<EventPayloadTrait>()) {
                serializeUnionMember(memberName, member, target)
            } else if (member.hasTrait<EventHeaderTrait>()) {
                TODO("TODO(EventStream): Implement @eventHeader trait")
            } else {
                throw IllegalStateException("Event Stream members must be a header or payload")
            }
        }
    }

    private fun RustWriter.serializeUnionMember(memberName: String, member: MemberShape, target: Shape) {
        if (target is BlobShape || target is StringShape) {
            data class PayloadContext(val conversionFn: String, val contentType: String)
            val ctx = when (target) {
                is BlobShape -> PayloadContext("into_inner", "application/octet-stream")
                is StringShape -> PayloadContext("into_bytes", "text/plain")
                else -> throw IllegalStateException("unreachable")
            }
            addStringHeader(":content-type", "${ctx.contentType.dq()}.into()")
            if (member.isOptional) {
                rust(
                    """
                    if let Some(inner_payload) = inner.$memberName {
                        inner_payload.${ctx.conversionFn}()
                    } else {
                        Vec::new()
                    }
                    """
                )
            } else {
                rust("inner.$memberName.${ctx.conversionFn}()")
            }
        } else {
            addStringHeader(":content-type", "${payloadContentType.dq()}.into()")

            val serializerFn = serializerGenerator.payloadSerializer(member)
            rustTemplate(
                """
                        #{serializerFn}(&inner.$memberName)
                            .map_err(|err| #{Error}::Marshalling(format!("{}", err)))?
                        """,
                "serializerFn" to serializerFn,
                *codegenScope
            )
        }
    }

    private fun RustWriter.addStringHeader(name: String, valueExpr: String) {
        rustTemplate("headers.push(#{Header}::new(${name.dq()}, #{HeaderValue}::String($valueExpr)));", *codegenScope)
    }

    private fun UnionShape.eventStreamMarshallerType(): RuntimeType {
        val symbol = symbolProvider.toSymbol(this)
        return RuntimeType("${symbol.name.toPascalCase()}Marshaller", null, "crate::event_stream_serde")
    }
}
