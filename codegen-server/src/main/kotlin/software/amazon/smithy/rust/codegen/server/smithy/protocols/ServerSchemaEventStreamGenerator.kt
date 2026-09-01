/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.EventHeaderTrait
import software.amazon.smithy.model.traits.EventPayloadTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.generators.unknownVariantError
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.eventStreamSerdeModule
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.EventStreamMarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticEventStreamUnionTrait
import software.amazon.smithy.rust.codegen.core.smithy.transformers.eventStreamErrors
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeModuleName

private fun schemaEventStreamTypeName(
    symbolProvider: RustSymbolProvider,
    unionShape: UnionShape,
    marker: RuntimeType,
    suffix: String,
): String = "${symbolProvider.toSymbol(unionShape).name}${marker.name}$suffix"

private fun schemaSerdeModule() = RustModule.eventStreamSerdeModule()

internal fun serverSchemaDeserFnPath(
    codegenContext: ServerCodegenContext,
    target: Shape,
): String {
    val symbolProvider = codegenContext.symbolProvider
    return "crate::schema_serde::${symbolProvider.shapeModuleName(codegenContext.serviceShape, target)}::deser_${symbolProvider.toSymbol(target).name.toSnakeCase()}"
}

private fun canReachConstrained(
    codegenContext: ServerCodegenContext,
    target: Shape,
): Boolean = target.canReachConstrainedShape(codegenContext.model, codegenContext.symbolProvider)

private fun RustWriter.addStringHeader(
    runtimeConfig: RuntimeConfig,
    name: String,
    valueExpr: String,
    varName: String = "headers",
) {
    rustTemplate(
        "$varName.push(#{Header}::new(${name.dq()}, #{HeaderValue}::String($valueExpr)));",
        "Header" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Header"),
        "HeaderValue" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::HeaderValue"),
    )
}

/**
 * Server-only schema-serde event-stream data marshaller.
 *
 * This intentionally lives in `codegen-server` rather than changing the shared
 * core generator. The shared generator's schema mode is client-shaped and
 * stores a `SharedClientProtocol`; server dispatch is static and uses the
 * generated protocol marker's `ServerProtocol::codec()`.
 */
class ServerSchemaEventStreamMarshallerGenerator(
    private val codegenContext: ServerCodegenContext,
    private val protocol: ServerProtocol,
    unionShape: UnionShape,
    serializerGenerator: StructuredDataSerializerGenerator,
    payloadContentType: String,
) : EventStreamMarshallerGenerator(
        codegenContext.model,
        CodegenTarget.SERVER,
        codegenContext.runtimeConfig,
        codegenContext.symbolProvider,
        unionShape,
        serializerGenerator,
        payloadContentType,
        useSchemaSerde = false,
    ) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val marker = protocol.markerStruct()
    private val serverProtocol = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
        .resolve("schema::protocol::ServerProtocol")
    private val serverEventStreamProtocol = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
        .resolve("schema::protocol::ServerEventStreamProtocol")
    private val union = unionShape
    private val typeName = schemaEventStreamTypeName(symbolProvider, union, marker, "SchemaMarshaller")
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "Blob" to RuntimeType.blob(runtimeConfig),
            "Bytes" to RuntimeType.Bytes,
            "MarshallMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::MarshallMessage"),
            "Message" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Message"),
            "Header" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Header"),
            "HeaderValue" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::HeaderValue"),
            "Error" to RuntimeType.smithyEventStream(runtimeConfig).resolve("error::Error"),
            "ShapeSerializer" to RuntimeType.smithySchema(runtimeConfig).resolve("serde::ShapeSerializer"),
            "FinishSerializer" to RuntimeType.smithySchema(runtimeConfig).resolve("codec::FinishSerializer"),
            "ServerProtocol" to serverProtocol,
            "ServerEventStreamProtocol" to serverEventStreamProtocol,
            "Marker" to marker,
        )

    override fun render(): RuntimeType =
        RuntimeType.forInlineFun("new_${typeName.toSnakeCase()}", schemaSerdeModule()) {
            renderType()
        }

    private fun RustWriter.renderType() {
        val unionSymbol = symbolProvider.toSymbol(union)
        rustTemplate(
            """
            ##[non_exhaustive]
            ##[derive(Debug)]
            pub struct $typeName;

            impl $typeName {
                pub fn new() -> Self {
                    Self
                }
            }

            pub(crate) fn new_${typeName.toSnakeCase()}() -> $typeName {
                $typeName::new()
            }
            """,
            *codegenScope,
        )
        rustBlockTemplate(
            "impl #{MarshallMessage} for $typeName",
            *codegenScope,
        ) {
            rust("type Input = ${unionSymbol.rustType().render(fullyQualified = true)};")
            rustBlockTemplate(
                "fn marshall(&self, input: Self::Input) -> #{Result}<#{Message}, #{Error}>",
                *codegenScope,
            ) {
                rustTemplate("let mut headers = #{Vec}::new();", *codegenScope)
                addStringHeader(runtimeConfig, ":message-type", """"event".into()""")
                rustBlock("let payload = match input") {
                    for (member in union.members()) {
                        val variant = symbolProvider.toMemberName(member)
                        val mayHaveInner = if (!member.isTargetUnit()) "(inner)" else ""
                        rustBlock("Self::Input::$variant$mayHaveInner => ") {
                            addStringHeader(runtimeConfig, ":event-type", "${member.memberName.dq()}.into()")
                            val target = model.expectShape(member.target, StructureShape::class.java)
                            renderEventPayload(member, target)
                        }
                    }
                    if (CodegenTarget.SERVER.renderUnknownVariant()) {
                        rustTemplate(
                            """
                            Self::Input::${UnionGenerator.UNKNOWN_VARIANT_NAME} => return #{Err}(
                                #{Error}::marshalling(${unknownVariantError(unionSymbol.rustType().name).dq()}.to_owned())
                            )
                            """,
                            *codegenScope,
                        )
                    }
                }
                rustTemplate("; #{Ok}(#{Message}::new_from_parts(headers, payload))", *codegenScope)
            }
        }
    }

    private fun RustWriter.renderEventPayload(
        unionMember: MemberShape,
        eventStruct: StructureShape,
    ) {
        val headerMembers = eventStruct.members().filter { it.hasTrait<EventHeaderTrait>() }
        val payloadMember = eventStruct.members().firstOrNull { it.hasTrait<EventPayloadTrait>() }
        for (member in headerMembers) {
            renderEventHeader(member)
        }
        when {
            payloadMember != null -> {
                val memberName = symbolProvider.toMemberName(payloadMember)
                renderPayloadValue("inner.$memberName", payloadMember, model.expectShape(payloadMember.target))
            }
            headerMembers.isEmpty() -> {
                val inner = if (unionMember.isTargetUnit()) "crate::model::Unit::builder().build()" else "inner"
                renderPayloadValue(inner, unionMember, eventStruct)
            }
            else -> rustTemplate("#{Bytes}::new()", *codegenScope)
        }
    }

    private fun RustWriter.renderEventHeader(member: MemberShape) {
        val memberName = symbolProvider.toMemberName(member)
        val target = model.expectShape(member.target)
        val optional = symbolProvider.toSymbol(member).isOptional()
        if (optional) {
            rustBlock("if let Some(value) = inner.$memberName") {
                renderAddHeader(member.memberName, "value", target)
            }
        } else {
            renderAddHeader(member.memberName, "inner.$memberName", target)
        }
    }

    private fun RustWriter.renderAddHeader(
        headerName: String,
        inputName: String,
        target: Shape,
    ) {
        rustTemplate(
            "headers.push(#{Header}::new(${headerName.dq()}, #{HeaderValue}::${headerValue(inputName, target)}));",
            *codegenScope,
        )
    }

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
            is BlobShape -> "ByteArray(#{Blob}::from($inputName).into_bytes())"
            is EnumShape -> "String($inputName.to_string().into())"
            is StringShape -> "String($inputName.into())"
            is TimestampShape -> "Timestamp($inputName.into())"
            else -> throw IllegalStateException("unsupported event stream header shape type: $target")
        }

    private fun RustWriter.renderPayloadValue(
        inputExpr: String,
        member: Shape,
        target: Shape,
    ) {
        val optional = (member as? MemberShape)?.let { symbolProvider.toSymbol(it).isOptional() } ?: false
        fun some(input: String) {
            when (target) {
                is BlobShape -> {
                    addStringHeader(runtimeConfig, ":content-type", """"application/octet-stream".into()""")
                    rustTemplate("#{Blob}::from($input).into_bytes()", "Blob" to RuntimeType.blob(runtimeConfig), *codegenScope)
                }
                is StringShape -> {
                    addStringHeader(runtimeConfig, ":content-type", """"text/plain".into()""")
                    rustTemplate("#{Bytes}::from($input.into_bytes())", *codegenScope)
                }
                else -> {
                    rustTemplate(
                        """
                        headers.push(#{Header}::new(
                            ":content-type",
                            #{HeaderValue}::String(<#{Marker} as #{ServerEventStreamProtocol}>::EVENT_PAYLOAD_CONTENT_TYPE.into()),
                        ));
                        """,
                        *codegenScope,
                    )
                    rustTemplate(
                        """
                        {
                            use #{FinishSerializer} as _;
                            let mut ser = <#{Marker} as #{ServerProtocol}>::codec().create_serializer();
                            #{ShapeSerializer}::write_struct(&mut ser, #{Target}::SCHEMA, &$input)
                                .map_err(|err| #{Error}::marshalling(format!("{err}")))?;
                            #{Bytes}::from(ser.finish())
                        }
                        """,
                        "Target" to symbolProvider.toSymbol(target),
                        *codegenScope,
                    )
                }
            }
        }
        if (optional) {
            rustBlock("if let Some(inner_payload) = $inputExpr") {
                some("inner_payload")
            }
            rustBlock(" else ") {
                rustTemplate("#{Bytes}::new()", *codegenScope)
            }
        } else {
            some(inputExpr)
        }
    }
}

class ServerSchemaEventStreamErrorMarshallerGenerator(
    private val codegenContext: ServerCodegenContext,
    private val protocol: ServerProtocol,
    private val union: UnionShape,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val marker = protocol.markerStruct()
    private val errorsShape = union.expectTrait<SyntheticEventStreamUnionTrait>()
    private val operationErrorSymbol =
        if (union.eventStreamErrors().isEmpty()) {
            RuntimeType.smithyHttp(runtimeConfig).resolve("event_stream::MessageStreamError").toSymbol()
        } else {
            symbolProvider.symbolForEventStreamError(union)
        }
    private val typeName = schemaEventStreamTypeName(symbolProvider, union, marker, "SchemaErrorMarshaller")
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "Bytes" to RuntimeType.Bytes,
            "MarshallMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::MarshallMessage"),
            "Message" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Message"),
            "Header" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Header"),
            "HeaderValue" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::HeaderValue"),
            "Error" to RuntimeType.smithyEventStream(runtimeConfig).resolve("error::Error"),
            "ShapeSerializer" to RuntimeType.smithySchema(runtimeConfig).resolve("serde::ShapeSerializer"),
            "FinishSerializer" to RuntimeType.smithySchema(runtimeConfig).resolve("codec::FinishSerializer"),
            "ServerProtocol" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
                .resolve("schema::protocol::ServerProtocol"),
            "ServerEventStreamProtocol" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
                .resolve("schema::protocol::ServerEventStreamProtocol"),
            "Marker" to marker,
        )

    fun render(): RuntimeType =
        RuntimeType.forInlineFun("new_${typeName.toSnakeCase()}", schemaSerdeModule()) {
            renderType()
        }

    private fun RustWriter.renderType() {
        rust(
            """
            ##[non_exhaustive]
            ##[derive(Debug)]
            pub struct $typeName;

            impl $typeName {
                pub fn new() -> Self {
                    Self
                }
            }

            pub(crate) fn new_${typeName.toSnakeCase()}() -> $typeName {
                $typeName::new()
            }
            """,
        )
        rustBlockTemplate(
            "impl #{MarshallMessage} for $typeName",
            *codegenScope,
        ) {
            rust("type Input = ${operationErrorSymbol.rustType().render(fullyQualified = true)};")
            rustBlockTemplate(
                "fn marshall(&self, _input: Self::Input) -> #{Result}<#{Message}, #{Error}>",
                *codegenScope,
            ) {
                rustTemplate("let mut headers = #{Vec}::new();", *codegenScope)
                addStringHeader(runtimeConfig, ":message-type", """"exception".into()""")
                if (errorsShape.errorMembers.isEmpty()) {
                    rustTemplate("let payload = #{Bytes}::new();", *codegenScope)
                } else {
                    rustBlock("let payload = match _input") {
                        errorsShape.errorMembers.forEach { error ->
                            val target = model.expectShape(error.target, StructureShape::class.java)
                            val targetSymbol = symbolProvider.toSymbol(target)
                            rustBlock("#T::${targetSymbol.name}(inner) => ", operationErrorSymbol) {
                                addStringHeader(runtimeConfig, ":exception-type", "${error.memberName.dq()}.into()")
                                renderPayload("inner", target)
                            }
                        }
                        if (CodegenTarget.SERVER.renderUnknownVariant()) {
                            rustTemplate(
                                """
                                #{OperationError}::Unhandled(_inner) => return #{Err}(
                                    #{Error}::marshalling(${unknownVariantError(symbolProvider.toSymbol(union).rustType().name).dq()}.to_owned())
                                ),
                                """,
                                *codegenScope,
                                "OperationError" to operationErrorSymbol,
                            )
                        }
                    }
                }
                rustTemplate("; #{Ok}(#{Message}::new_from_parts(headers, payload))", *codegenScope)
            }
        }
    }

    private fun RustWriter.renderPayload(
        input: String,
        target: StructureShape,
    ) {
        rustTemplate(
            """
            headers.push(#{Header}::new(
                ":content-type",
                #{HeaderValue}::String(<#{Marker} as #{ServerEventStreamProtocol}>::EVENT_PAYLOAD_CONTENT_TYPE.into()),
            ));
            """,
            *codegenScope,
        )
        rustTemplate(
            """
            {
                use #{FinishSerializer} as _;
                let mut ser = <#{Marker} as #{ServerProtocol}>::codec().create_serializer();
                #{ShapeSerializer}::write_struct(&mut ser, #{Target}::SCHEMA, &$input)
                    .map_err(|err| #{Error}::marshalling(format!("{err}")))?;
                #{Bytes}::from(ser.finish())
            }
            """,
            "Target" to symbolProvider.toSymbol(target),
            *codegenScope,
        )
    }
}

class ServerSchemaEventStreamUnmarshallerGenerator(
    private val codegenContext: ServerCodegenContext,
    private val protocol: ServerProtocol,
    private val operationShape: OperationShape,
    private val union: UnionShape,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val marker = protocol.markerStruct()
    private val unionSymbol = symbolProvider.toSymbol(union)
    private val errorSymbol =
        if (union.eventStreamErrors().isEmpty()) {
            RuntimeType.smithyHttp(runtimeConfig).resolve("event_stream::MessageStreamError").toSymbol()
        } else {
            symbolProvider.symbolForEventStreamError(union)
        }
    private val typeName = schemaEventStreamTypeName(symbolProvider, union, marker, "SchemaUnmarshaller")
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "Blob" to RuntimeType.blob(runtimeConfig),
            "expect_fns" to RuntimeType.smithyEventStream(runtimeConfig).resolve("smithy"),
            "Message" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Message"),
            "HeaderValue" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::HeaderValue"),
            "Error" to RuntimeType.smithyEventStream(runtimeConfig).resolve("error::Error"),
            "UnmarshalledMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::UnmarshalledMessage"),
            "UnmarshallMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::UnmarshallMessage"),
            "ServerProtocol" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
                .resolve("schema::protocol::ServerProtocol"),
            "Marker" to marker,
        )

    fun render(): RuntimeType =
        RuntimeType.forInlineFun("new_${typeName.toSnakeCase()}", schemaSerdeModule()) {
            renderType()
        }

    private fun RustWriter.renderType() {
        rust(
            """
            ##[non_exhaustive]
            ##[derive(Debug)]
            pub struct $typeName;

            impl $typeName {
                pub fn new() -> Self {
                    Self
                }
            }

            pub(crate) fn new_${typeName.toSnakeCase()}() -> $typeName {
                $typeName::new()
            }
            """,
        )
        rustBlockTemplate(
            "impl #{UnmarshallMessage} for $typeName",
            *codegenScope,
        ) {
            rust("type Output = ${unionSymbol.rustType().render(fullyQualified = true)};")
            rust("type Error = ${errorSymbol.rustType().render(fullyQualified = true)};")
            rustBlockTemplate(
                "fn unmarshall(&self, message: &#{Message}) -> #{Result}<#{UnmarshalledMessage}<Self::Output, Self::Error>, #{Error}>",
                *codegenScope,
            ) {
                rustTemplate("let response_headers = #{expect_fns}::parse_response_headers(message)?;", *codegenScope)
                rustBlock("match response_headers.message_type.as_str()") {
                    rustBlock("\"event\" => ") {
                        renderEventMatch()
                    }
                    rustBlock("\"exception\" => ") {
                        rustTemplate(
                            """
                            return #{Err}(#{Error}::unmarshalling(
                                format!("unrecognized exception: {}", response_headers.smithy_type.as_str()),
                            ));
                            """,
                            *codegenScope,
                        )
                    }
                    rustBlock("value => ") {
                        rustTemplate(
                            "return #{Err}(#{Error}::unmarshalling(format!(\"unrecognized :message-type: {value}\")));",
                            *codegenScope,
                        )
                    }
                }
            }
        }
    }

    private fun RustWriter.renderEventMatch() {
        rustBlock("match response_headers.smithy_type.as_str()") {
            for (member in union.members()) {
                val target = model.expectShape(member.target, StructureShape::class.java)
                rustBlock("${member.memberName.dq()} => ") {
                    renderUnionMember(member, target)
                }
            }
            rustBlock("_unknown_variant => ") {
                rustTemplate(
                    "return #{Err}(#{Error}::unmarshalling(format!(\"unrecognized :event-type: {_unknown_variant}\")));",
                    *codegenScope,
                )
            }
        }
    }

    private fun RustWriter.renderUnionMember(
        unionMember: MemberShape,
        eventStruct: StructureShape,
    ) {
        val unionMemberName = symbolProvider.toMemberName(unionMember)
        val empty = eventStruct.members().isEmpty()
        val payloadOnly =
            eventStruct.members().none { it.hasTrait<EventPayloadTrait>() || it.hasTrait<EventHeaderTrait>() }
        when {
            empty -> {
                if (unionMember.isTargetUnit()) {
                    rustTemplate(
                        "#{Ok}(#{UnmarshalledMessage}::Event(#{Output}::$unionMemberName))",
                        "Output" to unionSymbol,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        "#{Ok}(#{UnmarshalledMessage}::Event(#{Output}::$unionMemberName(#{UnionStruct}::builder().build())))",
                        "Output" to unionSymbol,
                        "UnionStruct" to symbolProvider.toSymbol(eventStruct),
                        *codegenScope,
                    )
                }
            }
            payloadOnly -> {
                rustTemplate("let parsed = #{parsePayload:W};", "parsePayload" to parsePayload(eventStruct, "message.payload()"), *codegenScope)
                if (canReachConstrained(codegenContext, eventStruct)) {
                    rustTemplate(
                        """
                        let parsed = parsed.build()
                            .map_err(|err| #{Error}::unmarshalling(format!("failed to unmarshall ${unionMember.memberName} due to constraint violation: {err}")))?;
                        """,
                        *codegenScope,
                    )
                }
                rustTemplate(
                    "#{Ok}(#{UnmarshalledMessage}::Event(#{Output}::$unionMemberName(parsed)))",
                    "Output" to unionSymbol,
                    *codegenScope,
                )
            }
            else -> {
                rust("let mut builder = #T::default();", symbolProvider.symbolForBuilder(eventStruct))
                eventStruct.members().firstOrNull { it.hasTrait<EventPayloadTrait>() }?.let {
                    renderEventPayloadMember(it)
                }
                val headerMembers = eventStruct.members().filter { it.hasTrait<EventHeaderTrait>() }
                if (headerMembers.isNotEmpty()) {
                    renderEventHeaders(headerMembers)
                }
                val implicitMembers =
                    eventStruct.members().filter {
                        !it.hasTrait<EventPayloadTrait>() && !it.hasTrait<EventHeaderTrait>()
                    }
                if (implicitMembers.isNotEmpty() && eventStruct.members().none { it.hasTrait<EventPayloadTrait>() }) {
                    rustTemplate(
                        """
                        return #{Err}(#{Error}::unmarshalling(
                            "schema-serde server event streams do not support events mixing @eventHeader with implicit document payload members yet",
                        ));
                        """,
                        *codegenScope,
                    )
                }
                rustTemplate(
                    "#{Ok}(#{UnmarshalledMessage}::Event(#{Output}::$unionMemberName(builder.build())))",
                    "Output" to unionSymbol,
                    *codegenScope,
                )
            }
        }
    }

    private fun parsePayload(
        target: Shape,
        payloadExpr: String,
    ) = software.amazon.smithy.rust.codegen.core.rustlang.writable {
        when (target) {
            is BlobShape -> rustTemplate("#{Blob}::from_maybe_shared($payloadExpr.clone())", *codegenScope)
            is StringShape -> {
                rustTemplate(
                    """
                    ::std::str::from_utf8($payloadExpr)
                        .map_err(|_| #{Error}::unmarshalling("message payload is not valid UTF-8"))?
                        .to_owned()
                    """,
                    *codegenScope,
                )
            }
            is StructureShape, is UnionShape -> {
                rustTemplate(
                    """
                    {
                        let mut deser = <#{Marker} as #{ServerProtocol}>::codec().create_deserializer(&$payloadExpr[..]);
                        #{deserFn}(&mut deser)
                            .map_err(|err| #{Error}::unmarshalling(format!("failed to unmarshall event payload: {err}")))?
                    }
                    """,
                    "deserFn" to RuntimeType(serverSchemaDeserFnPath(codegenContext, target)),
                    *codegenScope,
                )
            }
            else -> throw IllegalStateException("unsupported event stream payload shape type: $target")
        }
    }

    private fun RustWriter.renderEventPayloadMember(member: MemberShape) {
        val target = model.expectShape(member.target)
        withBlock("builder = builder.${member.setterName()}(", ");") {
            if (symbolProvider.toSymbol(member).isOptional()) {
                rustTemplate("Some(#{payload:W})", "payload" to parsePayload(target, "message.payload()"), *codegenScope)
            } else {
                rustTemplate("#{payload:W}", "payload" to parsePayload(target, "message.payload()"), *codegenScope)
            }
        }
    }

    private fun RustWriter.renderEventHeaders(headerMembers: List<MemberShape>) {
        rustBlock("for header in message.headers()") {
            rustBlock("match header.name().as_str()") {
                for (member in headerMembers) {
                    rustBlock("${member.memberName.dq()} => ") {
                        renderEventHeader(member)
                    }
                }
                rust("_ => {}")
            }
        }
    }

    private fun RustWriter.renderEventHeader(member: MemberShape) {
        withBlock("builder = builder.${member.setterName()}(", ");") {
            if (symbolProvider.toSymbol(member).isOptional()) {
                rust("Some(")
            }
            when (val target = model.expectShape(member.target)) {
                is BooleanShape -> rustTemplate("#{expect_fns}::expect_bool(header)?", *codegenScope)
                is ByteShape -> rustTemplate("#{expect_fns}::expect_byte(header)?", *codegenScope)
                is ShortShape -> rustTemplate("#{expect_fns}::expect_int16(header)?", *codegenScope)
                is IntegerShape -> rustTemplate("#{expect_fns}::expect_int32(header)?", *codegenScope)
                is LongShape -> rustTemplate("#{expect_fns}::expect_int64(header)?", *codegenScope)
                is BlobShape -> rustTemplate("#{expect_fns}::expect_byte_array(header)?", *codegenScope)
                is EnumShape -> rustTemplate("#{expect_fns}::expect_string(header)?.as_str().into()", *codegenScope)
                is StringShape -> rustTemplate("#{expect_fns}::expect_string(header)?", *codegenScope)
                is TimestampShape -> rustTemplate("#{expect_fns}::expect_timestamp(header)?", *codegenScope)
                else -> throw IllegalStateException("unsupported event stream header shape type: $target")
            }
            if (symbolProvider.toSymbol(member).isOptional()) {
                rust(")")
            }
        }
    }

    private fun payloadReachesEnumTrait(payload: StructureShape): Boolean =
        DirectedWalker(model).walkShapes(payload).any { it.hasTrait<EnumTrait>() }
}
