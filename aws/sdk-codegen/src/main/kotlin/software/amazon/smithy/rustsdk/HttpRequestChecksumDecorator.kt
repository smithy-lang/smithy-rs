/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.HttpChecksumTrait
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.HttpHeaderTrait
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
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectMember
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull

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
        if (serviceHasHttpChecksumOperation(codegenContext)) {
            listOf(
                adhocCustomization<ServiceConfigSection.MergeFromSharedConfig> { section ->
                    rustTemplate(
                        """
                        if self.field_never_set::<#{RequestChecksumCalculation}>() {
                            self.set_request_checksum_calculation(${section.sdkConfig}.request_checksum_calculation());
                        }
                        """,
                        "RequestChecksumCalculation" to
                            RuntimeType.smithyTypes(codegenContext.runtimeConfig)
                                .resolve("checksum_config::RequestChecksumCalculation"),
                    )
                },
            )
        } else {
            listOf()
        }
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
            // Use checksum algo set by user
            rust("""let checksum_algorithm = checksum_algorithm.map(|algorithm| algorithm.as_str());""")
        }

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
            // if there is no request algorithm member or if there is no httpHeader for the request algorithm
            val checksumTrait = operationShape.getTrait<HttpChecksumTrait>() ?: return@writable
            val requestAlgorithmMember =
                checksumTrait.requestAlgorithmMemberShape(codegenContext, operationShape) ?: return@writable
            val requestAlgoHeader =
                requestAlgorithmMember.getTrait<HttpHeaderTrait>()?.value ?: return@writable
            val requestAlgorithmMemberName = checksumTrait.requestAlgorithmMember(codegenContext, operationShape)
            val inputShape = codegenContext.model.expectShape(operationShape.inputShape)
            val requestChecksumRequired = checksumTrait.isRequestChecksumRequired
            val operationName = codegenContext.symbolProvider.toSymbol(operationShape).name
            val requestAlgorithmMemberInner =
                if (requestAlgorithmMember.isOptional) {
                    codegenContext.model.expectShape(requestAlgorithmMember.target)
                } else {
                    requestAlgorithmMember
                }

            when (section) {
                is OperationSection.AdditionalInterceptors -> {
                    if (requestAlgorithmMemberName != null) {
                        section.registerInterceptor(runtimeConfig, this) {
                            val runtimeApi = RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                            rustTemplate(
                                """
                                #{RequestChecksumInterceptor}::new(
                                |input: &#{Input}| {
                                    let input: &#{OperationInput} = input.downcast_ref().expect("correct type");
                                    let checksum_algorithm = input.$requestAlgorithmMemberName();
                                    #{checksum_algorithm_to_str}
                                    (checksum_algorithm.map(|s| s.to_string()), $requestChecksumRequired)
                                },
                                |request: &mut #{Request}, cfg: &#{ConfigBag}| {
                                    // We check if the user has set any of the checksum values manually
                                    let mut user_set_checksum_value = false;
                                    let headers_to_check = request.headers().iter().filter_map(|(name, _val)| {
                                        if name.starts_with("x-amz-checksum-") {
                                            Some(name)
                                        } else {
                                            None
                                        }
                                    });
                                    for algo_header in headers_to_check {
                                        if request.headers().get(algo_header).is_some() {
                                            user_set_checksum_value = true;
                                        }
                                    }

                                    // We check if the user set the checksum algo manually
                                    let user_set_checksum_algo = request.headers()
                                        .get(${requestAlgoHeader.dq()})
                                        .is_some();

                                    // This value is set by the user on the SdkConfig to indicate their preference
                                    let request_checksum_calculation = cfg
                                        .load::<#{RequestChecksumCalculation}>()
                                        .unwrap_or(&#{RequestChecksumCalculation}::WhenSupported);

                                    // From the httpChecksum trait
                                    let http_checksum_required = $requestChecksumRequired;

                                    let is_presigned_req = cfg.load::<#{PresigningMarker}>().is_some();

                                    // If the request is presigned we do not set a default.
                                    // If the RequestChecksumCalculation is WhenSupported and the user has not set a checksum value or algo
                                    // we default to Crc32. If it is WhenRequired and a checksum is required by the trait and the user has not
                                    // set a checksum value or algo we also set the default. In all other cases we do nothing.
                                    match (
                                        request_checksum_calculation,
                                        http_checksum_required,
                                        user_set_checksum_value,
                                        user_set_checksum_algo,
                                        is_presigned_req,
                                    ) {
                                        (_, _, _, _, true) => {}
                                        (#{RequestChecksumCalculation}::WhenSupported, _, false, false, _)
                                        | (#{RequestChecksumCalculation}::WhenRequired, true, false, false, _) => {
                                            request.headers_mut().insert(${requestAlgoHeader.dq()}, "CRC32");
                                        }
                                        _ => {},
                                    }

                                    // We return a bool indicating if the user did set the checksum value, if they did
                                    // we can short circuit and exit the interceptor early.
                                    Ok(user_set_checksum_value)
                                }
                                )
                                """,
                                *preludeScope,
                                "BoxError" to RuntimeType.boxError(runtimeConfig),
                                "Input" to runtimeApi.resolve("client::interceptors::context::Input"),
                                "Request" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("http::Request"),
                                "OperationInput" to codegenContext.symbolProvider.toSymbol(inputShape),
                                "ConfigBag" to RuntimeType.configBag(runtimeConfig),
                                "RequestChecksumInterceptor" to
                                    runtimeConfig.awsInlineableHttpRequestChecksum()
                                        .resolve("RequestChecksumInterceptor"),
                                "checksum_algorithm_to_str" to
                                    checksumTrait.checksumAlgorithmToStr(
                                        codegenContext,
                                        operationShape,
                                    ),
                                "RequestChecksumCalculation" to
                                    CargoDependency.smithyTypes(runtimeConfig).toType()
                                        .resolve("checksum_config::RequestChecksumCalculation"),
                                "ChecksumAlgoShape" to
                                    codegenContext.symbolProvider.toSymbol(
                                        requestAlgorithmMemberInner,
                                    ),
                                "OperationInputType" to
                                    codegenContext.symbolProvider.toSymbol(
                                        operationShape.inputShape(
                                            codegenContext.model,
                                        ),
                                    ),
                                "PresigningMarker" to AwsRuntimeType.presigning().resolve("PresigningMarker"),
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
                    RuntimeType.smithyTypes(rc)
                        .resolve("checksum_config::RequestChecksumCalculation"),
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
                        /// to determine when a checksum will be calculated for request payloads.
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
                        /// to determine when a checksum will be calculated for request payloads.
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
fun serviceHasHttpChecksumOperation(codegenContext: ClientCodegenContext): Boolean {
    val index = TopDownIndex.of(codegenContext.model)
    val ops = index.getContainedOperations(codegenContext.serviceShape.id)
    return ops.any { it.hasTrait<HttpChecksumTrait>() }
}

/**
 * Get the top-level operation input member used to opt-in to best-effort validation of a checksum returned in
 * the HTTP response of the operation.
 */
fun HttpChecksumTrait.requestAlgorithmMemberShape(
    codegenContext: ClientCodegenContext,
    operationShape: OperationShape,
): MemberShape? {
    val requestAlgorithmMember = this.requestAlgorithmMember.orNull() ?: return null
    return operationShape.inputShape(codegenContext.model).expectMember(requestAlgorithmMember)
}
