/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.GenericTypeArg
import software.amazon.smithy.rust.codegen.core.rustlang.RustGenerics
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

/**
 * Generates the code required to add the `.customize()` function to the
 * fluent client builders.
 */
class CustomizableOperationGenerator(
    private val runtimeConfig: RuntimeConfig,
    private val generics: FluentClientGenerics,
    private val includeFluentClient: Boolean,
) {

    companion object {
        val CustomizeModule = RustModule.public("customize", "Operation customization and supporting types", parent = RustModule.operation(Visibility.PUBLIC))
    }

    private val smithyHttp = CargoDependency.smithyHttp(runtimeConfig).toType()
    private val smithyTypes = CargoDependency.smithyTypes(runtimeConfig).toType()

    fun render(crate: RustCrate) {
        crate.withModule(CustomizeModule) {
            rustTemplate(
                """
                pub use #{Operation};
                pub use #{ClassifyRetry};
                pub use #{RetryKind};
                """,
                "Operation" to smithyHttp.resolve("operation::Operation"),
                "ClassifyRetry" to smithyHttp.resolve("retry::ClassifyRetry"),
                "RetryKind" to smithyTypes.resolve("retry::RetryKind"),
            )
            renderCustomizableOperationModule(this)

            if (includeFluentClient) {
                renderCustomizableOperationSend(this)
            }
        }
    }

    private fun renderCustomizableOperationModule(writer: RustWriter) {
        val operationGenerics = RustGenerics(GenericTypeArg("O"), GenericTypeArg("Retry"))
        val handleGenerics = generics.toRustGenerics()
        val combinedGenerics = operationGenerics + handleGenerics

        val codegenScope = arrayOf(
            // SDK Types
            "http_result" to smithyHttp.resolve("result"),
            "http_body" to smithyHttp.resolve("body"),
            "HttpRequest" to RuntimeType.HttpRequest,
            "handle_generics_decl" to handleGenerics.declaration(),
            "handle_generics_bounds" to handleGenerics.bounds(),
            "operation_generics_decl" to operationGenerics.declaration(),
            "combined_generics_decl" to combinedGenerics.declaration(),
        )

        writer.rustTemplate(
            """
            use crate::client::Handle;

            use #{http_body}::SdkBody;
            use #{http_result}::SdkError;

            use std::convert::Infallible;
            use std::sync::Arc;

            /// A wrapper type for [`Operation`](aws_smithy_http::operation::Operation)s that allows for
            /// customization of the operation before it is sent. A `CustomizableOperation` may be sent
            /// by calling its [`.send()`][crate::operation::customize::CustomizableOperation::send] method.
            ##[derive(Debug)]
            pub struct CustomizableOperation#{combined_generics_decl:W} {
                pub(crate) handle: Arc<Handle#{handle_generics_decl:W}>,
                pub(crate) operation: Operation#{operation_generics_decl:W},
            }

            impl#{combined_generics_decl:W} CustomizableOperation#{combined_generics_decl:W}
            where
                #{handle_generics_bounds:W}
            {
                /// Allows for customizing the operation's request
                pub fn map_request<E>(
                    mut self,
                    f: impl FnOnce(#{HttpRequest}<SdkBody>) -> Result<#{HttpRequest}<SdkBody>, E>,
                ) -> Result<Self, E> {
                    let (request, response) = self.operation.into_request_response();
                    let request = request.augment(|req, _props| f(req))?;
                    self.operation = Operation::from_parts(request, response);
                    Ok(self)
                }

                /// Convenience for `map_request` where infallible direct mutation of request is acceptable
                pub fn mutate_request(self, f: impl FnOnce(&mut #{HttpRequest}<SdkBody>)) -> Self {
                    self.map_request(|mut req| {
                        f(&mut req);
                        Result::<_, Infallible>::Ok(req)
                    })
                    .expect("infallible")
                }

                /// Allows for customizing the entire operation
                pub fn map_operation<E>(
                    mut self,
                    f: impl FnOnce(Operation#{operation_generics_decl:W}) -> Result<Operation#{operation_generics_decl:W}, E>,
                ) -> Result<Self, E> {
                    self.operation = f(self.operation)?;
                    Ok(self)
                }

                /// Direct access to read the HTTP request
                pub fn request(&self) -> &#{HttpRequest}<SdkBody> {
                    self.operation.request()
                }

                /// Direct access to mutate the HTTP request
                pub fn request_mut(&mut self) -> &mut #{HttpRequest}<SdkBody> {
                    self.operation.request_mut()
                }
            }
            """,
            *codegenScope,
        )
    }

    private fun renderCustomizableOperationSend(writer: RustWriter) {
        val smithyHttp = CargoDependency.smithyHttp(runtimeConfig).toType()
        val smithyClient = CargoDependency.smithyClient(runtimeConfig).toType()

        val operationGenerics = RustGenerics(GenericTypeArg("O"), GenericTypeArg("Retry"))
        val handleGenerics = generics.toRustGenerics()
        val combinedGenerics = operationGenerics + handleGenerics

        val codegenScope = arrayOf(
            "combined_generics_decl" to combinedGenerics.declaration(),
            "handle_generics_bounds" to handleGenerics.bounds(),
            "ParseHttpResponse" to smithyHttp.resolve("response::ParseHttpResponse"),
            "NewRequestPolicy" to smithyClient.resolve("retry::NewRequestPolicy"),
            "SmithyRetryPolicy" to smithyClient.resolve("bounds::SmithyRetryPolicy"),
        )

        writer.rustTemplate(
            """
            impl#{combined_generics_decl:W} CustomizableOperation#{combined_generics_decl:W}
            where
                #{handle_generics_bounds:W}
            {
                /// Sends this operation's request
                pub async fn send<T, E>(self) -> Result<T, SdkError<E>>
                where
                    E: std::error::Error + Send + Sync + 'static,
                    O: #{ParseHttpResponse}<Output = Result<T, E>> + Send + Sync + Clone + 'static,
                    Retry: Send + Sync + Clone,
                    <R as #{NewRequestPolicy}>::Policy: #{SmithyRetryPolicy}<O, T, E, Retry> + Clone,
                {
                    self.handle.client.call(self.operation).await
                }
            }
            """,
            *codegenScope,
        )
    }
}
