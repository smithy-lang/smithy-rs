/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.HttpChecksumTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.util.expectMember
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import kotlin.jvm.optionals.getOrNull

internal fun RuntimeConfig.awsInlineableHttpRequestChecksum() =
    RuntimeType.forInlineDependency(
        InlineAwsDependency.forRustFile(
            "http_request_checksum", visibility = Visibility.PUBCRATE,
            CargoDependency.Bytes,
            CargoDependency.Http,
            CargoDependency.HttpBody,
            CargoDependency.Tracing,
            AwsCargoDependency.awsRuntime(this).withFeature("http-02x"),
            CargoDependency.smithyChecksums(this),
            CargoDependency.smithyHttp(this),
            CargoDependency.smithyRuntimeApiClient(this),
            CargoDependency.smithyTypes(this),
            AwsCargoDependency.awsSigv4(this),
            CargoDependency.TempFile.toDevDependency(),
        ),
    )

class HttpRequestChecksumDecorator : ClientCodegenDecorator {
    override val name: String = "HttpRequestChecksum"
    override val order: Byte = 0
    private val defaultAlgorithm = "CRC32"

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations + HttpRequestChecksumCustomization(codegenContext, operation)

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + HttpRequestChecksumConfigCustomization(codegenContext)

    /**
     * Copy the `request_checksum_calculation` value from the `SdkConfig` to the client config
     */
    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        if (!serviceHasHttpChecksumOperation(codegenContext)) {
            listOf()
        } else {
            listOf(
                adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                    rust(
                        """
                        ${section.serviceConfigBuilder}.set_request_checksum_calculation(${section.sdkConfig}.request_checksum_calculation());
                        """,
                    )
                },
            )
        }

//    override fun transformModel(service: ServiceShape, model: Model, settings: ClientRustSettings): Model =
//        ModelTransformer.create().mapShapes(model) { shape ->
//            shape.letIf(isChecksumAlgoRequestMember(shape)) {
//                // Update the default on the CreateMPURequest shape
//                (shape as MemberShape).toBuilder().addTrait(DefaultTrait(Node.from(defaultAlgorithm))).build()
//            }
//        }
//
//
//    private fun isChecksumAlgoRequestMember(service: ServiceShape, currentShape: Shape): Boolean {
//        val checksumTrait = currentShape.getTrait<HttpChecksumTrait>() ?: return false
//
//        return true
//    }
}

/**
 * Extract the name of the operation's input member that indicates which checksum algorithm to use
 */
private fun HttpChecksumTrait.requestAlgorithmMember(
    codegenContext: ClientCodegenContext,
    operationShape: OperationShape,
): String? {
    val requestAlgorithmMember = this.requestAlgorithmMember.orNull() ?: return null
    val checksumAlgorithmMemberShape =
        operationShape.inputShape(codegenContext.model).expectMember(requestAlgorithmMember)

    return codegenContext.symbolProvider.toMemberName(checksumAlgorithmMemberShape)
}

/**
 * Extract the name of the operation's input member that indicates which checksum algorithm to use
 */
private fun HttpChecksumTrait.checksumAlgorithmToStr(
    codegenContext: ClientCodegenContext,
    operationShape: OperationShape,
): Writable {
    val runtimeConfig = codegenContext.runtimeConfig
    val requestAlgorithmMember = this.requestAlgorithmMember(codegenContext, operationShape)
    val isRequestChecksumRequired = this.isRequestChecksumRequired

    return {
        if (requestAlgorithmMember == null && isRequestChecksumRequired) {
            // Checksums are required but a user can't set one, so we set crc32 for them
            rust("""let checksum_algorithm = Some("crc32");""")
        } else {
            // Use checksum algo set by user or crc32 if one has not been set
            rust("""let checksum_algorithm = checksum_algorithm.map(|algorithm| algorithm.as_str()).or(Some("crc32"));""")
        }

        // Parse the checksum_algorithm to a ChecksumAlgorithm enum
        rustTemplate(
            """
            let checksum_algorithm = match checksum_algorithm {
                Some(algo) => Some(
                    algo.parse::<#{ChecksumAlgorithm}>()
                    .map_err(#{BuildError}::other)?
                ),
                None => None,
            };
            """,
            "BuildError" to runtimeConfig.operationBuildError(),
            "ChecksumAlgorithm" to RuntimeType.smithyChecksums(runtimeConfig).resolve("ChecksumAlgorithm"),
        )

        // If a request checksum is not required and there's no way to set one, do nothing
        // This happens when an operation only supports response checksums
    }
}

// This generator was implemented based on this spec:
// https://smithy.io/2.0/aws/aws-core.html#http-request-checksums

/**
 * Calculate the checksum algorithm based on the input member identified by the trait's
 * `requestAlgorithmMember`. Then instantiate an (inlineable) `http_request_checksum`
 * interceptor with that checksum algorithm.
 */
class HttpRequestChecksumCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig

    override fun section(section: OperationSection): Writable =
        writable {
            // Get the `HttpChecksumTrait`, returning early if this `OperationShape` doesn't have one
            val checksumTrait = operationShape.getTrait<HttpChecksumTrait>() ?: return@writable
            val requestAlgorithmMember = checksumTrait.requestAlgorithmMember(codegenContext, operationShape)
            val inputShape = codegenContext.model.expectShape(operationShape.inputShape)
            val requestChecksumRequired = checksumTrait.isRequestChecksumRequired

            when (section) {
                is OperationSection.AdditionalInterceptors -> {
                    if (requestAlgorithmMember != null) {
                        section.registerInterceptor(runtimeConfig, this) {
                            val runtimeApi = RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                            rustTemplate(
                                """
                                #{RequestChecksumInterceptor}::new(|input: &#{Input}| {
                                    let input: &#{OperationInput} = input.downcast_ref().expect("correct type");
                                    let checksum_algorithm = input.$requestAlgorithmMember();
                                    #{checksum_algorithm_to_str}
                                    #{Result}::<_, #{BoxError}>::Ok((checksum_algorithm, $requestChecksumRequired))
                                })
                                """,
                                *preludeScope,
                                "BoxError" to RuntimeType.boxError(runtimeConfig),
                                "Input" to runtimeApi.resolve("client::interceptors::context::Input"),
                                "OperationInput" to codegenContext.symbolProvider.toSymbol(inputShape),
                                "RequestChecksumInterceptor" to
                                    runtimeConfig.awsInlineableHttpRequestChecksum()
                                        .resolve("RequestChecksumInterceptor"),
                                "checksum_algorithm_to_str" to
                                    checksumTrait.checksumAlgorithmToStr(
                                        codegenContext,
                                        operationShape,
                                    ),
                            )
                        }
                    }
                }

                else -> {}
            }
        }
}

/**
 * Add a `request_checksum_calculation;` field to Service config.
 */
class HttpRequestChecksumConfigCustomization(private val codegenContext: ClientCodegenContext) :
    NamedCustomization<ServiceConfig>() {
    private val rc = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "RequestChecksumCalculation" to
                configReexport(
                    RuntimeType.smithyChecksums(rc)
                        .resolve("RequestChecksumCalculation"),
                ),
        )

    override fun section(section: ServiceConfig): Writable {
        // If the service contains no operations with the httpChecksum trait we return early
        if (!serviceHasHttpChecksumOperation(codegenContext)) {
            return emptySection
        }

        // Otherwise we write the necessary sections to the service config
        return when (section) {
            is ServiceConfig.ConfigImpl ->
                writable {
                    rustTemplate(
                        """
                        /// Return a reference to the request_checksum_calculation value contained in this config, if any.
                        pub fn request_checksum_calculation(&self) -> #{Option}<&#{RequestChecksumCalculation}> {
                            self.config.load::<#{RequestChecksumCalculation}>()
                        }
                        """,
                        *codegenScope,
                    )
                }

            is ServiceConfig.BuilderImpl ->
                writable {
                    rustTemplate(
                        """
                        /// Set the [`RequestChecksumCalculation`](#{RequestChecksumCalculation})
                        /// to configure checksum behavior.
                        pub fn request_checksum_calculation(
                            mut self,
                            request_checksum_calculation: #{RequestChecksumCalculation}
                        ) -> Self {
                            self.set_request_checksum_calculation(#{Some}(request_checksum_calculation));
                            self
                        }
                        """,
                        *codegenScope,
                    )

                    rustTemplate(
                        """
                        /// Set the [`RequestChecksumCalculation`](#{RequestChecksumCalculation})
                        /// to configure checksum behavior.
                        pub fn set_request_checksum_calculation(
                            &mut self,
                            request_checksum_calculation: #{Option}<#{RequestChecksumCalculation}>
                        ) -> &mut Self {
                            self.config.store_or_unset(request_checksum_calculation);
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

            is ServiceConfig.BuilderFromConfigBag ->
                writable {
                    rustTemplate(
                        "${section.builder}.set_request_checksum_calculation(${section.configBag}.load::<#{RequestChecksumCalculation}>().cloned());",
                        *codegenScope,
                    )
                }

            else -> emptySection
        }
    }
}

/**
 * Determine if the current service contains any operations with the HttpChecksum trait
 */
fun serviceHasHttpChecksumOperation(codegenContext: ClientCodegenContext) =
    codegenContext.serviceShape.allOperations
        .mapNotNull { codegenContext.model.getShape(it).getOrNull() }
        .any {
            it.hasTrait<HttpChecksumTrait>()
        }
