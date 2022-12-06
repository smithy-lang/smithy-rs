/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.aws.traits.auth.UnsignedPayloadTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.OptionalAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.EventStreamSigningConfig
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamOperations
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isInputEventStream
import software.amazon.smithy.rust.codegen.core.util.letIf

/**
 * The SigV4SigningDecorator:
 * - adds a `signing_service()` method to `config` to return the default signing service
 * - adds a `new_event_stream_signer()` method to `config` to create an Event Stream SigV4 signer
 * - sets the `SigningService` during operation construction
 * - sets a default `OperationSigningConfig` A future enhancement will customize this for specific services that need
 *   different behavior.
 */
class SigV4SigningDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "SigV4Signing"
    override val order: Byte = 0

    private fun applies(codegenContext: CodegenContext): Boolean = codegenContext.serviceShape.hasTrait<SigV4Trait>()

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations.letIf(applies(codegenContext)) { customizations ->
            customizations + SigV4SigningConfig(
                codegenContext.runtimeConfig,
                codegenContext.serviceShape.hasEventStreamOperations(codegenContext.model),
                codegenContext.serviceShape.expectTrait(),
            )
        }
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations.letIf(applies(codegenContext)) {
            it + SigV4SigningFeature(
                codegenContext.model,
                operation,
                codegenContext.runtimeConfig,
                codegenContext.serviceShape,
            )
        }
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

class SigV4SigningConfig(
    runtimeConfig: RuntimeConfig,
    private val serviceHasEventStream: Boolean,
    private val sigV4Trait: SigV4Trait,
) : EventStreamSigningConfig(runtimeConfig) {
    private val codegenScope = arrayOf(
        "SigV4Signer" to AwsRuntimeType.awsSigAuthEventStream(runtimeConfig).resolve("event_stream::SigV4Signer"),
    )

    override fun configImplSection(): Writable {
        return writable {
            rustTemplate(
                """
                /// The signature version 4 service signing name to use in the credential scope when signing requests.
                ///
                /// The signing service may be overridden by the `Endpoint`, or by specifying a custom
                /// [`SigningService`](aws_types::SigningService) during operation construction
                pub fn signing_service(&self) -> &'static str {
                    ${sigV4Trait.name.dq()}
                }
                """,
                *codegenScope,
            )
            if (serviceHasEventStream) {
                rustTemplate(
                    "#{signerFn:W}",
                    "signerFn" to
                        renderEventStreamSignerFn { propertiesName ->
                            writable {
                                rustTemplate(
                                    """
                                    #{SigV4Signer}::new($propertiesName)
                                    """,
                                    *codegenScope,
                                )
                            }
                        },
                )
            }
        }
    }
}

fun needsAmzSha256(service: ServiceShape) = when (service.id) {
    ShapeId.from("com.amazonaws.s3#AmazonS3") -> true
    ShapeId.from("com.amazonaws.s3control#AWSS3ControlServiceV20180820") -> true
    else -> false
}

fun disableDoubleEncode(service: ServiceShape) = when (service.id) {
    ShapeId.from("com.amazonaws.s3#AmazonS3") -> true
    else -> false
}

fun disableUriPathNormalization(service: ServiceShape) = when (service.id) {
    ShapeId.from("com.amazonaws.s3#AmazonS3") -> true
    else -> false
}

class SigV4SigningFeature(
    private val model: Model,
    private val operation: OperationShape,
    runtimeConfig: RuntimeConfig,
    private val service: ServiceShape,
) :
    OperationCustomization() {
    private val codegenScope = arrayOf(
        "sig_auth" to AwsRuntimeType.awsSigAuth(runtimeConfig),
        "aws_types" to AwsRuntimeType.awsTypes(runtimeConfig),
    )

    private val serviceIndex = ServiceIndex.of(model)

    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                rustTemplate(
                    "let mut signing_config = #{sig_auth}::signer::OperationSigningConfig::default_config();",
                    *codegenScope,
                )
                if (needsAmzSha256(service)) {
                    rust("signing_config.signing_options.content_sha256_header = true;")
                }
                if (disableDoubleEncode(service)) {
                    rust("signing_config.signing_options.double_uri_encode = false;")
                }
                if (disableUriPathNormalization(service)) {
                    rust("signing_config.signing_options.normalize_uri_path = false;")
                }
                if (operation.hasTrait<UnsignedPayloadTrait>()) {
                    rust("signing_config.signing_options.content_sha256_header = true;")
                    rustTemplate(
                        "${section.request}.properties_mut().insert(#{sig_auth}::signer::SignableBody::UnsignedPayload);",
                        *codegenScope,
                    )
                } else if (operation.isInputEventStream(model)) {
                    // TODO(EventStream): Is this actually correct for all Event Stream operations?
                    rustTemplate(
                        "${section.request}.properties_mut().insert(#{sig_auth}::signer::SignableBody::Bytes(&[]));",
                        *codegenScope,
                    )
                }
                // some operations are either unsigned or optionally signed:
                val authSchemes = serviceIndex.getEffectiveAuthSchemes(service, operation)
                if (!authSchemes.containsKey(SigV4Trait.ID)) {
                    rustTemplate(
                        "signing_config.signing_requirements = #{sig_auth}::signer::SigningRequirements::Disabled;",
                        *codegenScope,
                    )
                } else {
                    if (operation.hasTrait<OptionalAuthTrait>()) {
                        rustTemplate(
                            "signing_config.signing_requirements = #{sig_auth}::signer::SigningRequirements::Optional;",
                            *codegenScope,
                        )
                    }
                }
                rustTemplate(
                    """
                    ${section.request}.properties_mut().insert(signing_config);
                    ${section.request}.properties_mut().insert(#{aws_types}::SigningService::from_static(${section.config}.signing_service()));
                    if let Some(region) = &${section.config}.region {
                        ${section.request}.properties_mut().insert(#{aws_types}::region::SigningRegion::from(region.clone()));
                    }
                    """,
                    *codegenScope,
                )
            }
            else -> emptySection
        }
    }
}

fun RuntimeConfig.sigAuth() = awsRuntimeCrate("aws-sig-auth")
