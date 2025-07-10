/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.http

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.SimpleShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.MediaTypeTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asOptional
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.generators.OperationBuildError
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingDescriptor
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.EventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.SerializerUtil
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.ValueExpression
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isPrimitive
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.redactIfNecessary

/**
 * The type of HTTP message from which we are (de)serializing the HTTP-bound data.
 * We are either:
 *     - deserializing data from an HTTP request (we are a server),
 *     - deserializing data from an HTTP response (we are a client),
 *     - serializing data to an HTTP request (we are a client),
 *     - serializing data to an HTTP response (we are a server),
 */
enum class HttpMessageType {
    REQUEST,
    RESPONSE,
}

/**
 * Class describing an HTTP binding (de)serialization section that can be used in a customization.
 */
sealed class HttpBindingSection(name: String) : Section(name) {
    data class BeforeIteratingOverMapShapeBoundWithHttpPrefixHeaders(val variableName: String, val shape: MapShape) :
        HttpBindingSection("BeforeIteratingOverMapShapeBoundWithHttpPrefixHeaders")

    data class BeforeRenderingHeaderValue(var context: HttpBindingGenerator.HeaderValueSerializationContext) :
        HttpBindingSection("BeforeRenderingHeaderValue")

    data class AfterDeserializingIntoAHashMapOfHttpPrefixHeaders(val memberShape: MemberShape) :
        HttpBindingSection("AfterDeserializingIntoAHashMapOfHttpPrefixHeaders")

    data class AfterDeserializingIntoADateTimeOfHttpHeaders(val memberShape: MemberShape) :
        HttpBindingSection("AfterDeserializingIntoADateTimeOfHttpHeaders")
}

typealias HttpBindingCustomization = NamedCustomization<HttpBindingSection>

/**
 * This class generates Rust functions that (de)serialize data from/to an HTTP message.
 * They are useful for *both*:
 *     - servers (that deserialize HTTP requests and serialize HTTP responses); and
 *     - clients (that deserialize HTTP responses and serialize HTTP requests)
 * because the (de)serialization logic is entirely similar. In the cases where they _slightly_ differ,
 * the [HttpMessageType] enum is used to distinguish.
 *
 * For deserialization logic that is wholly different among clients and servers, use the classes:
 *     - [ServerRequestBindingGenerator] from the `codegen-server` project for servers; and
 *     - [ResponseBindingGenerator] from this project for clients
 * instead.
 *
 * For serialization logic that is wholly different among clients and servers, use the classes:
 *     - [ServerResponseBindingGenerator] from the `codegen-server` project for servers; and
 *     - [RequestBindingGenerator] from this project for clients
 * instead.
 */
class HttpBindingGenerator(
    private val protocol: Protocol,
    private val codegenContext: CodegenContext,
    private val symbolProvider: SymbolProvider,
    private val operationShape: OperationShape,
    private val customizations: List<HttpBindingCustomization> = listOf(),
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenTarget = codegenContext.target
    private val model = codegenContext.model
    private val index = HttpBindingIndex.of(model)
    private val headerUtil = RuntimeType.smithyHttp(runtimeConfig).resolve("header")
    private val defaultTimestampFormat = TimestampFormatTrait.Format.EPOCH_SECONDS
    private val protocolFunctions = ProtocolFunctions(codegenContext)
    private val serializerUtil = SerializerUtil(model, symbolProvider)

    /**
     * Generate a function to deserialize [binding] from HTTP headers.
     *
     * The name of the resulting function is returned as a String.
     *
     * Generates a function like:
     * ```rust
     * fn parse_foo(headers: &http::HeaderMap) -> Result<Option<String>, ParseError> {
     *   ...
     * }
     * ```
     */
    fun generateDeserializeHeaderFn(binding: HttpBindingDescriptor): RuntimeType {
        check(binding.location == HttpLocation.HEADER)
        val outputT = symbolProvider.toSymbol(binding.member).makeOptional()
        return protocolFunctions.deserializeFn(binding.member, fnNameSuffix = "header") { fnName ->
            rustBlockTemplate(
                "pub(crate) fn $fnName(header_map: &#{Headers}) -> #{Result}<#{Output}, #{header_util}::ParseError>",
                *preludeScope,
                "Headers" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("http::Headers"),
                "Output" to outputT,
                "header_util" to headerUtil,
            ) {
                rust("let headers = header_map.get_all(${binding.locationName.dq()});")
                deserializeFromHeader(model.expectShape(binding.member.target), binding.member)
            }
        }
    }

    fun generateDeserializePrefixHeaderFn(binding: HttpBindingDescriptor): RuntimeType {
        check(binding.location == HttpBinding.Location.PREFIX_HEADERS)
        val outputSymbol = symbolProvider.toSymbol(binding.member)
        val target = model.expectShape(binding.member.target)
        check(target is MapShape)
        val inner =
            protocolFunctions.deserializeFn(binding.member, fnNameSuffix = "inner") { fnName ->
                rustBlockTemplate(
                    "pub fn $fnName<'a>(headers: impl #{Iterator}<Item = &'a str>) -> std::result::Result<Option<#{Value}>, #{header_util}::ParseError>",
                    *preludeScope,
                    "Value" to symbolProvider.toSymbol(model.expectShape(target.value.target)),
                    "header_util" to headerUtil,
                ) {
                    deserializeFromHeader(model.expectShape(target.value.target), binding.member)
                }
            }
        val returnTypeSymbol = outputSymbol.mapRustType { it.asOptional() }
        return protocolFunctions.deserializeFn(binding.member, fnNameSuffix = "prefix_header") { fnName ->
            rustBlockTemplate(
                "pub(crate) fn $fnName(header_map: &#{Headers}) -> std::result::Result<#{Value}, #{header_util}::ParseError>",
                "Headers" to RuntimeType.headers(runtimeConfig),
                "Value" to returnTypeSymbol,
                "header_util" to headerUtil,
            ) {
                rust(
                    """
                    let headers = #T::headers_for_prefix(
                        header_map.iter().map(|(k, _)| k),
                        ${binding.locationName.dq()}
                    );
                    let out: std::result::Result<_, _> = headers.map(|(key, header_name)| {
                        let values = header_map.get_all(header_name);
                        #T(values).map(|v| (key.to_string(), v.expect(
                            "we have checked there is at least one value for this header name; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues"
                        )))
                    }).collect();
                    """,
                    headerUtil, inner,
                )

                for (customization in customizations) {
                    customization.section(
                        HttpBindingSection.AfterDeserializingIntoAHashMapOfHttpPrefixHeaders(binding.member),
                    )(this)
                }
                rust("out.map(Some)")
            }
        }
    }

    /**
     * Generate a function to deserialize `[binding]` from the request / response payload.
     */
    fun generateDeserializePayloadFn(
        binding: HttpBindingDescriptor,
        errorSymbol: Symbol,
        // Deserialize a single structure, union or document member marked as a payload
        payloadParser: RustWriter.(String) -> Unit,
        httpMessageType: HttpMessageType = HttpMessageType.RESPONSE,
    ): RuntimeType {
        check(binding.location == HttpBinding.Location.PAYLOAD)
        return protocolFunctions.deserializeFn(binding.member, fnNameSuffix = "payload") { fnName ->
            if (binding.member.isStreaming(model)) {
                val outputT = symbolProvider.toSymbol(binding.member)
                rustBlock(
                    "pub fn $fnName(body: &mut #T) -> std::result::Result<#T, #T>",
                    RuntimeType.sdkBody(runtimeConfig),
                    outputT,
                    errorSymbol,
                ) {
                    // Streaming unions are Event Streams and should be handled separately
                    val target = model.expectShape(binding.member.target)
                    if (target is UnionShape) {
                        bindEventStreamOutput(operationShape, outputT, target)
                    } else {
                        deserializeStreamingBody(binding)
                    }
                }
            } else {
                // The output needs to be Optional when deserializing the payload body or the caller signature
                // will not match.
                val outputT = symbolProvider.toSymbol(binding.member).makeOptional()
                rustBlock("pub(crate) fn $fnName(body: &[u8]) -> std::result::Result<#T, #T>", outputT, errorSymbol) {
                    deserializePayloadBody(
                        binding,
                        errorSymbol,
                        structuredHandler = payloadParser,
                        httpMessageType,
                    )
                }
            }
        }
    }

    private fun RustWriter.bindEventStreamOutput(
        operationShape: OperationShape,
        outputT: Symbol,
        targetShape: UnionShape,
    ) {
        val unmarshallerConstructorFn =
            EventStreamUnmarshallerGenerator(
                protocol,
                codegenContext,
                operationShape,
                targetShape,
            ).render()
        rustTemplate(
            """
            let unmarshaller = #{unmarshallerConstructorFn}();
            let body = std::mem::replace(body, #{SdkBody}::taken());
            Ok(#{receiver:W})
            """,
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "unmarshallerConstructorFn" to unmarshallerConstructorFn,
            "receiver" to
                writable {
                    if (codegenTarget == CodegenTarget.SERVER) {
                        rust("${outputT.rustType().qualifiedName()}::new(unmarshaller, body)")
                    } else {
                        rustTemplate(
                            "#{EventReceiver}::new(#{Receiver}::new(unmarshaller, body))",
                            "EventReceiver" to RuntimeType.eventReceiver(runtimeConfig),
                            "Receiver" to RuntimeType.eventStreamReceiver(runtimeConfig),
                        )
                    }
                },
        )
    }

    private fun RustWriter.deserializeStreamingBody(binding: HttpBindingDescriptor) {
        val member = binding.member
        val targetShape = model.expectShape(member.target)
        check(targetShape is BlobShape)
        rustTemplate(
            """
            // replace the body with an empty body
            let body = std::mem::replace(body, #{SdkBody}::taken());
            Ok(#{ByteStream}::new(body))
            """,
            "ByteStream" to symbolProvider.toSymbol(member), "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        )
    }

    private fun RustWriter.deserializePayloadBody(
        binding: HttpBindingDescriptor,
        errorSymbol: Symbol,
        structuredHandler: RustWriter.(String) -> Unit,
        httpMessageType: HttpMessageType = HttpMessageType.RESPONSE,
    ) {
        val member = binding.member
        val targetShape = model.expectShape(member.target)
        // There is an unfortunate bit of dual behavior caused by an empty body causing the output to be `None` instead
        // of an empty instance of the response type.
        withBlock("(!body.is_empty()).then(||{", "}).transpose()") {
            when (targetShape) {
                is StructureShape, is UnionShape, is DocumentShape -> this.structuredHandler("body")
                is StringShape -> {
                    when (httpMessageType) {
                        HttpMessageType.RESPONSE -> {
                            rustTemplate(
                                "let body_str = std::str::from_utf8(body).map_err(#{error_symbol}::unhandled)?;",
                                "error_symbol" to errorSymbol,
                            )
                        }

                        HttpMessageType.REQUEST -> {
                            rust("let body_str = std::str::from_utf8(body)?;")
                        }
                    }
                    if (targetShape.hasTrait<EnumTrait>()) {
                        // - In servers, `T` is an unconstrained `String` that will be constrained when building the
                        //   builder.
                        // - In clients, `T` will directly be the target generated enum type.
                        rust(
                            "Ok(#T::from(body_str))",
                            symbolProvider.toSymbol(targetShape),
                        )
                    } else {
                        rust("Ok(body_str.to_string())")
                    }
                }

                is BlobShape ->
                    rust(
                        "Ok(#T::new(body))",
                        symbolProvider.toSymbol(targetShape),
                    )
                // `httpPayload` can be applied to set/map/list shapes.
                // However, none of the AWS protocols support it.
                // Smithy CLI will refuse to build the model if you apply the trait to these shapes, so this branch
                // remains unreachable.
                else -> UNREACHABLE("unexpected shape: $targetShape")
            }
        }
    }

    /**
     * Parse a value from a header.
     * This function produces an expression which produces the precise type required by the target shape.
     */
    private fun RustWriter.deserializeFromHeader(
        targetShape: Shape,
        memberShape: MemberShape,
    ) {
        val rustType = symbolProvider.toSymbol(targetShape).rustType().stripOuter<RustType.Option>()
        // Normally, we go through a flow that looks for `,`s but that's wrong if the output
        // is just a single string (which might include `,`s.).
        // MediaType doesn't include `,` since it's base64, send that through the normal path
        if (targetShape is StringShape && !targetShape.hasTrait<MediaTypeTrait>()) {
            rust("#T::one_or_none(headers)", headerUtil)
            return
        }
        val (coreType, coreShape) =
            if (targetShape is CollectionShape) {
                val coreShape = model.expectShape(targetShape.member.target)
                symbolProvider.toSymbol(coreShape).rustType() to coreShape
            } else {
                rustType to targetShape
            }
        val parsedValue = safeName()
        if (coreShape.isTimestampShape()) {
            val timestampFormat =
                index.determineTimestampFormat(
                    memberShape,
                    HttpBinding.Location.HEADER,
                    defaultTimestampFormat,
                )
            val timestampFormatType = RuntimeType.parseTimestampFormat(codegenTarget, runtimeConfig, timestampFormat)
            rust(
                "let $parsedValue: Vec<${coreType.render()}> = #T::many_dates(headers, #T)?",
                headerUtil,
                timestampFormatType,
            )
            for (customization in customizations) {
                customization.section(HttpBindingSection.AfterDeserializingIntoADateTimeOfHttpHeaders(memberShape))(this)
            }
            rust(";")
        } else if (coreShape.isPrimitive()) {
            rust(
                "let $parsedValue = #T::read_many_primitive::<${coreType.render()}>(headers)?;",
                headerUtil,
            )
        } else {
            rust(
                "let $parsedValue: Vec<${coreType.render()}> = #T::read_many_from_str(headers)?;",
                headerUtil,
            )
            if (coreShape.hasTrait<MediaTypeTrait>()) {
                rustTemplate(
                    """
                    let $parsedValue: std::result::Result<Vec<_>, _> = $parsedValue
                        .iter().map(|s|
                            #{base_64_decode}(s).map_err(|_|#{header}::ParseError::new("failed to decode base64"))
                            .and_then(|bytes|String::from_utf8(bytes).map_err(|_|#{header}::ParseError::new("base64 encoded data was not valid utf-8")))
                        ).collect();
                    """,
                    "base_64_decode" to RuntimeType.base64Decode(runtimeConfig),
                    "header" to headerUtil,
                )
                rust("let $parsedValue = $parsedValue?;")
            }
        }
        when (rustType) {
            is RustType.Vec ->
                rust(
                    """
                    Ok(if !$parsedValue.is_empty() {
                        Some($parsedValue)
                    } else {
                        None
                    })
                    """,
                )

            is RustType.HashSet ->
                rust(
                    """
                    Ok(if !$parsedValue.is_empty() {
                        Some($parsedValue.into_iter().collect())
                    } else {
                        None
                    })
                    """,
                )

            else -> {
                if (targetShape is ListShape) {
                    // This is a constrained list shape and we must therefore be generating a server SDK.
                    check(codegenTarget == CodegenTarget.SERVER)
                    check(rustType is RustType.Opaque)
                    rust(
                        """
                        Ok(if !$parsedValue.is_empty() {
                            Some(#T($parsedValue))
                        } else {
                            None
                        })
                        """,
                        symbolProvider.toSymbol(targetShape),
                    )
                } else {
                    check(targetShape is SimpleShape)
                    rustTemplate(
                        """
                        if $parsedValue.len() > 1 {
                            Err(#{header_util}::ParseError::new(format!("expected one item but found {}", $parsedValue.len())))
                        } else {
                            let mut $parsedValue = $parsedValue;
                            Ok($parsedValue.pop())
                        }
                        """,
                        "header_util" to headerUtil,
                    )
                }
            }
        }
    }

    /**
     * Returns a function to set headers on an HTTP message for the given [shape].
     * Returns null if no headers need to be set.
     *
     * [shape] can either be:
     *     - an [OperationShape], in which case the header-bound data is in its input or output shape; or
     *     - an error shape (i.e. a [StructureShape] with the `error` trait), in which case the header-bound data is in the shape itself.
     */
    fun generateAddHeadersFn(
        shape: Shape,
        httpMessageType: HttpMessageType = HttpMessageType.REQUEST,
        serializeEmptyHeaders: Boolean = false,
    ): RuntimeType? {
        val (headerBindings, prefixHeaderBinding) =
            when (httpMessageType) {
                // Only a single structure member can be bound by `httpPrefixHeaders`, hence the `getOrNull(0)`.
                HttpMessageType.REQUEST ->
                    index.getRequestBindings(shape, HttpLocation.HEADER) to
                        index.getRequestBindings(shape, HttpLocation.PREFIX_HEADERS).getOrNull(0)

                HttpMessageType.RESPONSE ->
                    index.getResponseBindings(shape, HttpLocation.HEADER) to
                        index.getResponseBindings(shape, HttpLocation.PREFIX_HEADERS).getOrNull(0)
            }

        if (headerBindings.isEmpty() && prefixHeaderBinding == null) {
            return null
        }

        // Skip if we need to serialize operation input's members in an initial message of event stream
        // See https://smithy.io/2.0/spec/streaming.html#initial-request
        if (shape is OperationShape &&
            protocol.httpBindingResolver.handlesEventStreamInitialRequest(shape)
        ) {
            return null
        }

        return protocolFunctions.serializeFn(shape, fnNameSuffix = "headers") { fnName ->
            // If the shape is an operation shape, the input symbol of the generated function is the input or output
            // shape, which is the shape holding the header-bound data.
            val shapeSymbol =
                symbolProvider.toSymbol(
                    if (shape is OperationShape) {
                        when (httpMessageType) {
                            HttpMessageType.REQUEST -> shape.inputShape(model)
                            HttpMessageType.RESPONSE -> shape.outputShape(model)
                        }
                    } else {
                        shape
                    },
                )
            val codegenScope =
                arrayOf(
                    "BuildError" to runtimeConfig.operationBuildError(),
                    HttpMessageType.REQUEST.name to RuntimeType.HttpRequestBuilder1x,
                    HttpMessageType.RESPONSE.name to RuntimeType.HttpResponseBuilder1x,
                    "Shape" to shapeSymbol,
                )
            rustBlockTemplate(
                """
                pub fn $fnName(
                    input: &#{Shape},
                    mut builder: #{${httpMessageType.name}}
                ) -> std::result::Result<#{${httpMessageType.name}}, #{BuildError}>
                """,
                *codegenScope,
            ) {
                headerBindings.forEach { httpBinding -> renderHeaders(httpBinding, serializeEmptyHeaders) }
                if (prefixHeaderBinding != null) {
                    renderPrefixHeader(prefixHeaderBinding)
                }
                rust("Ok(builder)")
            }
        }
    }

    private fun RustWriter.renderHeaders(
        httpBinding: HttpBinding,
        serializeEmptyHeaders: Boolean,
    ) {
        check(httpBinding.location == HttpLocation.HEADER)
        val memberShape = httpBinding.member
        val targetShape = model.expectShape(memberShape.target)
        val memberName = symbolProvider.toMemberName(memberShape)
        val headerName = httpBinding.locationName
        val timestampFormat =
            index.determineTimestampFormat(memberShape, HttpBinding.Location.HEADER, defaultTimestampFormat)
        val renderErrorMessage = { headerValueVariableName: String ->
            OperationBuildError(runtimeConfig).invalidField(memberName) {
                rust(
                    """
                    format!(
                        "`{}` cannot be used as a header value: {}",
                        &${memberShape.redactIfNecessary(model, headerValueVariableName)},
                        err
                    )
                    """,
                )
            }
        }

        val memberSymbol = symbolProvider.toSymbol(memberShape)
        // If a header is of a primitive type and required (e.g. `bool`), we do not serialize it on the
        // wire if it's set to the default value for that primitive type (e.g. `false` for `bool`).
        // If the header is optional, instead, we want to serialize it if it has been set by the user to the
        // default value for that primitive type (e.g. `Some(false)` for an `Option<bool>` header).
        // If a header is multivalued, we always want to serialize its primitive members, regardless of their
        // values.
        ifSome(memberSymbol, ValueExpression.Reference("&input.$memberName")) { variableName ->
            if (targetShape is CollectionShape) {
                renderMultiValuedHeader(
                    model,
                    headerName,
                    variableName,
                    targetShape,
                    timestampFormat,
                    renderErrorMessage,
                    serializeEmptyHeaders,
                )
            } else {
                renderHeaderValue(
                    headerName,
                    variableName,
                    targetShape,
                    false,
                    timestampFormat,
                    renderErrorMessage,
                    serializeIfDefault = memberSymbol.isOptional(),
                    memberShape,
                    serializeEmptyHeaders,
                )
            }
        }
    }

    private fun RustWriter.renderMultiValuedHeader(
        model: Model,
        headerName: String,
        value: ValueExpression,
        shape: CollectionShape,
        timestampFormat: TimestampFormatTrait.Format,
        renderErrorMessage: (String) -> Writable,
        serializeEmptyHeaders: Boolean,
    ) {
        val loopVariable = ValueExpression.Reference(safeName("inner"))
        val context = HeaderValueSerializationContext(value, shape)
        for (customization in customizations) {
            customization.section(
                HttpBindingSection.BeforeRenderingHeaderValue(context),
            )(this)
        }

        // Conditionally wrap the header generation in a block that handles empty header values if
        // `serializeEmptyHeaders` is true
        conditionalBlock(
            """
            // Empty vec in header is serialized as an empty string
            if ${context.valueExpression.name}.is_empty() {
                builder = builder.header("$headerName", "");
            } else {""",
            "}", conditional = serializeEmptyHeaders,
        ) {
            rustBlock("for ${loopVariable.name} in ${context.valueExpression.asRef()}") {
                this.renderHeaderValue(
                    headerName,
                    loopVariable,
                    model.expectShape(shape.member.target),
                    isMultiValuedHeader = true,
                    timestampFormat,
                    renderErrorMessage,
                    serializeIfDefault = true,
                    shape.member,
                    serializeEmptyHeaders,
                )
            }
        }
    }

    data class HeaderValueSerializationContext(
        /** Expression representing the value to write to the JsonValueWriter */
        var valueExpression: ValueExpression,
        /** Path in the JSON to get here, used for errors */
        val shape: Shape,
    )

    private fun RustWriter.renderHeaderValue(
        headerName: String,
        value: ValueExpression,
        shape: Shape,
        isMultiValuedHeader: Boolean,
        timestampFormat: TimestampFormatTrait.Format,
        renderErrorMessage: (String) -> Writable,
        serializeIfDefault: Boolean,
        memberShape: MemberShape,
        serializeEmptyHeaders: Boolean,
    ) {
        val context = HeaderValueSerializationContext(value, shape)
        for (customization in customizations) {
            customization.section(
                HttpBindingSection.BeforeRenderingHeaderValue(context),
            )(this)
        }

        val block: RustWriter.(value: ValueExpression) -> Unit = { variableName ->
            if (shape.isPrimitive()) {
                val encoder = RuntimeType.smithyTypes(runtimeConfig).resolve("primitive::Encoder")
                rust("let mut encoder = #T::from(${variableName.asValue()});", encoder)
            }
            val formatted =
                headerFmtFun(
                    this,
                    shape,
                    timestampFormat,
                    context.valueExpression.name,
                    isMultiValuedHeader = isMultiValuedHeader,
                )
            val safeName = safeName("formatted")

            // If `serializeEmptyHeaders` is false we wrap header serialization in a `!foo.is_empty()` check and skip
            // serialization if the header value is empty
            rust("let $safeName = $formatted;")
            conditionalBlock("if !$safeName.is_empty() {", "}", conditional = !serializeEmptyHeaders) {
                rustTemplate(
                    """
                    let header_value = $safeName;
                    let header_value: #{HeaderValue} = header_value.parse().map_err(|err| {
                        #{invalid_field_error:W}
                    })?;
                    builder = builder.header("$headerName", header_value);

                    """,
                    "HeaderValue" to RuntimeType.Http1x.resolve("HeaderValue"),
                    "invalid_field_error" to renderErrorMessage("header_value"),
                )
            }
        }
        if (serializeIfDefault) {
            block(context.valueExpression)
        } else {
            with(serializerUtil) {
                ignoreDefaultsForNumbersAndBools(memberShape, context.valueExpression) {
                    block(context.valueExpression)
                }
            }
        }
    }

    private fun RustWriter.renderPrefixHeader(httpBinding: HttpBinding) {
        check(httpBinding.location == HttpLocation.PREFIX_HEADERS)
        val memberShape = httpBinding.member
        val targetShape = model.expectShape(memberShape.target, MapShape::class.java)
        val memberSymbol = symbolProvider.toSymbol(memberShape)
        val memberName = symbolProvider.toMemberName(memberShape)
        val valueTargetShape = model.expectShape(targetShape.value.target)
        val timestampFormat =
            index.determineTimestampFormat(memberShape, HttpBinding.Location.HEADER, defaultTimestampFormat)

        ifSet(targetShape, memberSymbol, ValueExpression.Reference("&input.$memberName")) { local ->
            for (customization in customizations) {
                customization.section(
                    HttpBindingSection.BeforeIteratingOverMapShapeBoundWithHttpPrefixHeaders(local.name, targetShape),
                )(this)
            }
            rustTemplate(
                """
                for (k, v) in ${local.asRef()} {
                    use std::str::FromStr;
                    let header_name = #{HeaderName}::from_str(&format!("{}{}", "${httpBinding.locationName}", &k)).map_err(|err| {
                        #{invalid_header_name:W}
                    })?;
                    let header_value = ${
                    headerFmtFun(
                        this,
                        valueTargetShape,
                        timestampFormat,
                        "v",
                        isMultiValuedHeader = false,
                    )
                };
                    let header_value: #{HeaderValue} = header_value.parse().map_err(|err| {
                        #{invalid_header_value:W}
                    })?;
                    builder = builder.header(header_name, header_value);
                }

                """,
                "HeaderValue" to RuntimeType.Http1x.resolve("HeaderValue"),
                "HeaderName" to RuntimeType.Http1x.resolve("HeaderName"),
                "invalid_header_name" to
                    OperationBuildError(runtimeConfig).invalidField(memberName) {
                        rust("""format!("`{k}` cannot be used as a header name: {err}")""")
                    },
                "invalid_header_value" to
                    OperationBuildError(runtimeConfig).invalidField(memberName) {
                        rust(
                            """
                            format!(
                                "`{}` cannot be used as a header value: {}",
                                ${memberShape.redactIfNecessary(model, "v")},
                                err
                            )
                            """,
                        )
                    },
            )
        }
    }

    /**
     * Format [member] when used as an HTTP header.
     */
    private fun headerFmtFun(
        writer: RustWriter,
        target: Shape,
        timestampFormat: TimestampFormatTrait.Format,
        targetName: String,
        isMultiValuedHeader: Boolean,
    ): String {
        fun quoteValue(value: String): String {
            // Timestamp shapes are not quoted in header lists
            return if (isMultiValuedHeader && !target.isTimestampShape) {
                val quoteFn = writer.format(headerUtil.resolve("quote_header_value"))
                "$quoteFn($value)"
            } else {
                value
            }
        }
        return when {
            target.isStringShape -> {
                if (target.hasTrait<MediaTypeTrait>()) {
                    val func = writer.format(RuntimeType.base64Encode(runtimeConfig))
                    "$func($targetName)"
                } else {
                    quoteValue("$targetName.as_str()")
                }
            }

            target.isTimestampShape -> {
                val timestampFormatType = RuntimeType.serializeTimestampFormat(runtimeConfig, timestampFormat)
                quoteValue("$targetName.fmt(${writer.format(timestampFormatType)})?")
            }

            target.isListShape || target.isMemberShape -> {
                throw IllegalArgumentException("lists should be handled at a higher level")
            }

            target.isPrimitive() -> {
                "encoder.encode()"
            }

            else -> throw CodegenException("unexpected shape: $target")
        }
    }
}
