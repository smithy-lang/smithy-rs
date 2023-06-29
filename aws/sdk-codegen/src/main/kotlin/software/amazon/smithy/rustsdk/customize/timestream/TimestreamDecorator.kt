/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.timestream

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.toType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rustsdk.AwsCargoDependency
import software.amazon.smithy.rustsdk.DocSection
import software.amazon.smithy.rustsdk.InlineAwsDependency

/**
 * This decorator does two things:
 * 1. Adds the `endpoint_discovery` inlineable
 * 2. Adds a `enable_endpoint_discovery` method on client that returns a wrapped client with endpoint discovery enabled
 */
class TimestreamDecorator : ClientCodegenDecorator {
    override val name: String = "Timestream"
    override val order: Byte = -1

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        return listOf(
            adhocCustomization<DocSection.CreateClient> {
                addDependency(AwsCargoDependency.awsConfig(codegenContext.runtimeConfig).toDevDependency())
                rustTemplate(
                    """
                    let config = aws_config::load_from_env().await;
                    // You MUST call `enable_endpoint_discovery` to produce a working client for this service.
                    let ${it.clientName} = ${it.crateName}::Client::new(&config).enable_endpoint_discovery().await;
                    """.replaceIndent(it.indent),
                )
            },
        )
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val endpointDiscovery = InlineAwsDependency.forRustFile(
            "endpoint_discovery",
            Visibility.PUBLIC,
            CargoDependency.Tokio.copy(scope = DependencyScope.Compile, features = setOf("sync")),
        )
        val runtimeMode = codegenContext.smithyRuntimeMode
        val smithyTypes = RuntimeType.smithyTypes(codegenContext.runtimeConfig)
        val runtimeApi = RuntimeType.smithyRuntimeApi(codegenContext.runtimeConfig)
        rustCrate.lib {
            // helper function to resolve an endpoint given a base client
            rustTemplate(
                """
                async fn resolve_endpoint(client: &crate::Client) -> Result<(#{Endpoint}, #{SystemTime}), #{ResolveEndpointError}> {
                    let describe_endpoints =
                        client.describe_endpoints().send().await.map_err(|e| {
                            #{ResolveEndpointError}::from_source("failed to call describe_endpoints", e)
                        })?;
                    let endpoint = describe_endpoints.endpoints().unwrap().get(0).unwrap();
                    let expiry = client.conf().time_source().now() + #{Duration}::from_secs(endpoint.cache_period_in_minutes() as u64 * 60);
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
                    pub async fn enable_endpoint_discovery(self) -> #{Result}<(Self, #{endpoint_discovery}::ReloadEndpoint), #{ResolveEndpointError}> {
                        let new_conf = self.conf().clone();
                        let sleep = self.conf().sleep_impl().expect("sleep impl must be provided");
                        let time = self.conf().time_source();
                        let (resolver, reloader) = #{endpoint_discovery}::create_cache(
                            move || {
                                let client = self.clone();
                                async move { resolve_endpoint(&client).await }
                            },
                            sleep,
                            time
                        )
                        .await?;
                        ##[derive(Debug)]
                        struct OverrideEndpointResolver { layer: #{FrozenLayer} }
                        let mut layer = #{Layer}::new("endpoint-discovery");
                        use #{ConfigBagAccessors};
                        let endpoint_resolver = #{DynEndpointResolver}::new(
                            #{DefaultEndpointResolver}::<#{Params}>::new(
                                #{SharedEndpointResolver}::new(resolver)
                            )
                        );
                        layer.set_endpoint_resolver(endpoint_resolver);
                        let layer = layer.freeze();
                        impl #{RuntimePlugin} for OverrideEndpointResolver {
                            fn config(&self) -> Option<#{FrozenLayer}> {
                                Some(self.layer.clone())
                            }
                        }
                        let new_conf = new_conf.with_runtime_plugin(OverrideEndpointResolver { layer });
                        Ok((Self::from_conf(new_conf), reloader))
                    }
                }
                """,
                "endpoint_discovery" to endpointDiscovery.toType(),
                "SystemTime" to RuntimeType.std.resolve("time::SystemTime"),
                "Duration" to RuntimeType.std.resolve("time::Duration"),
                "ConfigBagAccessors" to RuntimeType.configBagAccessors(codegenContext.runtimeConfig),
                "DefaultEndpointResolver" to RuntimeType.smithyRuntime(codegenContext.runtimeConfig)
                    .resolve("client::orchestrator::endpoints::DefaultEndpointResolver"),
                "DynEndpointResolver" to RuntimeType.smithyRuntimeApi(codegenContext.runtimeConfig)
                    .resolve("client::orchestrator::DynEndpointResolver"),
                "SharedEndpointResolver" to RuntimeType.smithyHttp(codegenContext.runtimeConfig)
                    .resolve("endpoint::SharedEndpointResolver"),
                "Layer" to smithyTypes.resolve("config_bag::Layer"),
                "FrozenLayer" to smithyTypes.resolve("config_bag::FrozenLayer"),
                "RuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::RuntimePlugin"),
                *Types(codegenContext.runtimeConfig).toArray(),
                "Params" to EndpointTypesGenerator.fromContext(codegenContext).paramsStruct(),
                *preludeScope,
            )
        }
    }
}
