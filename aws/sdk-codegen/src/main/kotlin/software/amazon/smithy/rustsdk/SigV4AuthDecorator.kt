/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.auth.SigV4ATrait
import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.aws.traits.auth.UnsignedPayloadTrait
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.traits.EndpointRuleSetTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ConditionalDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.AuthSchemeLister
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.featureGateBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamOperations
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isInputEventStream
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.thenSingletonListOf
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthSchemeOption as AuthSchemeOptionV2

internal fun ClientCodegenContext.usesSigAuth(): Boolean =
    ServiceIndex.of(model).getEffectiveAuthSchemes(serviceShape).containsKey(SigV4Trait.ID) ||
        usesSigV4a()

/**
 * SigV4a doesn't have a Smithy auth trait yet, so this is a hack to determine if a service supports it.
 *
 * TODO(https://github.com/smithy-lang/smithy-rs/issues/4076): Smithy's `ServiceIndex.getEffectiveAuthSchemes` should be used instead.
 */
internal fun ClientCodegenContext.usesSigV4a(): Boolean {
    val endpointAuthSchemes =
        serviceShape.getTrait<EndpointRuleSetTrait>()?.ruleSet?.let { EndpointRuleSet.fromNode(it) }
            ?.also { it.typeCheck() }?.let { AuthSchemeLister.authSchemesForRuleset(it) } ?: setOf()
    return endpointAuthSchemes.contains("sigv4a")
}

class SigV4AuthDecorator : ConditionalDecorator(
    predicate = { codegenContext, _ -> codegenContext?.usesSigAuth() ?: false },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name: String get() = "SigV4AuthDecorator"
            override val order: Byte = ORDER

            private val sigv4a = "sigv4a"

            private fun sigv4(runtimeConfig: RuntimeConfig) =
                writable {
                    val awsRuntimeAuthModule = AwsRuntimeType.awsRuntime(runtimeConfig).resolve("auth")
                    rust("#T", awsRuntimeAuthModule.resolve("sigv4::SCHEME_ID"))
                }

            private fun sigv4a(runtimeConfig: RuntimeConfig) =
                writable {
                    val awsRuntimeAuthModule = AwsRuntimeType.awsRuntime(runtimeConfig).resolve("auth")
                    featureGateBlock(sigv4a) {
                        rust("#T", awsRuntimeAuthModule.resolve("sigv4a::SCHEME_ID"))
                    }
                }

            override fun authSchemeOptions(
                codegenContext: ClientCodegenContext,
                baseAuthSchemeOptions: List<AuthSchemeOptionV2>,
            ): List<AuthSchemeOptionV2> =
                (baseAuthSchemeOptions + Sigv4AuthSchemeOption())
                    .letIf(codegenContext.usesSigV4a()) {
                        it + Sigv4aAuthSchemeOption()
                    }

            override fun authOptions(
                codegenContext: ClientCodegenContext,
                operationShape: OperationShape,
                baseAuthSchemeOptions: List<AuthSchemeOption>,
            ): List<AuthSchemeOption> {
                val supportsSigV4a =
                    codegenContext.usesSigV4a().thenSingletonListOf { sigv4a(codegenContext.runtimeConfig) }
                return baseAuthSchemeOptions +
                    AuthSchemeOption.StaticAuthSchemeOption(
                        SigV4Trait.ID,
                        listOf(sigv4(codegenContext.runtimeConfig)) + supportsSigV4a,
                    )
            }

            override fun serviceRuntimePluginCustomizations(
                codegenContext: ClientCodegenContext,
                baseCustomizations: List<ServiceRuntimePluginCustomization>,
            ): List<ServiceRuntimePluginCustomization> =
                baseCustomizations + listOf(AuthServiceRuntimePluginCustomization(codegenContext))

            override fun operationCustomizations(
                codegenContext: ClientCodegenContext,
                operation: OperationShape,
                baseCustomizations: List<OperationCustomization>,
            ): List<OperationCustomization> = baseCustomizations + AuthOperationCustomization(codegenContext)

            override fun configCustomizations(
                codegenContext: ClientCodegenContext,
                baseCustomizations: List<ConfigCustomization>,
            ): List<ConfigCustomization> =
                baseCustomizations + SigV4SigningConfig(codegenContext.runtimeConfig, codegenContext.serviceShape.getTrait())

            override fun extras(
                codegenContext: ClientCodegenContext,
                rustCrate: RustCrate,
            ) {
                if (codegenContext.usesSigV4a()) {
                    // Add optional feature for SigV4a support
                    rustCrate.mergeFeature(Feature("sigv4a", true, listOf("aws-runtime/sigv4a")))
                }
            }
        },
) {
    companion object {
        const val ORDER: Byte = 0
    }
}

private class Sigv4AuthSchemeOption : AuthSchemeOptionV2 {
    override val authSchemeId = SigV4Trait.ID

    override fun render(
        codegenContext: ClientCodegenContext,
        operation: OperationShape?,
    ) = renderImpl(
        codegenContext.runtimeConfig,
        AwsRuntimeType.awsRuntime(codegenContext.runtimeConfig)
            .resolve("auth::sigv4::SCHEME_ID"),
        operation,
    )
}

private class Sigv4aAuthSchemeOption : AuthSchemeOptionV2 {
    override val authSchemeId = SigV4ATrait.ID

    override fun render(
        codegenContext: ClientCodegenContext,
        operation: OperationShape?,
    ) = renderImpl(
        codegenContext.runtimeConfig,
        AwsRuntimeType.awsRuntime(codegenContext.runtimeConfig)
            .resolve("auth::sigv4a::SCHEME_ID"),
        operation,
    )
}

private fun renderImpl(
    runtimeConfig: RuntimeConfig,
    schemeId: RuntimeType,
    op: OperationShape?,
) = writable {
    val codegenScope =
        arrayOf(
            "AuthSchemeOption" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::auth::AuthSchemeOption"),
            "Layer" to
                RuntimeType.smithyTypes(runtimeConfig)
                    .resolve("config_bag::Layer"),
            "PayloadSigningOverride" to
                AwsRuntimeType.awsRuntime(runtimeConfig)
                    .resolve("auth::PayloadSigningOverride"),
        )
    rustTemplate(
        """
        #{AuthSchemeOption}::builder()
            .scheme_id(#{schemeId})
            #{properties:W}
            .build()
            .expect("required fields set")
        """,
        *codegenScope,
        "schemeId" to schemeId,
        "properties" to
            writable {
                if (op?.hasTrait<UnsignedPayloadTrait>() == true) {
                    rustTemplate(
                        """
                        .properties({
                            let mut layer = #{Layer}::new("${op.id.name}AuthOptionProperties");
                            layer.store_put(#{PayloadSigningOverride}::unsigned_payload());
                            layer.freeze()
                        })
                        """,
                        *codegenScope,
                    )
                }
            },
    )
}

private class SigV4SigningConfig(
    runtimeConfig: RuntimeConfig,
    private val sigV4Trait: SigV4Trait?,
) : ConfigCustomization() {
    private val codegenScope =
        arrayOf(
            "Region" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("region::Region"),
            "SigningName" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("SigningName"),
            "SigningRegion" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("region::SigningRegion"),
        )

    override fun section(section: ServiceConfig): Writable =
        writable {
            if (sigV4Trait != null) {
                when (section) {
                    ServiceConfig.ConfigImpl -> {
                        rust(
                            """
                            /// The signature version 4 service signing name to use in the credential scope when signing requests.
                            ///
                            /// The signing service may be overridden by the `Endpoint`, or by specifying a custom
                            /// [`SigningName`](aws_types::SigningName) during operation construction
                            pub fn signing_name(&self) -> &'static str {
                                ${sigV4Trait.name.dq()}
                            }
                            """,
                        )
                    }

                    ServiceConfig.BuilderBuild -> {
                        rustTemplate(
                            """
                            layer.store_put(#{SigningName}::from_static(${sigV4Trait.name.dq()}));
                            layer.load::<#{Region}>().cloned().map(|r| layer.store_put(#{SigningRegion}::from(r)));
                            """,
                            *codegenScope,
                        )
                    }

                    is ServiceConfig.OperationConfigOverride -> {
                        rustTemplate(
                            """
                            resolver.config_mut()
                                .load::<#{Region}>()
                                .cloned()
                                .map(|r| resolver.config_mut().store_put(#{SigningRegion}::from(r)));
                            """,
                            *codegenScope,
                        )
                    }

                    else -> {}
                }
            }
        }
}

private class AuthServiceRuntimePluginCustomization(private val codegenContext: ClientCodegenContext) :
    ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope by lazy {
        val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)
        arrayOf(
            "SigV4AuthScheme" to awsRuntime.resolve("auth::sigv4::SigV4AuthScheme"),
            "SigV4aAuthScheme" to awsRuntime.resolve("auth::sigv4a::SigV4aAuthScheme"),
            "SharedAuthScheme" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::auth::SharedAuthScheme"),
        )
    }

    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            when (section) {
                is ServiceRuntimePluginSection.RegisterRuntimeComponents -> {
                    val serviceHasEventStream =
                        codegenContext.serviceShape.hasEventStreamOperations(codegenContext.model)
                    if (serviceHasEventStream) {
                        // enable the aws-runtime `event-stream` feature
                        addDependency(
                            AwsCargoDependency.awsRuntime(runtimeConfig).withFeature("event-stream").toType()
                                .toSymbol(),
                        )
                    }
                    section.registerAuthScheme(this) {
                        rustTemplate("#{SharedAuthScheme}::new(#{SigV4AuthScheme}::new())", *codegenScope)
                    }

                    if (codegenContext.usesSigV4a()) {
                        featureGateBlock("sigv4a") {
                            section.registerAuthScheme(this) {
                                rustTemplate("#{SharedAuthScheme}::new(#{SigV4aAuthScheme}::new())", *codegenScope)
                            }
                        }
                    }
                }

                else -> {}
            }
        }
}

fun needsAmzSha256(service: ServiceShape) =
    when (service.id) {
        ShapeId.from("com.amazonaws.s3#AmazonS3") -> true
        ShapeId.from("com.amazonaws.s3control#AWSS3ControlServiceV20180820") -> true
        else -> false
    }

fun disableDoubleEncode(service: ServiceShape) =
    when (service.id) {
        ShapeId.from("com.amazonaws.s3#AmazonS3") -> true
        else -> false
    }

fun disableUriPathNormalization(service: ServiceShape) =
    when (service.id) {
        ShapeId.from("com.amazonaws.s3#AmazonS3") -> true
        else -> false
    }

private class AuthOperationCustomization(private val codegenContext: ClientCodegenContext) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope by lazy {
        val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)
        arrayOf(
            "SigV4OperationSigningConfig" to awsRuntime.resolve("auth::SigV4OperationSigningConfig"),
            "SigningOptions" to awsRuntime.resolve("auth::SigningOptions"),
            "SignableBody" to AwsRuntimeType.awsSigv4(runtimeConfig).resolve("http_request::SignableBody"),
            "Default" to RuntimeType.Default,
        )
    }
    private val serviceIndex = ServiceIndex.of(codegenContext.model)

    override fun section(section: OperationSection): Writable =
        writable {
            when (section) {
                is OperationSection.AdditionalRuntimePluginConfig -> {
                    val authSchemes =
                        serviceIndex.getEffectiveAuthSchemes(codegenContext.serviceShape, section.operationShape)
                    if (authSchemes.containsKey(SigV4Trait.ID)) {
                        val unsignedPayload = section.operationShape.hasTrait<UnsignedPayloadTrait>()
                        val doubleUriEncode = unsignedPayload || !disableDoubleEncode(codegenContext.serviceShape)
                        val contentSha256Header = needsAmzSha256(codegenContext.serviceShape) || unsignedPayload
                        val normalizeUrlPath = !disableUriPathNormalization(codegenContext.serviceShape)
                        rustTemplate(
                            """
                            let mut signing_options = #{SigningOptions}::default();
                            signing_options.double_uri_encode = $doubleUriEncode;
                            signing_options.content_sha256_header = $contentSha256Header;
                            signing_options.normalize_uri_path = $normalizeUrlPath;
                            signing_options.payload_override = #{payload_override};

                            ${section.newLayerName}.store_put(#{SigV4OperationSigningConfig} {
                                signing_options,
                                ..#{Default}::default()
                            });
                            """,
                            *codegenScope,
                            "payload_override" to
                                writable {
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
