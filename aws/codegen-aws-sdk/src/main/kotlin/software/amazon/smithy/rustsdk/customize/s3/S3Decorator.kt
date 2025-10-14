/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.DeprecatedTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rulesengine.traits.EndpointTestCase
import software.amazon.smithy.rulesengine.traits.EndpointTestOperationInput
import software.amazon.smithy.rulesengine.traits.EndpointTestsTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SyntheticNoAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.protocols.ClientRestXmlFactory
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestXml
import software.amazon.smithy.rust.codegen.core.smithy.traits.AllowInvalidXmlRoot
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rustsdk.AwsRuntimeType
import software.amazon.smithy.rustsdk.getBuiltIn
import software.amazon.smithy.rustsdk.toWritable
import java.util.logging.Logger

/**
 * Top level decorator for S3
 */
class S3Decorator : ClientCodegenDecorator {
    override val name: String = "S3"
    override val order: Byte = 0
    private val logger: Logger = Logger.getLogger(javaClass.name)
    private val invalidXmlRootAllowList =
        setOf(
            // To work around https://github.com/awslabs/aws-sdk-rust/issues/991
            ShapeId.from("com.amazonaws.s3#CreateSessionOutput"),
            // API returns GetObjectAttributes_Response_ instead of Output
            ShapeId.from("com.amazonaws.s3#GetObjectAttributesOutput"),
            // API returns ListAllMyDirectoryBucketsResult instead of ListDirectoryBucketsOutput
            ShapeId.from("com.amazonaws.s3#ListDirectoryBucketsOutput"),
        )

    // GetBucketLocation is deprecated because AWS recommends using HeadBucket instead
    // to determine a bucket's region
    private val deprecatedOperations =
        mapOf(
            ShapeId.from("com.amazonaws.s3#GetBucketLocation") to
                "Use HeadBucket operation instead to determine the bucket's region. For more information, see https://docs.aws.amazon.com/AmazonS3/latest/API/API_HeadBucket.html",
        )

    override fun protocols(
        serviceId: ShapeId,
        currentProtocols: ProtocolMap<OperationGenerator, ClientCodegenContext>,
    ): ProtocolMap<OperationGenerator, ClientCodegenContext> =
        currentProtocols +
            mapOf(
                RestXmlTrait.ID to
                    ClientRestXmlFactory { protocolConfig ->
                        S3ProtocolOverride(protocolConfig)
                    },
            )

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model =
        ModelTransformer.create().mapShapes(model) { shape ->
            shape.letIf(isInInvalidXmlRootAllowList(shape)) {
                logger.info("Adding AllowInvalidXmlRoot trait to $it")
                (it as StructureShape).toBuilder().addTrait(AllowInvalidXmlRoot()).build()
            }.letIf(isDeprecatedOperation(shape)) {
                logger.info("Adding DeprecatedTrait to $it")
                val message = deprecatedOperations[shape.id]!!
                (it as OperationShape).toBuilder().addTrait(createDeprecatedTrait(message)).build()
            }
        }
            // the model has the bucket in the path
            .let(StripBucketFromHttpPath()::transform)
            // the tests in EP2 are incorrect and are missing request route
            .let(
                FilterEndpointTests(
                    operationInputFilter = { input ->
                        when (input.operationName) {
                            // it's impossible to express HostPrefix behavior in the current EP2 rules schema :-/
                            // A handwritten test was written to cover this behavior
                            "WriteGetObjectResponse" -> null
                            else -> input
                        }
                    },
                )::transform,
            )
            // enable no auth for the service to support operations commonly used with public buckets
            .let(AddSyntheticNoAuth()::transform)
            .let(MakeS3BoolsAndNumbersOptional()::processModel)

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        return listOf(
            object : EndpointCustomization {
                override fun setBuiltInOnServiceConfig(
                    name: String,
                    value: Node,
                    configBuilderRef: String,
                ): Writable? {
                    if (!name.startsWith("AWS::S3")) {
                        return null
                    }
                    val builtIn = codegenContext.getBuiltIn(name) ?: return null
                    return writable {
                        rustTemplate(
                            "let $configBuilderRef = $configBuilderRef.${builtIn.name.rustName()}(#{value});",
                            "value" to value.toWritable(),
                        )
                    }
                }
            },
        )
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations +
            object : OperationCustomization() {
                private val runtimeConfig = codegenContext.runtimeConfig
                private val symbolProvider = codegenContext.symbolProvider

                override fun section(section: OperationSection): Writable {
                    return writable {
                        when (section) {
                            is OperationSection.BeforeParseResponse -> {
                                section.body?.also { body ->
                                    rustTemplate(
                                        """
                                        if matches!(#{errors}::body_is_error($body), Ok(true)) {
                                            ${section.forceError} = true;
                                        }
                                        """,
                                        "errors" to RuntimeType.unwrappedXmlErrors(runtimeConfig),
                                    )
                                }
                            }

                            is OperationSection.RetryClassifiers -> {
                                section.registerRetryClassifier(this) {
                                    rustTemplate(
                                        """
                                        #{AwsErrorCodeClassifier}::<#{OperationError}>::builder().transient_errors({
                                            let mut transient_errors: Vec<&'static str> = #{TRANSIENT_ERRORS}.into();
                                            transient_errors.push("InternalError");
                                            #{Cow}::Owned(transient_errors)
                                            }).build()""",
                                        "AwsErrorCodeClassifier" to
                                            AwsRuntimeType.awsRuntime(runtimeConfig)
                                                .resolve("retries::classifiers::AwsErrorCodeClassifier"),
                                        "Cow" to RuntimeType.Cow,
                                        "OperationError" to symbolProvider.symbolForOperationError(operation),
                                        "TRANSIENT_ERRORS" to
                                            AwsRuntimeType.awsRuntime(runtimeConfig)
                                                .resolve("retries::classifiers::TRANSIENT_ERRORS"),
                                    )
                                }
                            }

                            else -> {}
                        }
                    }
                }
            }
    }

    private fun isInInvalidXmlRootAllowList(shape: Shape): Boolean {
        return shape.isStructureShape && invalidXmlRootAllowList.contains(shape.id)
    }

    /**
     * Checks if the given shape is an operation that should be marked as deprecated.
     */
    private fun isDeprecatedOperation(shape: Shape): Boolean {
        return shape.isOperationShape && deprecatedOperations.containsKey(shape.id)
    }

    /**
     * Creates a DeprecatedTrait with the specified deprecation message.
     */
    private fun createDeprecatedTrait(message: String): DeprecatedTrait {
        return DeprecatedTrait.builder()
            .message(message)
            .build()
    }
}

class FilterEndpointTests(
    private val testFilter: (EndpointTestCase) -> EndpointTestCase? = { a -> a },
    private val operationInputFilter: (EndpointTestOperationInput) -> EndpointTestOperationInput? = { a -> a },
) {
    private fun updateEndpointTests(endpointTests: List<EndpointTestCase>): List<EndpointTestCase> {
        val filteredTests = endpointTests.mapNotNull { test -> testFilter(test) }
        return filteredTests.map { test ->
            val operationInputs = test.operationInputs
            test.toBuilder().operationInputs(operationInputs.mapNotNull { operationInputFilter(it) }).build()
        }
    }

    fun transform(model: Model): Model =
        ModelTransformer.create().mapTraits(model) { _, trait ->
            when (trait) {
                is EndpointTestsTrait ->
                    EndpointTestsTrait.builder().testCases(updateEndpointTests(trait.testCases))
                        .version(trait.version).build()

                else -> trait
            }
        }
}

// TODO(P96049742): This model transform may need to change depending on if and how the S3 model is updated.
// See the comment in the `SyntheticNoAuthTrait` class for why we use a synthetic trait instead of `optionalAuth`.
private class AddSyntheticNoAuth {
    fun transform(model: Model): Model =
        ModelTransformer.create().mapShapes(model) { shape ->
            if (shape is ServiceShape) {
                shape.toBuilder()
                    .addTrait(SyntheticNoAuthTrait())
                    .build()
            } else {
                shape
            }
        }
}

class S3ProtocolOverride(codegenContext: CodegenContext) : RestXml(codegenContext) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope =
        arrayOf(
            *RuntimeType.preludeScope,
            "Bytes" to RuntimeType.Bytes,
            "ErrorMetadata" to RuntimeType.errorMetadata(runtimeConfig),
            "ErrorBuilder" to RuntimeType.errorMetadataBuilder(runtimeConfig),
            "Headers" to RuntimeType.headers(runtimeConfig),
            "XmlDecodeError" to RuntimeType.smithyXml(runtimeConfig).resolve("decode::XmlDecodeError"),
            "base_errors" to restXmlErrors,
        )

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType {
        return ProtocolFunctions.crossOperationFn("parse_http_error_metadata") { fnName ->
            rustBlockTemplate(
                "pub fn $fnName(response_status: u16, _response_headers: &#{Headers}, response_body: &[u8]) -> #{Result}<#{ErrorBuilder}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rustTemplate(
                    """
                    // S3 HEAD responses have no response body to for an error code. Therefore,
                    // check the HTTP response status and populate an error code for 404s.
                    if response_body.is_empty() {
                        let mut builder = #{ErrorMetadata}::builder();
                        if response_status == 404 {
                            builder = builder.code("NotFound");
                        }
                        Ok(builder)
                    } else {
                        #{base_errors}::parse_error_metadata(response_body)
                    }
                    """,
                    *errorScope,
                )
            }
        }
    }
}
