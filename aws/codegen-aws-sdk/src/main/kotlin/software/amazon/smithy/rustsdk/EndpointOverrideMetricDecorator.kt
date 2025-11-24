/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

/**
 * Decorator that tracks endpoint override business metric when endpoint URL is configured.
 */
class EndpointOverrideMetricDecorator : ClientCodegenDecorator {
    override val name: String = "EndpointOverrideMetric"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        // Generate the interceptor in the config::endpoint module
        rustCrate.withModule(ClientRustModule.Config.endpoint) {
            val runtimeConfig = codegenContext.runtimeConfig
            val smithyRuntimeApi = RuntimeType.smithyRuntimeApiClient(runtimeConfig)
            val smithyTypes = RuntimeType.smithyTypes(runtimeConfig)
            val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)
            val awsTypes = AwsRuntimeType.awsTypes(runtimeConfig)

            rustTemplate(
                """
                /// Interceptor that tracks endpoint override business metric.
                ##[derive(Debug, Default)]
                pub(crate) struct EndpointOverrideFeatureTrackerInterceptor;

                impl #{Intercept} for EndpointOverrideFeatureTrackerInterceptor {
                    fn name(&self) -> &'static str {
                        "EndpointOverrideFeatureTrackerInterceptor"
                    }

                    fn read_before_execution(
                        &self,
                        _context: &#{BeforeSerializationInterceptorContextRef}<'_>,
                        cfg: &mut #{ConfigBag},
                    ) -> #{Result}<(), #{BoxError}> {
                        if cfg.load::<#{EndpointUrl}>().is_some() {
                            cfg.interceptor_state()
                                .store_append(#{AwsSdkFeature}::EndpointOverride);
                        }
                        #{Ok}(())
                    }
                }
                """,
                "Intercept" to smithyRuntimeApi.resolve("client::interceptors::Intercept"),
                "BeforeSerializationInterceptorContextRef" to
                    smithyRuntimeApi.resolve("client::interceptors::context::BeforeSerializationInterceptorContextRef"),
                "ConfigBag" to smithyTypes.resolve("config_bag::ConfigBag"),
                "BoxError" to smithyRuntimeApi.resolve("box_error::BoxError"),
                "EndpointUrl" to awsTypes.resolve("endpoint_config::EndpointUrl"),
                "AwsSdkFeature" to awsRuntime.resolve("sdk_feature::AwsSdkFeature"),
                *preludeScope,
            )
        }
    }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        baseCustomizations + listOf(EndpointOverrideFeatureTrackerRegistration(codegenContext))
}

private class EndpointOverrideFeatureTrackerRegistration(
    private val codegenContext: ClientCodegenContext,
) : ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection) =
        writable {
            if (section is ServiceRuntimePluginSection.RegisterRuntimeComponents) {
                section.registerInterceptor(this) {
                    rustTemplate("crate::config::endpoint::EndpointOverrideFeatureTrackerInterceptor")
                }
            }
        }
}
