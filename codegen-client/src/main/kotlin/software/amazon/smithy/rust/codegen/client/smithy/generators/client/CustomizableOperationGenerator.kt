/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations

/**
 * Generates the code required to add the `.customize()` function to the
 * fluent client builders.
 */
class CustomizableOperationGenerator(
    codegenContext: ClientCodegenContext,
    private val customizations: List<CustomizableOperationCustomization>,
) {
    private val runtimeConfig = codegenContext.runtimeConfig

    fun render(crate: RustCrate) {
        val codegenScope = arrayOf(
            *preludeScope,
            "CustomizableOperation" to ClientRustModule.Client.customize.toType()
                .resolve("CustomizableOperation"),
            "CustomizableSend" to ClientRustModule.Client.customize.toType()
                .resolve("internal::CustomizableSend"),
            "HttpRequest" to RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::orchestrator::HttpRequest"),
            "HttpResponse" to RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::orchestrator::HttpResponse"),
            "Intercept" to RuntimeType.intercept(runtimeConfig),
            "MapRequestInterceptor" to RuntimeType.smithyRuntime(runtimeConfig)
                .resolve("client::interceptors::MapRequestInterceptor"),
            "MutateRequestInterceptor" to RuntimeType.smithyRuntime(runtimeConfig)
                .resolve("client::interceptors::MutateRequestInterceptor"),
            "PhantomData" to RuntimeType.Phantom,
            "RuntimePlugin" to RuntimeType.runtimePlugin(runtimeConfig),
            "SharedRuntimePlugin" to RuntimeType.sharedRuntimePlugin(runtimeConfig),
            "SendResult" to ClientRustModule.Client.customize.toType()
                .resolve("internal::SendResult"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "SdkError" to RuntimeType.sdkError(runtimeConfig),
            "SharedInterceptor" to RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::interceptors::SharedInterceptor"),
        )

        val customizeModule = ClientRustModule.Client.customize
        crate.withModule(customizeModule) {
            renderConvenienceAliases(customizeModule, this)

            rustTemplate(
                """
                /// `CustomizableOperation` allows for configuring a single operation invocation before it is sent.
                pub struct CustomizableOperation<T, E, B> {
                    customizable_send: B,
                    config_override: #{Option}<crate::config::Builder>,
                    interceptors: Vec<#{SharedInterceptor}>,
                    runtime_plugins: Vec<#{SharedRuntimePlugin}>,
                    _output: #{PhantomData}<T>,
                    _error: #{PhantomData}<E>,
                }

                    impl<T, E, B> CustomizableOperation<T, E, B> {
                        /// Creates a new `CustomizableOperation` from `customizable_send`.
                        ##[allow(dead_code)] // unused when a service does not provide any operations
                        pub(crate) fn new(customizable_send: B) -> Self {
                            Self {
                                customizable_send,
                                config_override: #{None},
                                interceptors: vec![],
                                runtime_plugins: vec![],
                                _output: #{PhantomData},
                                _error: #{PhantomData}
                            }
                        }

                    /// Adds an [interceptor](#{Intercept}) that runs at specific stages of the request execution pipeline.
                    ///
                    /// Note that interceptors can also be added to `CustomizableOperation` by `config_override`,
                    /// `map_request`, and `mutate_request` (the last two are implemented via interceptors under the hood).
                    /// The order in which those user-specified operation interceptors are invoked should not be relied upon
                    /// as it is an implementation detail.
                    pub fn interceptor(mut self, interceptor: impl #{Intercept} + 'static) -> Self {
                        self.interceptors.push(#{SharedInterceptor}::new(interceptor));
                        self
                    }

                    /// Adds a runtime plugin.
                    ##[allow(unused)]
                    pub(crate) fn runtime_plugin(mut self, runtime_plugin: impl #{RuntimePlugin} + 'static) -> Self {
                        self.runtime_plugins.push(#{SharedRuntimePlugin}::new(runtime_plugin));
                        self
                    }

                    /// Allows for customizing the operation's request.
                    pub fn map_request<F, MapE>(mut self, f: F) -> Self
                    where
                        F: #{Fn}(#{HttpRequest}) -> #{Result}<#{HttpRequest}, MapE>
                            + #{Send}
                            + #{Sync}
                            + 'static,
                        MapE: ::std::error::Error + #{Send} + #{Sync} + 'static,
                    {
                        self.interceptors.push(
                            #{SharedInterceptor}::new(
                                #{MapRequestInterceptor}::new(f),
                            ),
                        );
                        self
                    }

                        /// Convenience for `map_request` where infallible direct mutation of request is acceptable.
                        pub fn mutate_request<F>(mut self, f: F) -> Self
                        where
                            F: #{Fn}(&mut #{HttpRequest}) + #{Send} + #{Sync} + 'static,
                        {
                            self.interceptors.push(
                                #{SharedInterceptor}::new(
                                    #{MutateRequestInterceptor}::new(f),
                                ),
                            );
                            self
                        }

                    /// Overrides config for a single operation invocation.
                    ///
                    /// `config_override` is applied to the operation configuration level.
                    /// The fields in the builder that are `Some` override those applied to the service
                    /// configuration level. For instance,
                    ///
                    /// | Config A           | overridden by Config B | = Config C         |
                    /// |--------------------|------------------------|--------------------|
                    /// | field_1: None,     | field_1: Some(v2),     | field_1: Some(v2), |
                    /// | field_2: Some(v1), | field_2: Some(v2),     | field_2: Some(v2), |
                    /// | field_3: Some(v1), | field_3: None,         | field_3: Some(v1), |
                    pub fn config_override(
                        mut self,
                        config_override: impl #{Into}<crate::config::Builder>,
                    ) -> Self {
                        self.config_override = Some(config_override.into());
                        self
                    }

                    /// Sends the request and returns the response.
                    pub async fn send(
                        self,
                    ) -> #{SendResult}<T, E>
                    where
                        E: std::error::Error + #{Send} + #{Sync} + 'static,
                        B: #{CustomizableSend}<T, E>,
                    {
                        let mut config_override = self.config_override.unwrap_or_default();
                        self.interceptors.into_iter().for_each(|interceptor| {
                            config_override.push_interceptor(interceptor);
                        });
                        self.runtime_plugins.into_iter().for_each(|plugin| {
                            config_override.push_runtime_plugin(plugin);
                        });

                        self.customizable_send.send(config_override).await
                    }

                    #{additional_methods}
                }
                """,
                *codegenScope,
                "additional_methods" to writable {
                    writeCustomizations(
                        customizations,
                        CustomizableOperationSection.CustomizableOperationImpl,
                    )
                },
            )
        }
    }

    private fun renderConvenienceAliases(parentModule: RustModule, writer: RustWriter) {
        writer.withInlineModule(RustModule.new("internal", Visibility.PUBCRATE, true, parentModule), null) {
            rustTemplate(
                """
                pub type BoxFuture<T> = ::std::pin::Pin<#{Box}<dyn ::std::future::Future<Output = T> + #{Send}>>;

                pub type SendResult<T, E> = #{Result}<
                    T,
                    #{SdkError}<
                        E,
                        #{HttpResponse},
                    >,
                >;

                pub trait CustomizableSend<T, E>: #{Send} + #{Sync} {
                    // Takes an owned `self` as the implementation will internally call methods that take `self`.
                    // If it took `&self`, that would make this trait object safe, but some implementing types do not
                    // derive `Clone`, unable to yield `self` from `&self`.
                    fn send(self, config_override: crate::config::Builder) -> BoxFuture<SendResult<T, E>>;
                }
                """,
                *preludeScope,
                "HttpResponse" to RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::orchestrator::HttpResponse"),
                "SdkError" to RuntimeType.sdkError(runtimeConfig),
            )
        }
    }
}
