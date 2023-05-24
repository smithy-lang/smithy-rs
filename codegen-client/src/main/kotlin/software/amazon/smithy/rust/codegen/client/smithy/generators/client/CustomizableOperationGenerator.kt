/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.GenericTypeArg
import software.amazon.smithy.rust.codegen.core.rustlang.RustGenerics
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.util.outputShape

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
                writeCustomizations(customizations, CustomizableOperationSection.CustomizableOperationImpl(null))
            },
        )
    }

    fun renderForOrchestrator(writer: RustWriter, operation: OperationShape) {
        val symbolProvider = codegenContext.symbolProvider
        val model = codegenContext.model

        val builderName = operation.fluentBuilderType(symbolProvider).name
        val outputType = symbolProvider.toSymbol(operation.outputShape(model))
        val errorType = symbolProvider.symbolForOperationError(operation)

        val codegenScope = arrayOf(
            *preludeScope,
            "HttpResponse" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                .resolve("client::orchestrator::HttpResponse"),
            "Interceptor" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                .resolve("client::interceptors::Interceptor"),
            "MapRequestInterceptor" to RuntimeType.smithyRuntime(runtimeConfig)
                .resolve("client::interceptors::MapRequestInterceptor"),
            "MutateRequestInterceptor" to RuntimeType.smithyRuntime(runtimeConfig)
                .resolve("client::interceptors::MutateRequestInterceptor"),
            "OperationError" to errorType,
            "OperationOutput" to outputType,
            "RuntimePlugin" to RuntimeType.runtimePlugin(runtimeConfig),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "SdkError" to RuntimeType.sdkError(runtimeConfig),
            "SharedInterceptor" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                .resolve("client::interceptors::SharedInterceptor"),
        )

        writer.rustTemplate(
            """
            /// A wrapper type for [`$builderName`]($builderName) that allows for configuring a single
            /// operation invocation.
            pub struct CustomizableOperation {
                pub(crate) fluent_builder: $builderName,
                pub(crate) config_override: #{Option}<crate::config::Builder>,
                pub(crate) interceptors: Vec<#{SharedInterceptor}>,
            }

            impl CustomizableOperation {
                /// Adds an [`Interceptor`](#{Interceptor}) that runs at specific stages of the request execution pipeline.
                ///
                /// Note that interceptors can also be added to `CustomizableOperation` by `config_override`,
                /// `map_request`, and `mutate_request` (the last two are implemented via interceptors under the hood).
                /// The order in which those user-specified operation interceptors are invoked should not be relied upon
                /// as it is an implementation detail.
                pub fn interceptor(mut self, interceptor: impl #{Interceptor} + #{Send} + #{Sync} + 'static) -> Self {
                    self.interceptors.push(#{SharedInterceptor}::new(interceptor));
                    self
                }

                /// Allows for customizing the operation's request.
                pub fn map_request<F, E>(mut self, f: F) -> Self
                where
                    F: #{Fn}(&mut http::Request<#{SdkBody}>) -> #{Result}<(), E>
                        + #{Send}
                        + #{Sync}
                        + 'static,
                    E: ::std::error::Error + #{Send} + #{Sync} + 'static,
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
                    self
                ) -> #{Result}<
                    #{OperationOutput},
                    #{SdkError}<
                        #{OperationError},
                        #{HttpResponse}
                    >
                > {
                    self.send_orchestrator_with_plugin(#{Option}::<#{Box}<dyn #{RuntimePlugin} + #{Send} + #{Sync}>>::None)
                        .await
                }

                ##[doc(hidden)]
                // TODO(enableNewSmithyRuntime): Delete when unused
                /// Equivalent to [`Self::send`] but adds a final runtime plugin to shim missing behavior
                pub async fn send_orchestrator_with_plugin(
                    self,
                    final_plugin: #{Option}<impl #{RuntimePlugin} + #{Send} + #{Sync} + 'static>
                ) -> #{Result}<#{OperationOutput}, #{SdkError}<#{OperationError}, #{HttpResponse}>> {
                    let mut config_override = if let Some(config_override) = self.config_override {
                        config_override
                    } else {
                        crate::config::Builder::new()
                    };

                    self.interceptors.into_iter().for_each(|interceptor| {
                        config_override.add_interceptor(interceptor);
                    });

                    self.fluent_builder
                        .config_override(config_override)
                        .send_orchestrator_with_plugin(final_plugin)
                        .await
                }

                #{additional_methods}
            }
            """,
            *codegenScope,
            "additional_methods" to writable {
                writeCustomizations(customizations, CustomizableOperationSection.CustomizableOperationImpl(operation))
            },
        )
    }
}

fun renderCustomizableOperationSend(runtimeConfig: RuntimeConfig, generics: FluentClientGenerics, writer: RustWriter) {
    val smithyHttp = CargoDependency.smithyHttp(runtimeConfig).toType()
    val smithyClient = CargoDependency.smithyClient(runtimeConfig).toType()

    val operationGenerics = RustGenerics(GenericTypeArg("O"), GenericTypeArg("Retry"))
    val handleGenerics = generics.toRustGenerics()
    val combinedGenerics = operationGenerics + handleGenerics

    val codegenScope = arrayOf(
        *preludeScope,
        "combined_generics_decl" to combinedGenerics.declaration(),
        "handle_generics_bounds" to handleGenerics.bounds(),
        "ParseHttpResponse" to smithyHttp.resolve("response::ParseHttpResponse"),
        "NewRequestPolicy" to smithyClient.resolve("retry::NewRequestPolicy"),
        "SmithyRetryPolicy" to smithyClient.resolve("bounds::SmithyRetryPolicy"),
        "ClassifyRetry" to RuntimeType.classifyRetry(runtimeConfig),
        "SdkSuccess" to RuntimeType.sdkSuccess(runtimeConfig),
        "SdkError" to RuntimeType.sdkError(runtimeConfig),
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
    } else {
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
