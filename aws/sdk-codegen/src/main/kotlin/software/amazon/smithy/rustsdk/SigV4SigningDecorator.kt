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
import software.amazon.smithy.rust.codegen.client.smithy.SmithyRuntimeMode
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.extendIf
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamOperations
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isInputEventStream

// TODO(enableNewSmithyRuntimeCleanup): Remove this decorator (superseded by SigV4AuthDecorator)
/**
 * The SigV4SigningDecorator:
 * - adds a `signing_service()` method to `config` to return the default signing service
 * - adds a `new_event_stream_signer()` method to `config` to create an Event Stream SigV4 signer
 * - sets the `SigningService` during operation construction
 * - sets a default `OperationSigningConfig` A future enhancement will customize this for specific services that need
 *   different behavior.
 */
class SigV4SigningDecorator : ClientCodegenDecorator {
    override val name: String = "SigV4Signing"
    override val order: Byte = 0

    private fun applies(codegenContext: CodegenContext): Boolean = codegenContext.serviceShape.hasTrait<SigV4Trait>()

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations.extendIf(applies(codegenContext)) {
            SigV4SigningConfig(
                codegenContext.runtimeConfig,
                codegenContext.smithyRuntimeMode,
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
        return baseCustomizations.extendIf(applies(codegenContext)) {
            SigV4SigningFeature(
                codegenContext.model,
                operation,
                codegenContext.runtimeConfig,
                codegenContext.serviceShape,
            )
        }
    }
}

class SigV4SigningConfig(
    private val runtimeConfig: RuntimeConfig,
    private val runtimeMode: SmithyRuntimeMode,
    private val serviceHasEventStream: Boolean,
    private val sigV4Trait: SigV4Trait,
) : ConfigCustomization() {
    private val codegenScope = arrayOf(
        "Region" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("region::Region"),
        "SigningService" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("SigningService"),
        "SigningRegion" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("region::SigningRegion"),
    )

    override fun section(section: ServiceConfig): Writable = writable {
        when (section) {
            ServiceConfig.ConfigImpl -> {
                if (runtimeMode.generateMiddleware && serviceHasEventStream) {
                    // enable the aws-sig-auth `sign-eventstream` feature
                    addDependency(AwsRuntimeType.awsSigAuthEventStream(runtimeConfig).toSymbol())
                }
                rust(
                    """
                    /// The signature version 4 service signing name to use in the credential scope when signing requests.
                    ///
                    /// The signing service may be overridden by the `Endpoint`, or by specifying a custom
                    /// [`SigningService`](aws_types::SigningService) during operation construction
                    pub fn signing_service(&self) -> &'static str {
                        ${sigV4Trait.name.dq()}
                    }
                    """,
                )
            }
            ServiceConfig.BuilderBuild -> {
                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        layer.store_put(#{SigningService}::from_static(${sigV4Trait.name.dq()}));
                        layer.load::<#{Region}>().cloned().map(|r| layer.store_put(#{SigningRegion}::from(r)));
                        """,
                        *codegenScope,
                    )
                }
            }

            else -> emptySection
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
