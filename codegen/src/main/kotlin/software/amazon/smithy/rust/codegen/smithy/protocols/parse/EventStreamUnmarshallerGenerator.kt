/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parse

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.toPascalCase

// TODO(EventStream): [TEST] Unit test EventStreamUnmarshallerGenerator
class EventStreamUnmarshallerGenerator(
    private val protocol: Protocol,
    private val model: Model,
    private val runtimeConfig: RuntimeConfig,
    private val symbolProvider: RustSymbolProvider,
    private val operationShape: OperationShape,
    private val unionShape: UnionShape,
) {
    private val unionSymbol = symbolProvider.toSymbol(unionShape)
    private val operationErrorSymbol = operationShape.errorSymbol(symbolProvider)
    private val smithyEventStream = CargoDependency.SmithyEventStream(runtimeConfig)
    private val codegenScope = arrayOf(
        "UnmarshallMessage" to RuntimeType("UnmarshallMessage", smithyEventStream, "smithy_eventstream::frame"),
        "UnmarshalledMessage" to RuntimeType("UnmarshalledMessage", smithyEventStream, "smithy_eventstream::frame"),
        "Message" to RuntimeType("Message", smithyEventStream, "smithy_eventstream::frame"),
        "Header" to RuntimeType("Header", smithyEventStream, "smithy_eventstream::frame"),
        "HeaderValue" to RuntimeType("HeaderValue", smithyEventStream, "smithy_eventstream::frame"),
        "Error" to RuntimeType("Error", smithyEventStream, "smithy_eventstream::error"),
        "Inlineables" to RuntimeType.eventStreamInlinables(runtimeConfig),
        "SmithyError" to RuntimeType("Error", CargoDependency.SmithyTypes(runtimeConfig), "smithy_types")
    )

    fun render(): RuntimeType {
        val unmarshallerType = unionShape.eventStreamUnmarshallerType()
        return RuntimeType.forInlineFun("${unmarshallerType.name}::new", "event_stream_serde") { inlineWriter ->
            inlineWriter.renderUnmarshaller(unmarshallerType, unionSymbol)
        }
    }

    private fun RustWriter.renderUnmarshaller(unmarshallerType: RuntimeType, unionSymbol: Symbol) {
        rust(
            """
            ##[non_exhaustive]
            ##[derive(Debug)]
            pub struct ${unmarshallerType.name};

            impl ${unmarshallerType.name} {
                pub fn new() -> Self {
                    ${unmarshallerType.name}
                }
            }
            """
        )

        rustBlockTemplate(
            "impl #{UnmarshallMessage} for ${unmarshallerType.name}",
            *codegenScope
        ) {
            rust("type Output = #T;", unionSymbol)
            rust("type Error = #T;", operationErrorSymbol)

            rustBlockTemplate(
                """
                fn unmarshall(
                    &self,
                    message: #{Message}
                ) -> std::result::Result<#{UnmarshalledMessage}<Self::Output, Self::Error>, #{Error}>
                """,
                *codegenScope
            ) {
                rustBlockTemplate(
                    """
                    let response_headers = #{Inlineables}::parse_response_headers(&message)?;
                    match response_headers.message_type.as_str()
                    """,
                    *codegenScope
                ) {
                    rustBlock("\"event\" => ") {
                        renderUnmarshallEvent()
                    }
                    rustBlock("\"exception\" => ") {
                        renderUnmarshallError()
                    }
                    rustBlock("value => ") {
                        rustTemplate(
                            "return Err(#{Error}::Unmarshalling(format!(\"unrecognized :message-type: {}\", value)));",
                            *codegenScope
                        )
                    }
                }
            }
        }
    }

    private fun RustWriter.renderUnmarshallEvent() {
        rustBlock("match response_headers.smithy_type.as_str()") {
            for (member in unionShape.members()) {
                val target = model.expectShape(member.target)
                if (!target.hasTrait<ErrorTrait>()) {
                    rustBlock("${member.memberName.dq()} => ") {
                        renderUnmarshallUnionMember(member, target)
                    }
                }
            }
            rustBlock("smithy_type => ") {
                // TODO: Handle this better once unions support unknown variants
                rustTemplate(
                    "return Err(#{Error}::Unmarshalling(format!(\"unrecognized :event-type: {}\", smithy_type)));",
                    *codegenScope
                )
            }
        }
    }

    private fun RustWriter.renderUnmarshallUnionMember(member: MemberShape, target: Shape) {
        rustTemplate(
            "return Ok(#{UnmarshalledMessage}::Event(#{Output}::${member.memberName.toPascalCase()}(",
            "Output" to unionSymbol,
            *codegenScope
        )
        // TODO(EventStream): [RPC] Don't blow up on an initial-message that's not part of the union (:event-type will be "initial-request" or "initial-response")
        // TODO(EventStream): [RPC] Incorporate initial-message into original output (:event-type will be "initial-request" or "initial-response")
        when (target) {
            is BlobShape -> {
                rust("unimplemented!(\"TODO(EventStream): Implement blob unmarshalling\")")
            }
            is StringShape -> {
                rust("unimplemented!(\"TODO(EventStream): Implement string unmarshalling\")")
            }
            is UnionShape, is StructureShape -> {
                // TODO(EventStream): Check :content-type against expected content-type, error if unexpected
                val parser = protocol.structuredDataParser(operationShape).payloadParser(member)
                rustTemplate(
                    """
                    #{parser}(&message.payload()[..])
                        .map_err(|err| {
                            #{Error}::Unmarshalling(format!("failed to unmarshall ${member.memberName}: {}", err))
                        })?
                    """,
                    "parser" to parser,
                    *codegenScope
                )
            }
        }
        rust(")))")
    }

    private fun RustWriter.renderUnmarshallError() {
        rustBlock("match response_headers.smithy_type.as_str()") {
            for (member in unionShape.members()) {
                val target = model.expectShape(member.target)
                if (target.hasTrait<ErrorTrait>() && target is StructureShape) {
                    rustBlock("${member.memberName.dq()} => ") {
                        val parser = protocol.structuredDataParser(operationShape).errorParser(target)
                        if (parser != null) {
                            rust("let builder = #T::builder();", symbolProvider.toSymbol(target))
                            rustTemplate(
                                """
                                let builder = #{parser}(&message.payload()[..], builder)
                                    .map_err(|err| {
                                        #{Error}::Unmarshalling(format!("failed to unmarshall ${member.memberName}: {}", err))
                                    })?;
                                return Ok(#{UnmarshalledMessage}::Error(
                                    #{OpError}::new(
                                        #{OpError}Kind::${member.memberName.toPascalCase()}(builder.build()),
                                        #{SmithyError}::builder().build(),
                                    )
                                ))
                                """,
                                "OpError" to operationErrorSymbol,
                                "parser" to parser,
                                *codegenScope
                            )
                        }
                    }
                }
            }
            rust("_ => {}")
        }
        // TODO(EventStream): Generic error parsing; will need to refactor `parseGenericError` to
        // operate on bodies rather than responses. This should be easy for all but restJson,
        // which pulls the error type out of a header.
        rust("unimplemented!(\"event stream generic error parsing\")")
    }

    private fun UnionShape.eventStreamUnmarshallerType(): RuntimeType {
        val symbol = symbolProvider.toSymbol(this)
        return RuntimeType("${symbol.name.toPascalCase()}Unmarshaller", null, "crate::event_stream_serde")
    }
}
