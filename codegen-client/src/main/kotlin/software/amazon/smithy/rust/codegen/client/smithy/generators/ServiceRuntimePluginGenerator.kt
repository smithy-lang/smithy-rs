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
            "ConfigBag" to RuntimeType.smithyRuntimeApi(rc).resolve("config_bag::ConfigBag"),
            "RuntimePlugin" to RuntimeType.smithyRuntimeApi(rc).resolve("client::runtime_plugin::RuntimePlugin"),
        )
    }

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            pub(crate) struct ServiceRuntimePlugin;

            impl #{RuntimePlugin} for ServiceRuntimePlugin {
                fn configure(&self, _cfg: &mut #{ConfigBag}) -> Result<(), #{BoxError}> {
                    // TODO(RuntimePlugins): Add the AuthOptionResolver to the config bag
                    // TODO(RuntimePlugins): Add the EndpointResolver to the config bag
                    // TODO(RuntimePlugins): Add the IdentityResolver to the config bag
                    // TODO(RuntimePlugins): Add the Connection to the config bag
                    // TODO(RuntimePlugins): Add the HttpAuthSchemes to the config bag
                    // TODO(RuntimePlugins): Add the RetryStrategy to the config bag
                    // TODO(RuntimePlugins): Add the TraceProbe to the config bag
                    Ok(())
                }
            }
            """,
            *codegenScope,
        )
    }
}
