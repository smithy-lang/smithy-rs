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
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator

class AwsQueryCompatibleHttpBindingResolver(
    private val awsQueryBindingResolver: AwsQueryBindingResolver,
    private val awsJsonHttpBindingResolver: AwsJsonHttpBindingResolver,
) : HttpBindingResolver {
    override fun httpTrait(operationShape: OperationShape): HttpTrait =
        awsJsonHttpBindingResolver.httpTrait(operationShape)

    override fun requestBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        awsJsonHttpBindingResolver.requestBindings(operationShape)

    override fun responseBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        awsJsonHttpBindingResolver.responseBindings(operationShape)

    override fun errorResponseBindings(errorShape: ToShapeId): List<HttpBindingDescriptor> =
        awsJsonHttpBindingResolver.errorResponseBindings(errorShape)

    override fun errorCode(errorShape: ToShapeId): String = awsQueryBindingResolver.errorCode(errorShape)

    override fun requestContentType(operationShape: OperationShape): String =
        awsJsonHttpBindingResolver.requestContentType(operationShape)

    override fun responseContentType(operationShape: OperationShape): String =
        awsJsonHttpBindingResolver.requestContentType(operationShape)

    override fun eventStreamMessageContentType(memberShape: MemberShape): String? =
        awsJsonHttpBindingResolver.eventStreamMessageContentType(memberShape)

    override fun handlesEventStreamInitialRequest(shape: Shape): Boolean =
        awsJsonHttpBindingResolver.handlesEventStreamInitialRequest(shape)

    override fun handlesEventStreamInitialResponse(shape: Shape): Boolean =
        awsJsonHttpBindingResolver.handlesEventStreamInitialResponse(shape)
}

class AwsQueryCompatible(
    val codegenContext: CodegenContext,
    private val awsJson: AwsJson,
) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
            "Headers" to RuntimeType.headers(runtimeConfig),
            "JsonError" to
                CargoDependency.smithyJson(runtimeConfig).toType()
                    .resolve("deserialize::error::DeserializeError"),
            "aws_query_compatible_errors" to RuntimeType.awsQueryCompatibleErrors(runtimeConfig),
            "json_errors" to RuntimeType.jsonErrors(runtimeConfig),
        )

    override val httpBindingResolver: HttpBindingResolver =
        AwsQueryCompatibleHttpBindingResolver(
            AwsQueryBindingResolver(codegenContext.model),
            AwsJsonHttpBindingResolver(codegenContext.model, awsJson.version, codegenContext.target == CodegenTarget.SERVER),
        )

    override val defaultTimestampFormat = awsJson.defaultTimestampFormat

    override fun structuredDataParser(): StructuredDataParserGenerator = awsJson.structuredDataParser()

    override fun structuredDataSerializer(): StructuredDataSerializerGenerator = awsJson.structuredDataSerializer()

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        ProtocolFunctions.crossOperationFn("parse_http_error_metadata") { fnName ->
            rustTemplate(
                """
                pub fn $fnName(_response_status: u16, response_headers: &#{Headers}, response_body: &[u8]) -> Result<#{ErrorMetadataBuilder}, #{JsonError}> {
                    let mut builder =
                        #{json_errors}::parse_error_metadata(response_body, response_headers)?;
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
            )
        }

    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType =
        awsJson.parseEventStreamErrorMetadata(operationShape)

    override fun additionalRequestHeaders(operationShape: OperationShape): List<Pair<String, String>> =
        listOf(
            "x-amz-target" to "${codegenContext.serviceShape.id.name}.${operationShape.id.name}",
            "x-amzn-query-mode" to "true",
        )
}
