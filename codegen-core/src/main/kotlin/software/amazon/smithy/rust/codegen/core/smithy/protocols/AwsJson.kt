/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.pattern.UriPattern
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.builderSymbolFn
import software.amazon.smithy.rust.codegen.core.smithy.generators.serializationError
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.util.inputShape

sealed class AwsJsonVersion {
    abstract val value: String

    object Json10 : AwsJsonVersion() {
        override val value = "1.0"
    }

    object Json11 : AwsJsonVersion() {
        override val value = "1.1"
    }
}

class AwsJsonHttpBindingResolver(
    private val model: Model,
    private val awsJsonVersion: AwsJsonVersion,
) : HttpBindingResolver {
    private val httpTrait = HttpTrait.builder()
        .code(200)
        .method("POST")
        .uri(UriPattern.parse("/"))
        .build()

    private fun bindings(shape: ToShapeId) =
        shape.let { model.expectShape(it.toShapeId()) }.members()
            .map { HttpBindingDescriptor(it, HttpLocation.DOCUMENT, "document") }
            .toList()

    override fun httpTrait(operationShape: OperationShape): HttpTrait = httpTrait

    override fun requestBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        bindings(operationShape.inputShape)

    override fun responseBindings(operationShape: OperationShape): List<HttpBindingDescriptor> =
        bindings(operationShape.outputShape)

    override fun errorResponseBindings(errorShape: ToShapeId): List<HttpBindingDescriptor> =
        bindings(errorShape)

    override fun requestContentType(operationShape: OperationShape): String =
        "application/x-amz-json-${awsJsonVersion.value}"

    override fun responseContentType(operationShape: OperationShape): String = requestContentType(operationShape)
}

/**
 * AwsJson requires all empty inputs to be sent across as `{}`. This class
 * customizes wraps [JsonSerializerGenerator] to add this functionality.
 */
class AwsJsonSerializerGenerator(
    private val codegenContext: CodegenContext,
    httpBindingResolver: HttpBindingResolver,
    private val jsonSerializerGenerator: JsonSerializerGenerator =
        JsonSerializerGenerator(codegenContext, httpBindingResolver, ::awsJsonFieldName),
) : StructuredDataSerializerGenerator by jsonSerializerGenerator {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "Error" to runtimeConfig.serializationError(),
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
    )

    override fun operationInputSerializer(operationShape: OperationShape): RuntimeType {
        var serializer = jsonSerializerGenerator.operationInputSerializer(operationShape)
        if (serializer == null) {
            val inputShape = operationShape.inputShape(codegenContext.model)
            val fnName = codegenContext.symbolProvider.serializeFunctionName(operationShape)
            serializer = RuntimeType.forInlineFun(fnName, RustModule.private("operation_ser")) {
                rustBlockTemplate(
                    "pub fn $fnName(_input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                    *codegenScope, "target" to codegenContext.symbolProvider.toSymbol(inputShape),
                ) {
                    rustTemplate("""Ok(#{SdkBody}::from("{}"))""", *codegenScope)
                }
            }
        }
        return serializer
    }
}

open class AwsJson(
    val codegenContext: CodegenContext,
    val awsJsonVersion: AwsJsonVersion,
) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "Error" to RuntimeType.genericError(runtimeConfig),
        "HeaderMap" to RuntimeType.Http.resolve("HeaderMap"),
        "JsonError" to CargoDependency.smithyJson(runtimeConfig).toType()
            .resolve("deserialize::error::DeserializeError"),
        "Response" to RuntimeType.Http.resolve("Response"),
        "json_errors" to RuntimeType.jsonErrors(runtimeConfig),
    )
    private val jsonDeserModule = RustModule.private("json_deser")

    val version: AwsJsonVersion get() = awsJsonVersion

    override val httpBindingResolver: HttpBindingResolver =
        AwsJsonHttpBindingResolver(codegenContext.model, awsJsonVersion)

    override val defaultTimestampFormat: TimestampFormatTrait.Format = TimestampFormatTrait.Format.EPOCH_SECONDS

    override fun additionalRequestHeaders(operationShape: OperationShape): List<Pair<String, String>> =
        listOf("x-amz-target" to "${codegenContext.serviceShape.id.name}.${operationShape.id.name}")

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator {
        return JsonParserGenerator(
            codegenContext,
            httpBindingResolver,
            ::awsJsonFieldName,
            builderSymbolFn(codegenContext.symbolProvider),
        )
    }

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator =
        AwsJsonSerializerGenerator(codegenContext, httpBindingResolver)

    override fun parseHttpGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_generic_error", jsonDeserModule) {
            rustTemplate(
                """
                pub fn parse_http_generic_error(response: &#{Response}<#{Bytes}>) -> Result<#{Error}, #{JsonError}> {
                    #{json_errors}::parse_generic_error(response.body(), response.headers())
                }
                """,
                *errorScope,
            )
        }

    override fun parseEventStreamGenericError(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_event_stream_generic_error", jsonDeserModule) {
            rustTemplate(
                """
                pub fn parse_event_stream_generic_error(payload: &#{Bytes}) -> Result<#{Error}, #{JsonError}> {
                    // Note: HeaderMap::new() doesn't allocate
                    #{json_errors}::parse_generic_error(payload, &#{HeaderMap}::new())
                }
                """,
                *errorScope,
            )
        }
}

fun awsJsonFieldName(member: MemberShape): String = member.memberName
