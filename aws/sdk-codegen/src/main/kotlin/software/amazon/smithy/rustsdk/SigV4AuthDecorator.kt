/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.aws.traits.auth.UnsignedPayloadTrait
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.OptionalAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isInputEventStream
import software.amazon.smithy.rust.codegen.core.util.letIf

class SigV4AuthDecorator : ClientCodegenDecorator {
    override val name: String get() = "SigV4AuthDecorator"
    override val order: Byte = 0

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations.letIf(codegenContext.settings.codegenConfig.enableNewSmithyRuntime) {
            it + listOf(AuthServiceRuntimePluginCustomization(codegenContext))
        }

    override fun operationRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationRuntimePluginCustomization>,
    ): List<OperationRuntimePluginCustomization> =
        baseCustomizations.letIf(codegenContext.settings.codegenConfig.enableNewSmithyRuntime) {
            it + listOf(AuthOperationRuntimePluginCustomization(codegenContext))
        }
}

private class AuthServiceRuntimePluginCustomization(codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope by lazy {
        val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)
        arrayOf(
            "SIGV4_SCHEME_ID" to awsRuntime.resolve("auth::sigv4::SCHEME_ID"),
            "SigV4HttpAuthScheme" to awsRuntime.resolve("auth::sigv4::SigV4HttpAuthScheme"),
            "SigningRegion" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("region::SigningRegion"),
            "SigningService" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("SigningService"),
        )
    }

    override fun section(section: ServiceRuntimePluginSection): Writable = writable {
        when (section) {
            is ServiceRuntimePluginSection.HttpAuthScheme -> {
                rustTemplate(
                    """
                    .auth_scheme(#{SIGV4_SCHEME_ID}, #{SigV4HttpAuthScheme}::new())
                    """,
                    *codegenScope,
                )
            }

            is ServiceRuntimePluginSection.AdditionalConfig -> {
                section.putConfigValue(this) {
                    rustTemplate("#{SigningService}::from_static(self.handle.conf.signing_service())", *codegenScope)
                }
                rustTemplate(
                    """
                    if let Some(region) = self.handle.conf.region() {
                        #{put_signing_region}
                    }
                    """,
                    *codegenScope,
                    "put_signing_region" to writable {
                        section.putConfigValue(this) {
                            rustTemplate("#{SigningRegion}::from(region.clone())", *codegenScope)
                        }
                    },
                )
            }

            else -> {}
        }
    }
}

private class AuthOperationRuntimePluginCustomization(private val codegenContext: ClientCodegenContext) :
    OperationRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope by lazy {
        val runtimeApi = RuntimeType.smithyRuntimeApi(runtimeConfig)
        val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)
        arrayOf(
            "AuthOptionListResolver" to runtimeApi.resolve("client::auth::option_resolver::AuthOptionListResolver"),
            "HttpAuthOption" to runtimeApi.resolve("client::orchestrator::HttpAuthOption"),
            "HttpSignatureType" to awsRuntime.resolve("auth::sigv4::HttpSignatureType"),
            "PropertyBag" to RuntimeType.smithyHttp(runtimeConfig).resolve("property_bag::PropertyBag"),
            "SIGV4_SCHEME_ID" to awsRuntime.resolve("auth::sigv4::SCHEME_ID"),
            "SigV4OperationSigningConfig" to awsRuntime.resolve("auth::sigv4::SigV4OperationSigningConfig"),
            "SigningOptions" to awsRuntime.resolve("auth::sigv4::SigningOptions"),
            "SigningRegion" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("region::SigningRegion"),
            "SigningService" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("SigningService"),
            "SignableBody" to AwsRuntimeType.awsSigv4(runtimeConfig).resolve("http_request::SignableBody"),
        )
    }
    private val serviceIndex = ServiceIndex.of(codegenContext.model)

    override fun section(section: OperationRuntimePluginSection): Writable = writable {
        when (section) {
            is OperationRuntimePluginSection.AdditionalConfig -> {
                val authSchemes = serviceIndex.getEffectiveAuthSchemes(codegenContext.serviceShape, section.operationShape)
                if (authSchemes.containsKey(SigV4Trait.ID)) {
                    val unsignedPayload = section.operationShape.hasTrait<UnsignedPayloadTrait>()
                    val doubleUriEncode = unsignedPayload || !disableDoubleEncode(codegenContext.serviceShape)
                    val contentSha256Header = needsAmzSha256(codegenContext.serviceShape)
                    val normalizeUrlPath = !disableUriPathNormalization(codegenContext.serviceShape)
                    val signingOptional = section.operationShape.hasTrait<OptionalAuthTrait>()
                    rustTemplate(
                        """
                        let signing_region = cfg.get::<#{SigningRegion}>().expect("region required for signing").clone();
                        let signing_service = cfg.get::<#{SigningService}>().expect("service required for signing").clone();
                        let mut signing_options = #{SigningOptions}::default();
                        signing_options.double_uri_encode = $doubleUriEncode;
                        signing_options.content_sha256_header = $contentSha256Header;
                        signing_options.normalize_uri_path = $normalizeUrlPath;
                        signing_options.signing_optional = $signingOptional;
                        signing_options.payload_override = #{payload_override};

                        let mut sigv4_properties = #{PropertyBag}::new();
                        sigv4_properties.insert(#{SigV4OperationSigningConfig} {
                            region: signing_region,
                            service: signing_service,
                            signing_options,
                        });
                        let auth_option_resolver = #{AuthOptionListResolver}::new(
                            vec![#{HttpAuthOption}::new(#{SIGV4_SCHEME_ID}, std::sync::Arc::new(sigv4_properties))]
                        );
                        ${section.configBagName}.set_auth_option_resolver(auth_option_resolver);
                        """,
                        *codegenScope,
                        "payload_override" to writable {
                            if (unsignedPayload) {
                                rustTemplate("Some(#{SignableBody}::UnsignedPayload)", *codegenScope)
                            } else if (section.operationShape.isInputEventStream(codegenContext.model)) {
                                // TODO(EventStream): Is this actually correct for all Event Stream operations?
                                rustTemplate("Some(#{SignableBody}::Bytes(&[]))", *codegenScope)
                            } else {
                                rust("None")
                            }
                        },
                    )
                }
            }

            else -> {}
        }
    }
}
