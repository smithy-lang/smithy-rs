/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

class MakeConnectorSettingsFromConfigGenerator(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val codegenScope = arrayOf("MakeConnectorSettings" to smithyClientHttpConnector(runtimeConfig).member("MakeConnectorSettings"))
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.TraitImpls -> rustTemplate(
                """
                impl From<&Config> for #{MakeConnectorSettings} {
                    fn from(service_config: &Config) -> #{MakeConnectorSettings} {
                        service_config
                            .timeout_config
                            .as_ref()
                            .map(|timeout_config| {
                                #{MakeConnectorSettings}::new()
                                    .with_http_timeout_config(timeout_config.http_timeouts().clone())
                                    .with_tcp_timeout_config(timeout_config.tcp_timeouts().clone())
                            })
                            .unwrap_or_default()
                    }
                }
                """,
                *codegenScope
            )
            else -> emptySection
        }
    }
}

// Generate path to the http_connector module in aws_smithy_client
fun smithyClientHttpConnector(runtimeConfig: RuntimeConfig) =
    RuntimeType("http_connector", runtimeConfig.runtimeCrate("client"), "aws_smithy_client")
