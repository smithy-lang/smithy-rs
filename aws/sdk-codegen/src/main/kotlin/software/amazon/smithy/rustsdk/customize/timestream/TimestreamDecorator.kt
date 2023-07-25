/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.timestream

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.toType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rustsdk.AwsCargoDependency
import software.amazon.smithy.rustsdk.DocSection
import software.amazon.smithy.rustsdk.InlineAwsDependency

/**
 * This decorator does two things:
 * 1. Adds the `endpoint_discovery` inlineable
 * 2. Adds a `with_endpoint_discovery_enabled` method on client that returns a wrapped client with endpoint discovery enabled
 */
class TimestreamDecorator : ClientCodegenDecorator {
    override val name: String = "Timestream"
    override val order: Byte = -1

    private fun applies(codegenContext: ClientCodegenContext): Boolean =
        codegenContext.smithyRuntimeMode.generateOrchestrator

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        emptyList<AdHocCustomization>().letIf(applies(codegenContext)) {
            listOf(
                adhocCustomization<DocSection.CreateClient> {
                    addDependency(AwsCargoDependency.awsConfig(codegenContext.runtimeConfig).toDevDependency())
                    rustTemplate(
                        """
                        let config = aws_config::load_from_env().await;
                        // You MUST call `with_endpoint_discovery_enabled` to produce a working client for this service.
                        let ${it.clientName} = ${it.crateName}::Client::new(&config).with_endpoint_discovery_enabled().await;
                        """.replaceIndent(it.indent),
                    )
                },
            )
        }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        if (!applies(codegenContext)) {
            return
        }

        val endpointDiscovery = InlineAwsDependency.forRustFile(
            "endpoint_discovery",
            Visibility.PUBLIC,
            CargoDependency.Tokio.copy(scope = DependencyScope.Compile, features = setOf("sync")),
            CargoDependency.smithyAsync(codegenContext.runtimeConfig).toDevDependency().withFeature("test-util"),
        )
        rustCrate.withModule(ClientRustModule.client) {
            // helper function to resolve an endpoint given a base client
            rustTemplate(
                """
                async fn resolve_endpoint(client: &crate::Client) -> Result<(#{Endpoint}, #{SystemTime}), #{ResolveEndpointError}> {
                    let describe_endpoints =
                        client.describe_endpoints().send().await.map_err(|e| {
                            #{ResolveEndpointError}::from_source("failed to call describe_endpoints", e)
                        })?;
                    let endpoint = describe_endpoints.endpoints().unwrap().get(0).unwrap();
                    let expiry = client.conf().time_source().expect("checked when ep discovery was enabled").now()
                        + #{Duration}::from_secs(endpoint.cache_period_in_minutes() as u64 * 60);
                    Ok((
                        #{Endpoint}::builder()
                            .url(format!("https://{}", endpoint.address().unwrap()))
                            .build(),
                        expiry,
                    ))
                }

                impl Client {
                    /// Enable endpoint discovery for this client
                    ///
                    /// This method MUST be called to construct a working client.
                    pub async fn with_endpoint_discovery_enabled(self) -> #{Result}<(Self, #{endpoint_discovery}::ReloadEndpoint), #{ResolveEndpointError}> {
                        let handle = self.handle.clone();

                        // The original client without endpoint discover gets moved into the endpoint discovery
                        // resolver since calls to DescribeEndpoint without discovery need to be made.
                        let client_without_discovery = self;
                        let (resolver, reloader) = #{endpoint_discovery}::create_cache(
                            move || {
                                let client = client_without_discovery.clone();
                                async move { resolve_endpoint(&client).await }
                            },
                            handle.conf.sleep_impl()
                                .expect("endpoint discovery requires the client config to have a sleep impl"),
                            handle.conf.time_source()
                                .expect("endpoint discovery requires the client config to have a time source"),
                        ).await?;

                        let client_with_discovery = crate::Client::from_conf(
                            handle.conf.to_builder()
                                    .endpoint_resolver(#{SharedEndpointResolver}::new(resolver))
                                    .build()
                        );
                        Ok((client_with_discovery, reloader))
                    }
                }
                """,
                *RuntimeType.preludeScope,
                "Arc" to RuntimeType.Arc,
                "Duration" to RuntimeType.std.resolve("time::Duration"),
                "SharedEndpointResolver" to RuntimeType.smithyHttp(codegenContext.runtimeConfig)
                    .resolve("endpoint::SharedEndpointResolver"),
                "SystemTime" to RuntimeType.std.resolve("time::SystemTime"),
                "endpoint_discovery" to endpointDiscovery.toType(),
                *Types(codegenContext.runtimeConfig).toArray(),
            )
        }
    }
}
