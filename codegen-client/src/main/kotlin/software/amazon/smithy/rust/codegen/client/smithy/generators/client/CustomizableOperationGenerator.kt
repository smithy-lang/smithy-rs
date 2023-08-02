/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.GenericTypeArg
import software.amazon.smithy.rust.codegen.core.rustlang.RustGenerics
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
    private val codegenContext: ClientCodegenContext,
    private val generics: FluentClientGenerics,
    private val customizations: List<CustomizableOperationCustomization>,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val smithyHttp = CargoDependency.smithyHttp(runtimeConfig).toType()
    private val smithyTypes = CargoDependency.smithyTypes(runtimeConfig).toType()

    fun render(crate: RustCrate) {
        crate.withModule(ClientRustModule.Client.customize) {
            if (codegenContext.smithyRuntimeMode.generateMiddleware) {
                rustTemplate(
                    """
                    pub use #{Operation};
                    pub use #{Request};
                    pub use #{Response};
                    pub use #{ClassifyRetry};
                    pub use #{RetryKind};
                    """,
                    "Operation" to smithyHttp.resolve("operation::Operation"),
                    "Request" to smithyHttp.resolve("operation::Request"),
                    "Response" to smithyHttp.resolve("operation::Response"),
                    "ClassifyRetry" to RuntimeType.classifyRetry(runtimeConfig),
                    "RetryKind" to smithyTypes.resolve("retry::RetryKind"),
                )
                renderCustomizableOperationModule(this)
            }
        }
    }

    private fun renderCustomizableOperationModule(writer: RustWriter) {
        val operationGenerics = RustGenerics(GenericTypeArg("O"), GenericTypeArg("Retry"))
        val handleGenerics = generics.toRustGenerics()
        val combinedGenerics = operationGenerics + handleGenerics

        val codegenScope = arrayOf(
            *preludeScope,
            "Arc" to RuntimeType.Arc,
            "Infallible" to RuntimeType.stdConvert.resolve("Infallible"),
            // SDK Types
            "HttpRequest" to RuntimeType.HttpRequest,
            "handle_generics_decl" to handleGenerics.declaration(),
            "handle_generics_bounds" to handleGenerics.bounds(),
            "operation_generics_decl" to operationGenerics.declaration(),
            "combined_generics_decl" to combinedGenerics.declaration(),
            "customize_module" to ClientRustModule.Client.customize,
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        )

        writer.rustTemplate(
            """
            /// A wrapper type for [`Operation`](aws_smithy_http::operation::Operation)s that allows for
            /// customization of the operation before it is sent. A `CustomizableOperation` may be sent
            /// by calling its [`.send()`][#{customize_module}::CustomizableOperation::send] method.
            ##[derive(Debug)]
            pub struct CustomizableOperation#{combined_generics_decl:W} {
                pub(crate) handle: #{Arc}<crate::client::Handle#{handle_generics_decl:W}>,
                pub(crate) operation: Operation#{operation_generics_decl:W},
            }

            impl#{combined_generics_decl:W} CustomizableOperation#{combined_generics_decl:W}
            where
                #{handle_generics_bounds:W}
            {
                /// Allows for customizing the operation's request
                pub fn map_request<E>(
                    mut self,
                    f: impl #{FnOnce}(#{HttpRequest}<#{SdkBody}>) -> #{Result}<#{HttpRequest}<#{SdkBody}>, E>,
                ) -> #{Result}<Self, E> {
                    let (request, response) = self.operation.into_request_response();
                    let request = request.augment(|req, _props| f(req))?;
                    self.operation = Operation::from_parts(request, response);
                    #{Ok}(self)
                }

                /// Convenience for `map_request` where infallible direct mutation of request is acceptable
                pub fn mutate_request(self, f: impl #{FnOnce}(&mut #{HttpRequest}<#{SdkBody}>)) -> Self {
                    self.map_request(|mut req| {
                        f(&mut req);
                        #{Result}::<_, #{Infallible}>::Ok(req)
                    })
                    .expect("infallible")
                }

                /// Allows for customizing the entire operation
                pub fn map_operation<E>(
                    mut self,
                    f: impl #{FnOnce}(Operation#{operation_generics_decl:W}) -> #{Result}<Operation#{operation_generics_decl:W}, E>,
                ) -> #{Result}<Self, E> {
                    self.operation = f(self.operation)?;
                    #{Ok}(self)
                }

                /// Direct access to read the HTTP request
                pub fn request(&self) -> &#{HttpRequest}<#{SdkBody}> {
                    self.operation.request()
                }

                /// Direct access to mutate the HTTP request
                pub fn request_mut(&mut self) -> &mut #{HttpRequest}<#{SdkBody}> {
                    self.operation.request_mut()
                }

                #{additional_methods}
            }
            """,
            *codegenScope,
            "additional_methods" to writable {
                writeCustomizations(customizations, CustomizableOperationSection.CustomizableOperationImpl(false))
            },
        )
    }

    fun renderForOrchestrator(crate: RustCrate) {
        val codegenScope = arrayOf(
            *preludeScope,
            "CustomizableOperation" to ClientRustModule.Client.customize.toType()
                .resolve("orchestrator::CustomizableOperation"),
            "CustomizableSend" to ClientRustModule.Client.customize.toType()
                .resolve("internal::CustomizableSend"),
            "HttpRequest" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                .resolve("client::orchestrator::HttpRequest"),
            "HttpResponse" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                .resolve("client::orchestrator::HttpResponse"),
            "Interceptor" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                .resolve("client::interceptors::Interceptor"),
            "MapRequestInterceptor" to RuntimeType.smithyRuntime(runtimeConfig)
                .resolve("client::interceptors::MapRequestInterceptor"),
            "MutateRequestInterceptor" to RuntimeType.smithyRuntime(runtimeConfig)
                .resolve("client::interceptors::MutateRequestInterceptor"),
            "RuntimePlugin" to RuntimeType.runtimePlugin(runtimeConfig),
            "SharedRuntimePlugin" to RuntimeType.sharedRuntimePlugin(runtimeConfig),
            "SendResult" to ClientRustModule.Client.customize.toType()
                .resolve("internal::SendResult"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "SdkError" to RuntimeType.sdkError(runtimeConfig),
            "SharedInterceptor" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                .resolve("client::interceptors::SharedInterceptor"),
        )

        val customizeModule = ClientRustModule.Client.customize

        crate.withModule(customizeModule) {
            renderConvenienceAliases(customizeModule, this)

            // TODO(enableNewSmithyRuntimeCleanup): Render it directly under the customize module when CustomizableOperation
            //  in the middleware has been removed.
            withInlineModule(
                RustModule.new(
                    "orchestrator",
                    Visibility.PUBLIC,
                    true,
                    customizeModule,
                    documentationOverride = "Module for defining types for `CustomizableOperation` in the orchestrator",
                ),
                null,
            ) {
                rustTemplate(
                    """
                    /// `CustomizableOperation` allows for configuring a single operation invocation before it is sent.
                    pub struct CustomizableOperation<T, E> {
                        pub(crate) customizable_send: #{Box}<dyn #{CustomizableSend}<T, E>>,
                        pub(crate) config_override: #{Option}<crate::config::Builder>,
                        pub(crate) interceptors: Vec<#{SharedInterceptor}>,
                        pub(crate) runtime_plugins: Vec<#{SharedRuntimePlugin}>,
                    }

                    impl<T, E> CustomizableOperation<T, E> {
                        /// Adds an [`Interceptor`](#{Interceptor}) that runs at specific stages of the request execution pipeline.
                        ///
                        /// Note that interceptors can also be added to `CustomizableOperation` by `config_override`,
                        /// `map_request`, and `mutate_request` (the last two are implemented via interceptors under the hood).
                        /// The order in which those user-specified operation interceptors are invoked should not be relied upon
                        /// as it is an implementation detail.
                        pub fn interceptor(mut self, interceptor: impl #{Interceptor} + 'static) -> Self {
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
                            F: #{Fn}(&mut http::Request<#{SdkBody}>) + #{Send} + #{Sync} + 'static,
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
                        /// Config A     overridden by    Config B          ==        Config C
                        /// field_1: None,                field_1: Some(v2),          field_1: Some(v2),
                        /// field_2: Some(v1),            field_2: Some(v2),          field_2: Some(v2),
                        /// field_3: Some(v1),            field_3: None,              field_3: Some(v1),
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
                        {
                            let mut config_override = if let Some(config_override) = self.config_override {
                                config_override
                            } else {
                                crate::config::Builder::new()
                            };

                            self.interceptors.into_iter().for_each(|interceptor| {
                                config_override.push_interceptor(interceptor);
                            });
                            self.runtime_plugins.into_iter().for_each(|plugin| {
                                config_override.push_runtime_plugin(plugin);
                            });

                            (self.customizable_send)(config_override).await
                        }

                        #{additional_methods}
                    }
                    """,
                    *codegenScope,
                    "additional_methods" to writable {
                        writeCustomizations(
                            customizations,
                            CustomizableOperationSection.CustomizableOperationImpl(true),
                        )
                    },
                )
            }
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

                pub trait CustomizableSend<T, E>:
                    #{FnOnce}(crate::config::Builder) -> BoxFuture<SendResult<T, E>>
                {}

                impl<F, T, E> CustomizableSend<T, E> for F
                where
                    F: #{FnOnce}(crate::config::Builder) -> BoxFuture<SendResult<T, E>>
                {}
                """,
                *preludeScope,
                "HttpResponse" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                    .resolve("client::orchestrator::HttpResponse"),
                "SdkError" to RuntimeType.sdkError(runtimeConfig),
            )
        }
    }
}

fun renderCustomizableOperationSend(codegenContext: ClientCodegenContext, generics: FluentClientGenerics, writer: RustWriter) {
    val runtimeConfig = codegenContext.runtimeConfig
    val smithyHttp = CargoDependency.smithyHttp(runtimeConfig).toType()
    val smithyClient = CargoDependency.smithyClient(runtimeConfig).toType()

    val operationGenerics = RustGenerics(GenericTypeArg("O"), GenericTypeArg("Retry"))
    val handleGenerics = generics.toRustGenerics()
    val combinedGenerics = operationGenerics + handleGenerics

    val codegenScope = arrayOf(
        *preludeScope,
        "SdkSuccess" to RuntimeType.sdkSuccess(runtimeConfig),
        "SdkError" to RuntimeType.sdkError(runtimeConfig),
        // TODO(enableNewSmithyRuntimeCleanup): Delete the trait bounds when cleaning up middleware
        "ParseHttpResponse" to smithyHttp.resolve("response::ParseHttpResponse"),
        "NewRequestPolicy" to smithyClient.resolve("retry::NewRequestPolicy"),
        "SmithyRetryPolicy" to smithyClient.resolve("bounds::SmithyRetryPolicy"),
        "ClassifyRetry" to RuntimeType.classifyRetry(runtimeConfig),
        // TODO(enableNewSmithyRuntimeCleanup): Delete the generics when cleaning up middleware
        "combined_generics_decl" to combinedGenerics.declaration(),
        "handle_generics_bounds" to handleGenerics.bounds(),
    )

    if (generics is FlexibleClientGenerics) {
        writer.rustTemplate(
            """
            impl#{combined_generics_decl:W} CustomizableOperation#{combined_generics_decl:W}
            where
                #{handle_generics_bounds:W}
            {
                /// Sends this operation's request
                pub async fn send<T, E>(self) -> #{Result}<T, #{SdkError}<E>>
                where
                    E: std::error::Error + #{Send} + #{Sync} + 'static,
                    O: #{ParseHttpResponse}<Output = #{Result}<T, E>> + #{Send} + #{Sync} + #{Clone} + 'static,
                    Retry: #{Send} + #{Sync} + #{Clone},
                    Retry: #{ClassifyRetry}<#{SdkSuccess}<T>, #{SdkError}<E>> + #{Send} + #{Sync} + #{Clone},
                    <R as #{NewRequestPolicy}>::Policy: #{SmithyRetryPolicy}<O, T, E, Retry> + #{Clone},
                {
                    self.handle.client.call(self.operation).await
                }
            }
            """,
            *codegenScope,
        )
    } else if (codegenContext.smithyRuntimeMode.generateMiddleware) {
        writer.rustTemplate(
            """
            impl#{combined_generics_decl:W} CustomizableOperation#{combined_generics_decl:W}
            where
                #{handle_generics_bounds:W}
            {
                /// Sends this operation's request
                pub async fn send<T, E>(self) -> #{Result}<T, #{SdkError}<E>>
                where
                    E: std::error::Error + #{Send} + #{Sync} + 'static,
                    O: #{ParseHttpResponse}<Output = #{Result}<T, E>> + #{Send} + #{Sync} + #{Clone} + 'static,
                    Retry: #{Send} + #{Sync} + #{Clone},
                    Retry: #{ClassifyRetry}<#{SdkSuccess}<T>, #{SdkError}<E>> + #{Send} + #{Sync} + #{Clone},
                {
                    self.handle.client.call(self.operation).await
                }
            }
            """,
            *codegenScope,
        )
    }
}
