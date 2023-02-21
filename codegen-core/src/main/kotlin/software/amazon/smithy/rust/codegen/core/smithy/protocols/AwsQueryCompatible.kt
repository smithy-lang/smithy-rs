package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ToShapeId
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
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

    override fun errorCode(errorShape: ToShapeId): String =
        awsQueryBindingResolver.errorCode(errorShape)

    override fun requestContentType(operationShape: OperationShape): String =
        awsJsonHttpBindingResolver.requestContentType(operationShape)

    override fun responseContentType(operationShape: OperationShape): String =
        awsJsonHttpBindingResolver.requestContentType(operationShape)
}

class AwsQueryCompatible(
    val codegenContext: CodegenContext,
    private val awsJson: AwsJson,
) : Protocol {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "ErrorMetadataBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
        "HeaderMap" to RuntimeType.Http.resolve("HeaderMap"),
        "HeaderValue" to RuntimeType.Http.resolve("HeaderValue"),
        "JsonError" to CargoDependency.smithyJson(runtimeConfig).toType()
            .resolve("deserialize::error::DeserializeError"),
        "Response" to RuntimeType.Http.resolve("Response"),
        "json_errors" to RuntimeType.jsonErrors(runtimeConfig),
        "aws_query_compatible_errors" to RuntimeType.awsQueryCompatibleErrors(runtimeConfig),
    )
    private val jsonDeserModule = RustModule.private("json_deser")

    override val httpBindingResolver: HttpBindingResolver =
        AwsQueryCompatibleHttpBindingResolver(
            AwsQueryBindingResolver(codegenContext.model),
            AwsJsonHttpBindingResolver(codegenContext.model, awsJson.version),
        )

    override val defaultTimestampFormat = awsJson.defaultTimestampFormat

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator =
        awsJson.structuredDataParser(operationShape)

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator =
        awsJson.structuredDataSerializer(operationShape)

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType =
        RuntimeType.forInlineFun("parse_http_error_metadata", jsonDeserModule) {
            rustTemplate(
                """
                pub fn parse_http_error_metadata(response: &#{Response}<#{Bytes}>) -> Result<#{ErrorMetadataBuilder}, #{JsonError}> {
                    let mut builder =
                        #{json_errors}::parse_error_metadata(response.body(), response.headers())?;
                    if let Some((error_code, error_type)) =
                        #{aws_query_compatible_errors}::parse_aws_query_compatible_error(response.headers())
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
}
