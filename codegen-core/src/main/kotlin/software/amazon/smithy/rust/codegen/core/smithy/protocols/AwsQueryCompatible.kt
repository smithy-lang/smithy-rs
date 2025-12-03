/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator

class AwsQueryCompatibleHttpBindingResolver(
    private val awsQueryBindingResolver: AwsQueryBindingResolver,
    private val targetProtocolHttpBinding: HttpBindingResolver,
) : HttpBindingResolver {
    override fun httpTrait(operationShape: OperationShape): HttpTrait =
        targetProtocolHttpBinding.httpTrait(operationShape)

    override fun requestBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        targetProtocolHttpBinding.requestBindings(operationShape)

    override fun responseBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        targetProtocolHttpBinding.responseBindings(operationShape)

    override fun errorResponseBindings(errorShape: ToShapeId): List<HttpBindingDescriptor> =
        targetProtocolHttpBinding.errorResponseBindings(errorShape)

    override fun errorCode(errorShape: ToShapeId): String = awsQueryBindingResolver.errorCode(errorShape)

    override fun requestContentType(operationShape: OperationShape): String? =
        targetProtocolHttpBinding.requestContentType(operationShape)

    override fun responseContentType(operationShape: OperationShape): String? =
        targetProtocolHttpBinding.requestContentType(operationShape)

    override fun eventStreamMessageContentType(memberShape: MemberShape): String? =
        targetProtocolHttpBinding.eventStreamMessageContentType(memberShape)

    override fun handlesEventStreamInitialRequest(shape: Shape): Boolean =
        targetProtocolHttpBinding.handlesEventStreamInitialRequest(shape)

    override fun handlesEventStreamInitialResponse(shape: Shape): Boolean =
        targetProtocolHttpBinding.handlesEventStreamInitialResponse(shape)

    override fun isRpcProtocol() = targetProtocolHttpBinding.isRpcProtocol()
}

data class ParseErrorMetadataParams(
    val deserializeErrorType: RuntimeType,
    val innerParseErrorMetadata: Writable,
)

class AwsQueryCompatible(
    val codegenContext: CodegenContext,
    private val targetProtocol: Protocol,
    private val params: ParseErrorMetadataParams,
) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
            "Headers" to RuntimeType.headers(runtimeConfig),
            "aws_query_compatible_errors" to RuntimeType.awsQueryCompatibleErrors(runtimeConfig),
            *RuntimeType.preludeScope,
        )

    override val httpBindingResolver: HttpBindingResolver =
        AwsQueryCompatibleHttpBindingResolver(
            AwsQueryBindingResolver(codegenContext.model),
            targetProtocol.httpBindingResolver,
        )

    override val defaultTimestampFormat = targetProtocol.defaultTimestampFormat

    override fun structuredDataParser(): StructuredDataParserGenerator = targetProtocol.structuredDataParser()

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator =
        targetProtocol.structuredDataSerializer()

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_http_error_metadata") { fnName ->
            rustTemplate(
                """
                pub fn $fnName(_response_status: u16, response_headers: &#{Headers}, response_body: &[u8]) -> #{Result}<#{ErrorMetadataBuilder}, #{DeserializeError}> {
                    let mut builder = #{parse_error_metadata};
                    if let Some((error_code, error_type)) =
                        #{aws_query_compatible_errors}::parse_aws_query_compatible_error(response_headers)
                    {
                        builder = builder.code(error_code);
                        builder = builder.custom("type", error_type);
                    }
                    Ok(builder)
                }
                """,
                *errorScope,
                "DeserializeError" to params.deserializeErrorType,
                "parse_error_metadata" to params.innerParseErrorMetadata,
            )
        }

    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        targetProtocol.parseEventStreamErrorMetadata(operationShape)

    override fun additionalRequestHeaders(operationShape: OperationShape): List<Pair<String, String>> =
        targetProtocol.additionalRequestHeaders(operationShape) +
            listOf(
                "x-amzn-query-mode" to "true",
            )
}
