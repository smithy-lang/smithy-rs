/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Generates the service-level runtime plugin
 */
class ServiceRuntimePluginGenerator(
    codegenContext: ClientCodegenContext,
) {
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        arrayOf(
            "BoxError" to RuntimeType.smithyRuntimeApi(rc).resolve("client::runtime_plugin::BoxError"),
            "NeverRetryStrategy" to RuntimeType.smithyRuntimeApi(rc).resolve("client::retries::NeverRetryStrategy"),
            "ConfigBag" to RuntimeType.smithyRuntimeApi(rc).resolve("config_bag::ConfigBag"),
            "ConfigBagAccessors" to RuntimeType.smithyRuntimeApi(rc).resolve("client::orchestrator::ConfigBagAccessors"),
            "TestConnection" to RuntimeType.smithyRuntimeApi(rc).resolve("client::connections::test_connection::TestConnection"),
            "RuntimePlugin" to RuntimeType.smithyRuntimeApi(rc).resolve("client::runtime_plugin::RuntimePlugin"),
            "StaticUriEndpointResolver" to RuntimeType.smithyRuntimeApi(rc).resolve("client::endpoints::StaticUriEndpointResolver"),
            "StubAuthOptionResolver" to RuntimeType.smithyRuntimeApi(rc).resolve("client::auth::option_resolver::StubAuthOptionResolver"),
            "StubAuthOptionResolverParams" to RuntimeType.smithyRuntimeApi(rc).resolve("client::auth::option_resolver::StubAuthOptionResolverParams"),
            "AuthOptionResolverParams" to RuntimeType.smithyRuntimeApi(rc).resolve("client::orchestrator::AuthOptionResolverParams"),
            "IdentityResolvers" to RuntimeType.smithyRuntimeApi(rc).resolve("client::orchestrator::IdentityResolvers"),
        )
    }

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            pub(crate) struct ServiceRuntimePlugin {
                handle: std::sync::Arc<crate::client::Handle>,
            }

            impl ServiceRuntimePlugin {
                pub fn new(handle: std::sync::Arc<crate::client::Handle>) -> Self { Self { handle } }
            }

            impl #{RuntimePlugin} for ServiceRuntimePlugin {
                fn configure(&self, cfg: &mut #{ConfigBag}) -> Result<(), #{BoxError}> {
                    use #{ConfigBagAccessors};

                    cfg.set_identity_resolvers(#{IdentityResolvers}::builder().build());
                    cfg.set_auth_option_resolver_params(#{AuthOptionResolverParams}::new(#{StubAuthOptionResolverParams}::new()));
                    cfg.set_auth_option_resolver(#{StubAuthOptionResolver}::new());
                    cfg.set_endpoint_resolver(#{StaticUriEndpointResolver}::default());
                    cfg.set_retry_strategy(#{NeverRetryStrategy}::new());
                    cfg.set_connection(#{TestConnection}::new(vec![]));
                    // TODO(RuntimePlugins): Add the HttpAuthSchemes to the config bag
                    // TODO(RuntimePlugins): Add the TraceProbe to the config bag
                    Ok(())
                }
            }
            """,
            *codegenScope,
        )
    }
}
