/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator

/** TODO(rpcv2): Implement this (currently a copy of [software.amazon.smithy.rust.codegen.core.smithy.protocols.RestJson])
 */
open class RpcV2(val codegenContext: CodegenContext) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
        "HeaderMap" to RuntimeType.Http.resolve("HeaderMap"),
        "JsonError" to CargoDependency.smithyJson(runtimeConfig).toType()
            .resolve("deserialize::error::DeserializeError"),
        "Response" to RuntimeType.Http.resolve("Response"),
        "json_errors" to RuntimeType.jsonErrors(runtimeConfig),
    )
    private val jsonDeserModule = RustModule.private("json_deser")

    override val httpBindingResolver: HttpBindingResolver
        get() = RestJsonHttpBindingResolver(codegenContext.model, ProtocolContentTypes("application/json", "application/json", "application/vnd.amazon.eventstream"))

    override val defaultTimestampFormat: TimestampFormatTrait.Format =
        TimestampFormatTrait.Format.DATE_TIME

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator =
        // TODO(rpcv2): Implement `RpcV2.structuredDataParser`
        JsonParserGenerator(codegenContext, httpBindingResolver, ::restJsonFieldName)

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator =
        // TODO(rpcv2): Implement `RpcV2.structuredDataSerializer`
        JsonSerializerGenerator(codegenContext, httpBindingResolver, ::restJsonFieldName)

    // TODO(rpcv2): Implement `RpcV2.parseHttpErrorMetadata`
    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_error_metadata", jsonDeserModule) {
            rustTemplate(
                """
                pub fn parse_http_error_metadata(response: &#{Response}<#{Bytes}>) -> Result<#{ErrorMetadataBuilder}, #{JsonError}> {
                    #{json_errors}::parse_error_metadata(response.body(), response.headers())
                }
                """,
                *errorScope,
            )
        }

    // TODO(rpcv2): Implement `RpcV2.parseEventStreamErrorMetadata`
    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_event_stream_error_metadata", jsonDeserModule) {
            rustTemplate(
                """
                pub fn parse_event_stream_error_metadata(payload: &#{Bytes}) -> Result<#{ErrorMetadataBuilder}, #{JsonError}> {
                    // Note: HeaderMap::new() doesn't allocate
                    #{json_errors}::parse_error_metadata(payload, &#{HeaderMap}::new())
                }
                """,
                *errorScope,
            )
        }
}
