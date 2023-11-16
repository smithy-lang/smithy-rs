/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

class ClientRuntimeTypesReExportGenerator(
    private val codegenContext: ClientCodegenContext,
    private val rustCrate: RustCrate,
) {
    fun render() {
        val rc = codegenContext.runtimeConfig
        val smithyRuntimeApi = RuntimeType.smithyRuntimeApi(rc)

        rustCrate.withModule(ClientRustModule.config) {
            rustTemplate(
                """
                pub use #{ConfigBag};
                pub use #{Intercept};
                pub use #{RuntimeComponents};
                pub use #{SharedInterceptor};
                pub use #{IdentityCache};
                """,
                "ConfigBag" to RuntimeType.configBag(rc),
                "Intercept" to RuntimeType.intercept(rc),
                "RuntimeComponents" to RuntimeType.runtimeComponents(rc),
                "SharedInterceptor" to RuntimeType.sharedInterceptor(rc),
                "IdentityCache" to RuntimeType.smithyRuntime(rc).resolve("client::identity::IdentityCache"),
            )

            if (codegenContext.enableUserConfigurableRuntimePlugins) {
                rustTemplate(
                    """
                    pub use #{runtime_plugin}::{RuntimePlugin, SharedRuntimePlugin};
                    pub use #{config_bag}::FrozenLayer;
                    pub use #{RuntimeComponentsBuilder};
                    """,
                    "runtime_plugin" to RuntimeType.smithyRuntimeApi(rc).resolve("client::runtime_plugin"),
                    "config_bag" to RuntimeType.smithyTypes(rc).resolve("config_bag"),
                    "RuntimeComponentsBuilder" to RuntimeType.runtimeComponentsBuilder(rc),
                )
            }
        }
        rustCrate.withModule(ClientRustModule.Config.endpoint) {
            rustTemplate(
                """
                pub use #{SharedEndpointResolver};
                pub use #{EndpointFuture};
                pub use #{Endpoint};
                """,
                *Types(rc).toArray(),
            )
        }
        rustCrate.withModule(ClientRustModule.Config.retry) {
            rustTemplate(
                """
                pub use #{ClassifyRetry};
                pub use #{RetryAction};
                pub use #{ShouldAttempt};
                """,
                "ClassifyRetry" to smithyRuntimeApi.resolve("client::retries::classifiers::ClassifyRetry"),
                "RetryAction" to smithyRuntimeApi.resolve("client::retries::classifiers::RetryAction"),
                "ShouldAttempt" to smithyRuntimeApi.resolve("client::retries::ShouldAttempt"),
            )
        }
        rustCrate.withModule(ClientRustModule.Config.interceptors) {
            rustTemplate(
                """
                pub use #{AfterDeserializationInterceptorContextRef};
                pub use #{BeforeDeserializationInterceptorContextMut};
                pub use #{BeforeDeserializationInterceptorContextRef};
                pub use #{BeforeSerializationInterceptorContextMut};
                pub use #{BeforeSerializationInterceptorContextRef};
                pub use #{BeforeTransmitInterceptorContextMut};
                pub use #{BeforeTransmitInterceptorContextRef};
                pub use #{FinalizerInterceptorContextMut};
                pub use #{FinalizerInterceptorContextRef};
                pub use #{InterceptorContext};
                """,
                "AfterDeserializationInterceptorContextRef" to RuntimeType.afterDeserializationInterceptorContextRef(rc),
                "BeforeDeserializationInterceptorContextMut" to RuntimeType.beforeDeserializationInterceptorContextMut(rc),
                "BeforeDeserializationInterceptorContextRef" to RuntimeType.beforeDeserializationInterceptorContextRef(rc),
                "BeforeSerializationInterceptorContextMut" to RuntimeType.beforeSerializationInterceptorContextMut(rc),
                "BeforeSerializationInterceptorContextRef" to RuntimeType.beforeSerializationInterceptorContextRef(rc),
                "BeforeTransmitInterceptorContextMut" to RuntimeType.beforeTransmitInterceptorContextMut(rc),
                "BeforeTransmitInterceptorContextRef" to RuntimeType.beforeTransmitInterceptorContextRef(rc),
                "FinalizerInterceptorContextMut" to RuntimeType.finalizerInterceptorContextMut(rc),
                "FinalizerInterceptorContextRef" to RuntimeType.finalizerInterceptorContextRef(rc),
                "InterceptorContext" to RuntimeType.interceptorContext(rc),
            )
        }
        rustCrate.withModule(ClientRustModule.Error) {
            rustTemplate(
                "pub use #{BoxError};",
                "BoxError" to RuntimeType.boxError(rc),
            )
        }
    }
}
