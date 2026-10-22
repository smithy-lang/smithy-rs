/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.http

import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerEventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.targetCanReachConstrainedShape

class ServerRequestBindingGenerator(
    val protocol: ServerProtocol,
    private val codegenContext: ServerCodegenContext,
    private val operationShape: OperationShape,
    private val additionalHttpBindingCustomizations: List<HttpBindingCustomization> = listOf(),
) {
    private val httpBindingGenerator =
        HttpBindingGenerator(
            protocol,
            codegenContext,
            // Note how we parse the HTTP-bound values into _unconstrained_ types; they will be constrained when
            // building the builder.
            codegenContext.unconstrainedShapeSymbolProvider,
            operationShape,
            listOf(
                ServerRequestAfterDeserializingIntoAHashMapOfHttpPrefixHeadersWrapInUnconstrainedMapHttpBindingCustomization(
                    codegenContext,
                ),
            ) + additionalHttpBindingCustomizations,
        )

    fun generateDeserializeHeaderFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializeHeaderFn(binding)

    fun generateDeserializePayloadFn(
        binding: HttpBindingDescriptor,
        structuredHandler: RustWriter.(String) -> Unit,
    ): RuntimeType {
        val target = codegenContext.model.expectShape(binding.member.target)
        if (binding.member.isStreaming(codegenContext.model) && target is UnionShape) {
            return generateDeserializeEventStreamFn(binding, target)
        }
        return httpBindingGenerator.generateDeserializePayloadFn(
            binding,
            protocol.deserializePayloadErrorType(binding).toSymbol(),
            structuredHandler,
            HttpMessageType.REQUEST,
        )
    }

    private fun generateDeserializeEventStreamFn(
        binding: HttpBindingDescriptor,
        target: UnionShape,
    ): RuntimeType {
        check(binding.location == HttpBinding.Location.PAYLOAD)
        val output = codegenContext.unconstrainedShapeSymbolProvider.toSymbol(binding.member)
        val constructor = ServerEventStreamUnmarshallerGenerator(protocol, codegenContext, target).render()
        return ProtocolFunctions(codegenContext).deserializeFn(binding.member, fnNameSuffix = "payload") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(body: &mut #{SdkBody}) -> #{Result}<#{Output}, #{Error}>",
                *preludeScope,
                "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                "Output" to output,
                "Error" to protocol.deserializePayloadErrorType(binding),
            ) {
                rustTemplate("let unmarshaller = #{Constructor}();", "Constructor" to constructor)
                for (customization in additionalHttpBindingCustomizations) {
                    customization.section(
                        HttpBindingSection.BeforeCreatingEventStreamReceiver(operationShape, target, "unmarshaller"),
                    )(this)
                }
                rustTemplate(
                    """
                    let body = #{Replace}(body, #{SdkBody}::taken());
                    let receiver = #{Receiver};
                    #{Ok}(receiver)
                    """,
                    *preludeScope,
                    "Replace" to RuntimeType.std.resolve("mem::replace"),
                    "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                    "Receiver" to writable { rust("${output.rustType().qualifiedName()}::new(unmarshaller, body)") },
                )
            }
        }
    }

    fun generateDeserializePrefixHeadersFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializePrefixHeaderFn(binding)
}

/**
 * A customization to, just after we've deserialized HTTP request headers bound to a map shape via `@httpPrefixHeaders`,
 * wrap the `std::collections::HashMap` in an unconstrained type wrapper newtype.
 */
class ServerRequestAfterDeserializingIntoAHashMapOfHttpPrefixHeadersWrapInUnconstrainedMapHttpBindingCustomization(val codegenContext: ServerCodegenContext) :
    HttpBindingCustomization() {
    override fun section(section: HttpBindingSection): Writable =
        when (section) {
            is HttpBindingSection.BeforeRenderingHeaderValue,
            is HttpBindingSection.BeforeIteratingOverMapShapeBoundWithHttpPrefixHeaders,
            -> emptySection
            is HttpBindingSection.AfterDeserializingIntoAHashMapOfHttpPrefixHeaders ->
                writable {
                    if (section.memberShape.targetCanReachConstrainedShape(codegenContext.model, codegenContext.unconstrainedShapeSymbolProvider)) {
                        rust(
                            "let out = out.map(#T);",
                            codegenContext.unconstrainedShapeSymbolProvider.toSymbol(section.memberShape).mapRustType {
                                it.stripOuter<RustType.Option>()
                            },
                        )
                    }
                }
            else -> emptySection
        }
}
