/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.timestream

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.toType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rustsdk.InlineAwsDependency

/** Adds Endpoint Discovery Utility to Timestream */
class TimestreamDecorator : ClientCodegenDecorator {
    override val name: String = "Timestream"
    override val order: Byte = 0

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val endpointDiscovery = InlineAwsDependency.forRustFile(
            "endpoint_discovery",
            Visibility.PUBLIC,
            CargoDependency.Tokio.copy(scope = DependencyScope.Compile, features = setOf("sync")),
        )
        rustCrate.lib {
            rustTemplate(
                """
                async fn resolve_endpoint(client: &crate::Client) -> Result<(#{Endpoint}, #{SystemTime}), #{ResolveEndpointError}> {
                    let describe_endpoints =
                        client.describe_endpoints().send().await.map_err(|e| {
                            #{ResolveEndpointError}::from_source("failed to call describe_endpoints", e)
                        })?;
                    let endpoint = describe_endpoints.endpoints().unwrap().get(0).unwrap();
                    let expiry =
                        #{SystemTime}::now() + #{Duration}::from_secs(endpoint.cache_period_in_minutes() as u64 * 60);
                    Ok((
                        #{Endpoint}::builder()
                            .url(format!("https://{}", endpoint.address().unwrap()))
                            .build(),
                        expiry,
                    ))
                }

                impl Client {
                    pub async fn enable_endpoint_discovery(self) -> Result<(Self, #{endpoint_discovery}::ReloadEndpoint), #{ResolveEndpointError}> {
                        let mut new_conf = self.conf().clone();
                        let sleep = self.conf().sleep_impl().expect("sleep impl must be provided");
                        let (resolver, reloader) = #{endpoint_discovery}::create_cache(
                            move || {
                                let client = self.clone();
                                async move { resolve_endpoint(&client).await }
                            },
                            sleep,
                        )
                        .await?;
                        new_conf.endpoint_resolver = std::sync::Arc::new(resolver);
                        Ok((Self::from_conf(new_conf), reloader))
                    }
                }

                """,
                "endpoint_discovery" to endpointDiscovery.toType(),
                "SystemTime" to RuntimeType.std.resolve("time::SystemTime"),
                "Duration" to RuntimeType.std.resolve("time::Duration"),
                *Types(codegenContext.runtimeConfig).toArray(),
            )
        }
    }
}
