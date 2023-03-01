/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator

open class RpcV2(val codegenContext: CodegenContext) : Protocol {
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

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_error_metadata", jsonDeserModule) {
            rustTemplate(
                """
                // TODO(rpcv2): Implement `RpcV2.parseHttpErrorMetadata`
                """,
            )
        }

    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_error_metadata", jsonDeserModule) {
            rustTemplate(
                """
                // TODO(rpcv2): Implement `RpcV2.parseEventStreamErrorMetadata`
                """,
            )
        }
}
