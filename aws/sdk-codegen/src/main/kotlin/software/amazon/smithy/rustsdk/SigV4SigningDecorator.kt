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
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.hasEventStreamOperations
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.isInputEventStream

/**
 * The SigV4SigningDecorator:
 * - adds a `signing_service()` method to `config` to return the default signing service
 * - adds a `new_event_stream_signer()` method to `config` to create an Event Stream SigV4 signer
 * - sets the `SigningService` during operation construction
 * - sets a default `OperationSigningConfig` A future enhancement will customize this for specific services that need
 *   different behavior.
 */
class SigV4SigningDecorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "SigV4Signing"
    override val order: Byte = 0

    private fun applies(coreCodegenContext: CoreCodegenContext): Boolean = coreCodegenContext.serviceShape.hasTrait<SigV4Trait>()

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations.letIf(applies(codegenContext)) { customizations ->
            customizations + SigV4SigningConfig(
                codegenContext.runtimeConfig,
                codegenContext.serviceShape.hasEventStreamOperations(codegenContext.model),
                codegenContext.serviceShape.expectTrait()
            )
        }
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
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
}

class SigV4SigningConfig(
    runtimeConfig: RuntimeConfig,
    private val serviceHasEventStream: Boolean,
    private val sigV4Trait: SigV4Trait
) : ConfigCustomization() {
    private val codegenScope = arrayOf(
        "SigV4Signer" to RuntimeType(
            "SigV4Signer",
            runtimeConfig.awsRuntimeDependency("aws-sig-auth", setOf("sign-eventstream")),
            "aws_sig_auth::event_stream"
        ),
        "SharedPropertyBag" to RuntimeType(
            "SharedPropertyBag",
            CargoDependency.SmithyHttp(runtimeConfig),
            "aws_smithy_http::property_bag"
        )
    )

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigImpl -> writable {
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
                    *codegenScope
                )
                if (serviceHasEventStream) {
                    rustTemplate(
                        """
                        /// Creates a new Event Stream `SignMessage` implementor.
                        pub fn new_event_stream_signer(
                            &self,
                            properties: #{SharedPropertyBag}
                        ) -> #{SigV4Signer} {
                            #{SigV4Signer}::new(properties)
                        }
                        """,
                        *codegenScope
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

class SigV4SigningFeature(
    private val model: Model,
    private val operation: OperationShape,
    runtimeConfig: RuntimeConfig,
    private val service: ServiceShape,
) :
    OperationCustomization() {
    private val codegenScope =
        arrayOf("sig_auth" to runtimeConfig.sigAuth().asType(), "aws_types" to awsTypes(runtimeConfig).asType())

    private val serviceIndex = ServiceIndex.of(model)

    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                rustTemplate(
                    "let mut signing_config = #{sig_auth}::signer::OperationSigningConfig::default_config();",
                    *codegenScope
                )
                if (needsAmzSha256(service)) {
                    rust("signing_config.signing_options.content_sha256_header = true;")
                }
                if (disableDoubleEncode(service)) {
                    rust("signing_config.signing_options.double_uri_encode = false;")
                }
                if (operation.hasTrait<UnsignedPayloadTrait>()) {
                    rust("signing_config.signing_options.content_sha256_header = true;")
                    rustTemplate(
                        "${section.request}.properties_mut().insert(#{sig_auth}::signer::SignableBody::UnsignedPayload);",
                        *codegenScope
                    )
                } else if (operation.isInputEventStream(model)) {
                    // TODO(EventStream): Is this actually correct for all Event Stream operations?
                    rustTemplate(
                        "${section.request}.properties_mut().insert(#{sig_auth}::signer::SignableBody::Bytes(&[]));",
                        *codegenScope
                    )
                }
                // some operations are either unsigned or optionally signed:
                val authSchemes = serviceIndex.getEffectiveAuthSchemes(service, operation)
                if (!authSchemes.containsKey(SigV4Trait.ID)) {
                    rustTemplate("signing_config.signing_requirements = #{sig_auth}::signer::SigningRequirements::Disabled;", *codegenScope)
                } else {
                    if (operation.hasTrait<OptionalAuthTrait>()) {
                        rustTemplate("signing_config.signing_requirements = #{sig_auth}::signer::SigningRequirements::Optional;", *codegenScope)
                    }
                }
                rustTemplate(
                    """
                    ${section.request}.properties_mut().insert(signing_config);
                    ${section.request}.properties_mut().insert(#{aws_types}::SigningService::from_static(${section.config}.signing_service()));
                    """,
                    *codegenScope
                )
            }
            else -> emptySection
        }
    }
}

fun RuntimeConfig.sigAuth() = awsRuntimeDependency("aws-sig-auth")
