/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.HttpChecksumTrait
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
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
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.orNull

private fun RuntimeConfig.awsInlineableHttpResponseChecksum() =
    RuntimeType.forInlineDependency(
        InlineAwsDependency.forRustFile(
            "http_response_checksum", visibility = Visibility.PUBCRATE,
            CargoDependency.Bytes,
            CargoDependency.Http0x,
            CargoDependency.HttpBody0x,
            CargoDependency.Tracing,
            CargoDependency.smithyChecksums(this),
            CargoDependency.smithyHttp(this),
            CargoDependency.smithyRuntimeApiClient(this),
            CargoDependency.smithyTypes(this),
        ),
    )

/**
 * Get the top-level operation input member used to opt-in to best-effort validation of a checksum returned in
 * the HTTP response of the operation.
 */
fun HttpChecksumTrait.requestValidationModeMember(
    codegenContext: ClientCodegenContext,
    operationShape: OperationShape,
): MemberShape? {
    val requestValidationModeMember = this.requestValidationModeMember.orNull() ?: return null
    return operationShape.inputShape(codegenContext.model).expectMember(requestValidationModeMember)
}

class HttpResponseChecksumDecorator : ClientCodegenDecorator {
    override val name: String = "HttpResponseChecksum"
    override val order: Byte = 0

    private fun applies(operationShape: OperationShape): Boolean =
        operationShape.outputShape != ShapeId.from("com.amazonaws.s3#GetObjectOutput")

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations.letIf(applies(operation)) {
            it + HttpResponseChecksumCustomization(codegenContext, operation)
        }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + HttpResponseChecksumConfigCustomization(codegenContext)

    /**
     * Copy the `response_checksum_validation` value from the `SdkConfig` to the client config
     */
    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        if (serviceHasHttpChecksumOperation(codegenContext)) {
            listOf(
                adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                    rust(
                        """
                        ${section.serviceConfigBuilder}.set_response_checksum_validation(${section.sdkConfig}.response_checksum_validation());
                        """,
                    )
                },
            )
        } else {
            listOf()
        }
}

// This generator was implemented based on this spec:
// https://smithy.io/2.0/aws/aws-core.html#http-request-checksums

/**
 * Calculate the checksum algorithm based on the input member identified by the trait's
 * `requestAlgorithmMember`. Then instantiate an (inlineable) `http_request_checksum`
 * interceptor with that checksum algorithm.
 */
class HttpResponseChecksumCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable =
        writable {
            // Return early if this operation lacks the `httpChecksum` trait or that trait lacks a `requestValidationModeMember`
            val checksumTrait = operationShape.getTrait<HttpChecksumTrait>() ?: return@writable
            val requestValidationModeMember =
                checksumTrait.requestValidationModeMember(codegenContext, operationShape) ?: return@writable
            val requestValidationModeName = codegenContext.symbolProvider.toSymbol(requestValidationModeMember).name
            val requestValidationModeMemberInner =
                if (requestValidationModeMember.isOptional) {
                    codegenContext.model.expectShape(requestValidationModeMember.target)
                } else {
                    requestValidationModeMember
                }
            val validationModeName = codegenContext.symbolProvider.toMemberName(requestValidationModeMember)
            val inputShape = codegenContext.model.expectShape(operationShape.inputShape)
            val operationName = codegenContext.symbolProvider.toSymbol(operationShape).name

            when (section) {
                is OperationSection.AdditionalInterceptors -> {
                    section.registerInterceptor(codegenContext.runtimeConfig, this) {
                        // CRC32, CRC32C, SHA256, SHA1 -> "crc32", "crc32c", "sha256", "sha1"
                        val responseAlgorithms =
                            checksumTrait.responseAlgorithms
                                .map { algorithm -> algorithm.lowercase() }
                                .joinToString(", ") { algorithm -> algorithm.dq() }
                        val runtimeApi = RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig)
                        rustTemplate(
                            """
                            #{ResponseChecksumInterceptor}::new(
                                [$responseAlgorithms].as_slice(),
                                |input: &#{Input}| {
                                    ${
                                ""/* Per [the spec](https://smithy.io/2.0/aws/aws-core.html#http-response-checksums),
                                           we check to see if it's the `ENABLED` variant */
                            }
                                    let input: &#{OperationInput} = input.downcast_ref().expect("correct type");
                                    matches!(input.$validationModeName(), #{Some}(#{ValidationModeShape}::Enabled))
                                },
                                |input: &mut #{Input}, cfg: &#{ConfigBag}|  {
                                    let input = input
                                        .downcast_mut::<#{OperationInputType}>()
                                        .ok_or("failed to downcast to #{OperationInputType}")?;

                                    let request_validation_enabled =
                                        matches!(input.$requestValidationModeName(), Some(#{ValidationModeShape}::Enabled));

                                    if !request_validation_enabled {
                                        // This value is set by the user on the SdkConfig to indicate their preference
                                        let response_checksum_validation = cfg
                                            .load::<#{ResponseChecksumValidation}>()
                                            .unwrap_or(&#{ResponseChecksumValidation}::WhenSupported);

                                        let is_presigned_req = cfg.load::<#{PresigningMarker}>().is_some();

                                        // For presigned requests we do not enable the checksum-mode header.
                                        if is_presigned_req {
                                            return #{Ok}(())
                                        }

                                        // If validation setting is WhenSupported (or unknown) we enable response checksum
                                        // validation. If it is WhenRequired we do not enable (since there is no way to
                                        // indicate that a response checksum is required).
                                        ##[allow(clippy::wildcard_in_or_patterns)]
                                        match response_checksum_validation {
                                            #{ResponseChecksumValidation}::WhenRequired => {}
                                            #{ResponseChecksumValidation}::WhenSupported | _ => {
                                                input.$requestValidationModeName = Some(#{ValidationModeShape}::Enabled);
                                            }
                                        }
                                    }

                                    #{Ok}(())
                                }
                            )
                            """,
                            *preludeScope,
                            "ResponseChecksumInterceptor" to
                                codegenContext.runtimeConfig.awsInlineableHttpResponseChecksum()
                                    .resolve("ResponseChecksumInterceptor"),
                            "Input" to runtimeApi.resolve("client::interceptors::context::Input"),
                            "OperationInput" to codegenContext.symbolProvider.toSymbol(inputShape),
                            "ValidationModeShape" to
                                codegenContext.symbolProvider.toSymbol(
                                    requestValidationModeMemberInner,
                                ),
                            "OperationInputType" to
                                codegenContext.symbolProvider.toSymbol(
                                    operationShape.inputShape(
                                        codegenContext.model,
                                    ),
                                ),
                            "ValidationModeShape" to
                                codegenContext.symbolProvider.toSymbol(
                                    requestValidationModeMemberInner,
                                ),
                            "ResponseChecksumValidation" to
                                CargoDependency.smithyTypes(codegenContext.runtimeConfig).toType()
                                    .resolve("checksum_config::ResponseChecksumValidation"),
                            "ConfigBag" to RuntimeType.configBag(codegenContext.runtimeConfig),
                            "PresigningMarker" to AwsRuntimeType.presigning().resolve("PresigningMarker"),
                        )
                    }
                }

                else -> {}
            }
        }
}

/**
 * Add a `response_checksum_validation;` field to Service config.
 */
class HttpResponseChecksumConfigCustomization(private val codegenContext: ClientCodegenContext) :
    NamedCustomization<ServiceConfig>() {
    private val rc = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "ResponseChecksumValidation" to
                configReexport(
                    RuntimeType.smithyTypes(rc)
                        .resolve("checksum_config::ResponseChecksumValidation"),
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
                        /// Return a reference to the response_checksum_validation value contained in this config, if any.
                        pub fn response_checksum_validation(&self) -> #{Option}<&#{ResponseChecksumValidation}> {
                            self.config.load::<#{ResponseChecksumValidation}>()
                        }
                        """,
                        *codegenScope,
                    )
                }

            is ServiceConfig.BuilderImpl ->
                writable {
                    rustTemplate(
                        """
                        /// Set the [`ResponseChecksumValidation`](#{ResponseChecksumValidation})
                        /// to determine when checksum validation will be performed on response payloads.
                        pub fn response_checksum_validation(
                            mut self,
                            response_checksum_validation: #{ResponseChecksumValidation}
                        ) -> Self {
                            self.set_response_checksum_validation(#{Some}(response_checksum_validation));
                            self
                        }
                        """,
                        *codegenScope,
                    )

                    rustTemplate(
                        """
                        /// Set the [`ResponseChecksumValidation`](#{ResponseChecksumValidation})
                        /// to determine when checksum validation will be performed on response payloads.
                        pub fn set_response_checksum_validation(
                            &mut self,
                            response_checksum_validation: #{Option}<#{ResponseChecksumValidation}>
                        ) -> &mut Self {
                            self.config.store_or_unset(response_checksum_validation);
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

            is ServiceConfig.BuilderFromConfigBag ->
                writable {
                    rustTemplate(
                        "${section.builder}.set_response_checksum_validation(${section.configBag}.load::<#{ResponseChecksumValidation}>().cloned());",
                        *codegenScope,
                    )
                }

            else -> emptySection
        }
    }
}
