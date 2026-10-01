/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.http

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpMessageType
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.targetCanReachConstrainedShape

class ServerRequestBindingGenerator(
    val protocol: ServerProtocol,
    codegenContext: ServerCodegenContext,
    operationShape: OperationShape,
    additionalHttpBindingCustomizations: List<HttpBindingCustomization> = listOf(),
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
                ServerEventStreamMessageTimeoutHttpBindingCustomization(codegenContext),
            ) + additionalHttpBindingCustomizations,
        )

    fun generateDeserializeHeaderFn(binding: HttpBindingDescriptor): RuntimeType =
        httpBindingGenerator.generateDeserializeHeaderFn(binding)

    fun generateDeserializePayloadFn(
        binding: HttpBindingDescriptor,
        structuredHandler: RustWriter.(String) -> Unit,
    ): RuntimeType =
        httpBindingGenerator.generateDeserializePayloadFn(
            binding,
            protocol.deserializePayloadErrorType(binding).toSymbol(),
            structuredHandler,
            HttpMessageType.REQUEST,
        )

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

/**
 * A customization to apply a per-message completion deadline to the event stream request body
 * handed to the operation handler, configured via `customizationConfig.requestBodyReadTimeouts`.
 * Once the first bytes of a message arrive, the complete message frame must arrive before the
 * deadline expires, mitigating slow-drip request attacks on event stream operations.
 *
 * The deadline is applied by wrapping the request body with the inlineable
 * `event_stream_message_timeout` module, keeping the runtime change local to the generated crate.
 */
class ServerEventStreamMessageTimeoutHttpBindingCustomization(val codegenContext: ServerCodegenContext) :
    HttpBindingCustomization() {
    override fun section(section: HttpBindingSection): Writable =
        when (section) {
            is HttpBindingSection.WrapEventStreamRequestBody ->
                writable {
                    val timeoutMillis =
                        codegenContext.settings.requestBodyReadTimeouts
                            .eventStreamMessageTimeoutMillisFor(section.operationShape.id)
                    if (timeoutMillis != null) {
                        rustTemplate(
                            """
                            let ${section.bodyVariableName} = #{wrap_with_message_timeout}(
                                ${section.bodyVariableName},
                                #{Duration}::from_millis(${timeoutMillis}u64),
                            );
                            """,
                            "Duration" to RuntimeType.std.resolve("time::Duration"),
                            "wrap_with_message_timeout" to
                                eventStreamMessageTimeoutModule(codegenContext.runtimeConfig)
                                    .resolve("wrap_with_message_timeout"),
                        )
                    }
                }
            else -> emptySection
        }

    private fun eventStreamMessageTimeoutModule(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.forInlineDependency(
            InlineDependency.forRustFile(
                RustModule.private("event_stream_message_timeout"),
                "/inlineable/src/event_stream_message_timeout.rs",
                CargoDependency.smithyTypes(runtimeConfig).withFeature("http-body-1-x"),
                CargoDependency.Bytes,
                CargoDependency.HttpBody1x,
                CargoDependency.HttpBodyUtil01x.toDevDependency(),
                CargoDependency.Tokio.toDevDependency(),
                ServerCargoDependency.PinProjectLite,
                ServerCargoDependency.TokioTime,
                CargoDependency.Tracing,
            ),
        )
}
