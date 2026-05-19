/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.dynamodb

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rustsdk.SdkConfigSection

class DynamoDbDecorator : ClientCodegenDecorator {
    override val name: String = "DynamoDb"
    override val order: Byte = 0

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        val rc = codegenContext.runtimeConfig
        return listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rustTemplate(
                    """
                    if ${section.sdkConfig}.retry_config()
                        .and_then(|rc| rc.retry_spec())
                        .is_some_and(|s| s.is_at_least(#{RetrySpec}::V2_1))
                    {
                        let mut rc = #{RetryConfig}::standard()
                            .with_retry_spec(
                                #{RetrySpec}::v2_1()
                                    .with_non_throttling_initial_backoff(#{Duration}::from_millis(25))
                            );
                        if !${section.sdkConfig}.get_origin("retry_config").is_client_config() {
                            rc = rc.with_max_attempts(4);
                        }
                        ${section.serviceConfigBuilder} = ${section.serviceConfigBuilder}.retry_config(rc);
                    }
                    """,
                    "RetryConfig" to CargoDependency.smithyTypes(rc).toType().resolve("retry::RetryConfig"),
                    "RetrySpec" to CargoDependency.smithyTypes(rc).toType().resolve("retry::RetrySpec"),
                    "Duration" to RuntimeType.Duration,
                )
            },
        )
    }
}
