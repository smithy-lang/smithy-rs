/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EventHeaderTrait
import software.amazon.smithy.model.traits.EventPayloadTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.generators.unknownVariantError
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.eventStreamSerdeModule
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.toPascalCase

open class EventStreamMarshallerGenerator(
    private val model: Model,
    private val target: CodegenTarget,
    runtimeConfig: RuntimeConfig,
    private val symbolProvider: RustSymbolProvider,
    private val unionShape: UnionShape,
    private val serializerGenerator: StructuredDataSerializerGenerator,
    private val payloadContentType: String,
) {
    private val smithyEventStream = RuntimeType.smithyEventStream(runtimeConfig)
    private val smithyTypes = RuntimeType.smithyTypes(runtimeConfig)
    private val eventStreamSerdeModule = RustModule.eventStreamSerdeModule()
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "Bytes" to RuntimeType.Bytes,
            "MarshallMessage" to smithyEventStream.resolve("frame::MarshallMessage"),
            "Message" to smithyTypes.resolve("event_stream::Message"),
            "Header" to smithyTypes.resolve("event_stream::Header"),
            "HeaderValue" to smithyTypes.resolve("event_stream::HeaderValue"),
            "Error" to smithyEventStream.resolve("error::Error"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        )

    open fun render(): RuntimeType {
        val marshallerType = unionShape.eventStreamMarshallerType()
        val unionSymbol = symbolProvider.toSymbol(unionShape)

        return RuntimeType.forInlineFun("${marshallerType.name}::new", eventStreamSerdeModule) {
            renderMarshaller(marshallerType, unionSymbol)
        }
    }

    fun renderInitialRequestGenerator(contentType: String): RuntimeType {
        return RuntimeType.forInlineFun("initial_message_from_body", eventStreamSerdeModule) {
            rustBlockTemplate(
                """
                pub(crate) fn initial_message_from_body(
                    body: #{SdkBody}
                ) -> #{Message}
                """,
                *codegenScope,
            ) {
                rustTemplate("let mut headers = #{Vec}::new();", *codegenScope)
                addStringHeader(":message-type", "\"event\".into()")
                addStringHeader(":event-type", "\"initial-request\".into()")
                addStringHeader(":content-type", "${contentType.dq()}.into()")
                rustTemplate(
                    """
                    let body = #{Bytes}::from(
                        body.bytes()
                            .expect("body should've been created from non-streaming payload for initial message")
                            .to_vec(),
                    );
                    #{Message}::new_from_parts(headers, body)
                    """,
                    *codegenScope,
                )
            }
        }
    }

    fun renderInitialResponseGenerator(contentType: String): RuntimeType {
        return RuntimeType.forInlineFun("initial_response_from_payload", eventStreamSerdeModule) {
            rustBlockTemplate(
                """
                pub(crate) fn initial_response_from_payload(
                    payload: #{Bytes}
                ) -> #{Message}
                """,
                *codegenScope,
            ) {
                rustTemplate("let mut headers = #{Vec}::new();", *codegenScope)
                addStringHeader(":message-type", "\"event\".into()")
                addStringHeader(":event-type", "\"initial-response\".into()")
                addStringHeader(":content-type", "${contentType.dq()}.into()")
                rustTemplate("#{Message}::new_from_parts(headers, payload)", *codegenScope)
            }
        }
    }

    private fun RustWriter.renderMarshaller(
        marshallerType: RuntimeType,
        unionSymbol: Symbol,
    ) {
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
            rust("type Input = ${unionSymbol.rustType().render(fullyQualified = true)};")

            rustBlockTemplate(
                "fn marshall(&self, input: Self::Input) -> std::result::Result<#{Message}, #{Error}>",
                *codegenScope,
            ) {
                rust("let mut headers = Vec::new();")
                addStringHeader(":message-type", "\"event\".into()")
                rustBlock("let payload = match input") {
                    for (member in unionShape.members()) {
                        val eventType = member.memberName // must be the original name, not the Rust-safe name
                        // Union members targeting the Smithy `Unit` type do not have associated data in the
                        // Rust enum generated for the type.
                        val mayHaveInner =
                            if (!member.isTargetUnit()) {
                                "(inner)"
                            } else {
                                ""
                            }
                        rustBlock("Self::Input::${symbolProvider.toMemberName(member)}$mayHaveInner => ") {
                            addStringHeader(":event-type", "${eventType.dq()}.into()")
                            val target = model.expectShape(member.target, StructureShape::class.java)
                            renderMarshallEvent(member, target)
                        }
                    }
                    if (target.renderUnknownVariant()) {
                        rustTemplate(
                            """
                            Self::Input::${UnionGenerator.UNKNOWN_VARIANT_NAME} => return Err(
                                #{Error}::marshalling(${unknownVariantError(unionSymbol.rustType().name).dq()}.to_owned())
                            )
                            """,
                            *codegenScope,
                        )
                    }
                }
                rustTemplate("; Ok(#{Message}::new_from_parts(headers, payload))", *codegenScope)
            }
        }
    }

    private fun RustWriter.renderMarshallEvent(
        unionMember: MemberShape,
        eventStruct: StructureShape,
    ) {
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
            val serializerFn = serializerGenerator.payloadSerializer(payloadMember)
            renderMarshallEventPayload("inner.$memberName", payloadMember, target, serializerFn)
        } else if (headerMembers.isEmpty()) {
            val serializerFn = serializerGenerator.payloadSerializer(unionMember)
            // Union members targeting the Smithy `Unit` type do not have associated data in the
            // Rust enum generated for the type. For these, we need to pass the `crate::model::Unit` data type.
            val inner =
                if (unionMember.isTargetUnit()) {
                    "crate::model::Unit::builder().build()"
                } else {
                    "inner"
                }
            renderMarshallEventPayload(inner, unionMember, eventStruct, serializerFn)
        } else {
            rust("Vec::new()")
        }
    }

    protected fun RustWriter.renderMarshallEventHeader(
        memberName: String,
        member: MemberShape,
        target: Shape,
    ) {
        val headerName = member.memberName
        handleOptional(
            symbolProvider.toSymbol(member).isOptional(),
            "inner.$memberName",
            "value",
            { input -> renderAddHeader(headerName, input, target) },
        )
    }

    private fun RustWriter.renderAddHeader(
        headerName: String,
        inputName: String,
        target: Shape,
    ) {
        withBlock("headers.push(", ");") {
            rustTemplate(
                "#{Header}::new(${headerName.dq()}, #{HeaderValue}::${headerValue(inputName, target)})",
                *codegenScope,
            )
        }
    }

    // Event stream header types: https://smithy.io/2.0/spec/streaming.html#eventheader-trait
    // Note: there are no floating point header types for Event Stream.
    private fun headerValue(
        inputName: String,
        target: Shape,
    ): String =
        when (target) {
            is BooleanShape -> "Bool($inputName)"
            is ByteShape -> "Byte($inputName)"
            is ShortShape -> "Int16($inputName)"
            is IntegerShape -> "Int32($inputName)"
            is LongShape -> "Int64($inputName)"
            is BlobShape -> "ByteArray($inputName.into_inner().into())"
            is EnumShape -> "String($inputName.to_string().into())"
            is StringShape -> "String($inputName.into())"
            is TimestampShape -> "Timestamp($inputName)"
            else -> throw IllegalStateException("unsupported event stream header shape type: $target")
        }

    protected fun RustWriter.renderMarshallEventPayload(
        inputExpr: String,
        member: Shape,
        target: Shape,
        serializerFn: RuntimeType,
    ) {
        val optional = symbolProvider.toSymbol(member).isOptional()
        if (target is BlobShape || target is StringShape) {
            data class PayloadContext(val conversionFn: String, val contentType: String)

            val ctx =
                when (target) {
                    is BlobShape -> PayloadContext("into_inner", "application/octet-stream")
                    is StringShape -> PayloadContext("into_bytes", "text/plain")
                    else -> throw IllegalStateException("unreachable")
                }
            addStringHeader(":content-type", "${ctx.contentType.dq()}.into()")
            handleOptional(
                optional,
                inputExpr,
                "inner_payload",
                { input -> rust("$input.${ctx.conversionFn}()") },
                { rust("Vec::new()") },
            )
        } else {
            addStringHeader(":content-type", "${payloadContentType.dq()}.into()")

            handleOptional(
                optional,
                inputExpr,
                "inner_payload",
                { input ->
                    rustTemplate(
                        """
                        #{serializerFn}(&$input)
                            .map_err(|err| #{Error}::marshalling(format!("{}", err)))?
                        """,
                        "serializerFn" to serializerFn,
                        *codegenScope,
                    )
                },
                { rust("unimplemented!(\"TODO(EventStream): Figure out what to do when there's no payload\")") },
            )
        }
    }

    private fun RustWriter.handleOptional(
        optional: Boolean,
        inputExpr: String,
        someName: String,
        writeSomeCase: RustWriter.(String) -> Unit,
        writeNoneCase: (Writable)? = null,
    ) {
        if (optional) {
            rustBlock("if let Some($someName) = $inputExpr") {
                writeSomeCase(someName)
            }
            if (writeNoneCase != null) {
                rustBlock(" else ") {
                    writeNoneCase()
                }
            }
        } else {
            writeSomeCase(inputExpr)
        }
    }

    protected fun RustWriter.addStringHeader(
        name: String,
        valueExpr: String,
    ) {
        rustTemplate("headers.push(#{Header}::new(${name.dq()}, #{HeaderValue}::String($valueExpr)));", *codegenScope)
    }

    private fun UnionShape.eventStreamMarshallerType(): RuntimeType {
        val symbol = symbolProvider.toSymbol(this)
        return RuntimeType("crate::event_stream_serde::${symbol.name.toPascalCase()}Marshaller")
    }
}
