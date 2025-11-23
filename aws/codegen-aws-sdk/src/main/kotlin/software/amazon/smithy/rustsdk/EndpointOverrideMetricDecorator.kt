/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.sdkId

/**
 * Decorator that tracks endpoint override business metric when endpoint URL is configured
 * via any source: global endpoint (AWS_ENDPOINT_URL), service-specific endpoint
 * (AWS_ENDPOINT_URL_<SERVICE>), config file, or SdkConfig builder.
 *
 * Note: Direct Config::builder().endpoint_url() calls are not tracked due to architecture
 * limitations that would cause method name conflicts.
 */
class EndpointOverrideMetricDecorator : ClientCodegenDecorator {
    override val name: String = "EndpointOverrideMetric"
    override val order: Byte = 0

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        val runtimeConfig = codegenContext.runtimeConfig
        val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)
        val serviceId = codegenContext.serviceShape.sdkId()

        return listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rustTemplate(
                    """
                    // Track endpoint override metric if endpoint URL is configured from any source
                    let has_global_endpoint = ${section.sdkConfig}.endpoint_url().is_some();
                    let has_service_specific_endpoint = ${section.sdkConfig}
                        .service_config()
                        .and_then(|conf| conf.load_config(service_config_key(${serviceId.dq()}, ${"AWS_ENDPOINT_URL".dq()}, ${"endpoint_url".dq()})))
                        .is_some();
                    
                    if has_global_endpoint || has_service_specific_endpoint {
                        ${section.serviceConfigBuilder}.push_runtime_plugin(
                            #{SharedRuntimePlugin}::new(#{EndpointOverrideRuntimePlugin}::new_with_feature_flag())
                        );
                    }
                    """,
                    "EndpointOverrideRuntimePlugin" to awsRuntime.resolve("endpoint_override::EndpointOverrideRuntimePlugin"),
                    "SharedRuntimePlugin" to RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::runtime_plugin::SharedRuntimePlugin"),
                )
            },
        )
    }
}
