/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EventHeaderTrait
import software.amazon.smithy.model.traits.EventPayloadTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.generators.unknownVariantError
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.eventStreamSerdeModule
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticEventStreamUnionTrait
import software.amazon.smithy.rust.codegen.core.smithy.transformers.eventStreamErrors
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.toPascalCase

class EventStreamErrorMarshallerGenerator(
    private val model: Model,
    private val target: CodegenTarget,
    runtimeConfig: RuntimeConfig,
    private val symbolProvider: RustSymbolProvider,
    private val unionShape: UnionShape,
    private val serializerGenerator: StructuredDataSerializerGenerator,
    payloadContentType: String,
) : EventStreamMarshallerGenerator(model, target, runtimeConfig, symbolProvider, unionShape, serializerGenerator, payloadContentType) {
    private val smithyEventStream = RuntimeType.smithyEventStream(runtimeConfig)

    private val operationErrorSymbol = if (target == CodegenTarget.SERVER && unionShape.eventStreamErrors().isEmpty()) {
        RuntimeType.smithyHttp(runtimeConfig).resolve("event_stream::MessageStreamError").toSymbol()
    } else {
        symbolProvider.symbolForEventStreamError(unionShape)
    }
    private val eventStreamSerdeModule = RustModule.eventStreamSerdeModule()
    private val errorsShape = unionShape.expectTrait<SyntheticEventStreamUnionTrait>()
    private val codegenScope = arrayOf(
        "MarshallMessage" to smithyEventStream.resolve("frame::MarshallMessage"),
        "Message" to smithyEventStream.resolve("frame::Message"),
        "Header" to smithyEventStream.resolve("frame::Header"),
        "HeaderValue" to smithyEventStream.resolve("frame::HeaderValue"),
        "Error" to smithyEventStream.resolve("error::Error"),
    )

    override fun render(): RuntimeType {
        val marshallerType = unionShape.eventStreamMarshallerType()
        val unionSymbol = symbolProvider.toSymbol(unionShape)

        return RuntimeType.forInlineFun("${marshallerType.name}::new", eventStreamSerdeModule) {
            renderMarshaller(marshallerType, unionSymbol)
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
            """,
        )

        rustBlockTemplate(
            "impl #{MarshallMessage} for ${marshallerType.name}",
            *codegenScope,
        ) {
            rust("type Input = ${operationErrorSymbol.rustType().render(fullyQualified = true)};")

            rustBlockTemplate(
                "fn marshall(&self, _input: Self::Input) -> std::result::Result<#{Message}, #{Error}>",
                *codegenScope,
            ) {
                rust("let mut headers = Vec::new();")
                addStringHeader(":message-type", """"exception".into()""")
                if (errorsShape.errorMembers.isEmpty()) {
                    rust("let payload = Vec::new();")
                } else {
                    rustBlock("let payload = match _input") {
                        errorsShape.errorMembers.forEach { error ->
                            val errorString = error.memberName
                            val target = model.expectShape(error.target, StructureShape::class.java)
                            val targetSymbol = symbolProvider.toSymbol(target)
                            rustBlock("#T::${targetSymbol.name}(inner) => ", operationErrorSymbol) {
                                addStringHeader(":exception-type", "${errorString.dq()}.into()")
                                renderMarshallEvent(error, target)
                            }
                        }
                        if (target.renderUnknownVariant()) {
                            rustTemplate(
                                """
                                #{OperationError}::Unhandled(_inner) => return Err(
                                    #{Error}::marshalling(${unknownVariantError(unionSymbol.rustType().name).dq()}.to_owned())
                                ),
                                """,
                                *codegenScope,
                                "OperationError" to operationErrorSymbol,
                            )
                        }
                    }
                }
                rustTemplate("; Ok(#{Message}::new_from_parts(headers, payload))", *codegenScope)
            }
        }
    }

    private fun RustWriter.renderMarshallEvent(unionMember: MemberShape, eventStruct: StructureShape) {
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

    private fun UnionShape.eventStreamMarshallerType(): RuntimeType {
        val symbol = symbolProvider.toSymbol(this)
        return RuntimeType("crate::event_stream_serde::${symbol.name.toPascalCase()}ErrorMarshaller")
    }
}
