/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

class InterceptorConfigCustomization(codegenContext: CodegenContext) : ConfigCustomization() {
    private val moduleUseName = codegenContext.moduleUseName()
    private val runtimeConfig = codegenContext.runtimeConfig
    private val interceptors = RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::interceptors")
    private val codegenScope = arrayOf(
        "Interceptor" to interceptors.resolve("Interceptor"),
        "SharedInterceptor" to interceptors.resolve("SharedInterceptor"),
    )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.ConfigStruct -> rustTemplate(
                    """
                    // TODO(enableNewSmithyRuntime): Unused until we completely switch to the orchestrator
                    ##[allow(dead_code)]
                    pub(crate) interceptors: Vec<#{SharedInterceptor}>,
                    """,
                    *codegenScope,
                )

                ServiceConfig.BuilderStruct ->
                    rustTemplate(
                        """
                        interceptors: Vec<#{SharedInterceptor}>,
                        """,
                        *codegenScope,
                    )

                ServiceConfig.ConfigImpl -> rustTemplate(
                    """
                    // TODO(enableNewSmithyRuntime): Remove this doc hidden upon launch
                    ##[doc(hidden)]
                    /// Returns interceptors currently registered by the user.
                    pub fn interceptors(&self) -> impl Iterator<Item = &#{SharedInterceptor}> + '_ {
                        self.interceptors.iter()
                    }
                    """,
                    *codegenScope,
                )

                ServiceConfig.BuilderImpl ->
                    rustTemplate(
                        """
                        // TODO(enableNewSmithyRuntime): Remove this doc hidden upon launch
                        ##[doc(hidden)]
                        /// Add an [`Interceptor`](#{Interceptor}) that runs at specific stages of the request execution pipeline.
                        ///
                        /// Interceptors targeted at a certain stage are executed according to the pre-defined priority.
                        /// The SDK provides a default set of interceptors. An interceptor configured by this method
                        /// will run after those default interceptors.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// ## ##[cfg(test)]
                        /// ## mod tests {
                        /// ## ##[test]
                        /// ## fn example() {
                        /// use aws_smithy_runtime_api::client::interceptors::context::phase::BeforeTransmit;
                        /// use aws_smithy_runtime_api::client::interceptors::{Interceptor, InterceptorContext};
                        /// use aws_smithy_runtime_api::config_bag::ConfigBag;
                        /// use $moduleUseName::config::Config;
                        ///
                        /// fn base_url() -> String {
                        ///     // ...
                        ///     ## String::new()
                        /// }
                        ///
                        /// ##[derive(Debug)]
                        /// pub struct UriModifierInterceptor;
                        /// impl Interceptor for UriModifierInterceptor {
                        ///     fn modify_before_signing(
                        ///         &self,
                        ///         context: &mut InterceptorContext<BeforeTransmit>,
                        ///         _cfg: &mut ConfigBag,
                        ///     ) -> Result<(), aws_smithy_runtime_api::client::interceptors::BoxError> {
                        ///         let request = context.request_mut();
                        ///         let uri = format!("{}{}", base_url(), request.uri().path());
                        ///         *request.uri_mut() = uri.parse()?;
                        ///
                        ///         Ok(())
                        ///     }
                        /// }
                        ///
                        /// let config = Config::builder()
                        ///     .interceptor(UriModifierInterceptor)
                        ///     .build();
                        /// ## }
                        /// ## }
                        /// ```
                        pub fn interceptor(mut self, interceptor: impl #{Interceptor} + Send + Sync + 'static) -> Self {
                            self.add_interceptor(#{SharedInterceptor}::new(interceptor));
                            self
                        }

                        // TODO(enableNewSmithyRuntime): Remove this doc hidden upon launch
                        ##[doc(hidden)]
                        /// Add a [`SharedInterceptor`](#{SharedInterceptor}) that runs at specific stages of the request execution pipeline.
                        ///
                        /// Interceptors targeted at a certain stage are executed according to the pre-defined priority.
                        /// The SDK provides a default set of interceptors. An interceptor configured by this method
                        /// will run after those default interceptors.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// ## ##[cfg(test)]
                        /// ## mod tests {
                        /// ## ##[test]
                        /// ## fn example() {
                        /// use aws_smithy_runtime_api::client::interceptors::context::phase::BeforeTransmit;
                        /// use aws_smithy_runtime_api::client::interceptors::{Interceptor, InterceptorContext, SharedInterceptor};
                        /// use aws_smithy_runtime_api::config_bag::ConfigBag;
                        /// use $moduleUseName::config::{Builder, Config};
                        ///
                        /// fn base_url() -> String {
                        ///     // ...
                        ///     ## String::new()
                        /// }
                        ///
                        /// fn modify_request_uri(builder: &mut Builder) {
                        ///     ##[derive(Debug)]
                        ///     pub struct UriModifierInterceptor;
                        ///     impl Interceptor for UriModifierInterceptor {
                        ///         fn modify_before_signing(
                        ///             &self,
                        ///             context: &mut InterceptorContext<BeforeTransmit>,
                        ///             _cfg: &mut ConfigBag,
                        ///         ) -> Result<(), aws_smithy_runtime_api::client::interceptors::BoxError> {
                        ///             let request = context.request_mut();
                        ///             let uri = format!("{}{}", base_url(), request.uri().path());
                        ///             *request.uri_mut() = uri.parse()?;
                        ///
                        ///             Ok(())
                        ///         }
                        ///     }
                        ///     builder.add_interceptor(SharedInterceptor::new(UriModifierInterceptor));
                        /// }
                        ///
                        /// let mut builder = Config::builder();
                        /// modify_request_uri(&mut builder);
                        /// let config = builder.build();
                        /// ## }
                        /// ## }
                        /// ```
                        pub fn add_interceptor(&mut self, interceptor: #{SharedInterceptor}) -> &mut Self {
                            self.interceptors.push(interceptor);
                            self
                        }

                        // TODO(enableNewSmithyRuntime): Remove this doc hidden upon launch
                        ##[doc(hidden)]
                        /// Set [`SharedInterceptor`](#{SharedInterceptor})s for the builder.
                        pub fn set_interceptors(&mut self, interceptors: impl IntoIterator<Item = #{SharedInterceptor}>) -> &mut Self {
                            self.interceptors = interceptors.into_iter().collect();
                            self
                        }
                        """,
                        *codegenScope,
                    )

                ServiceConfig.BuilderBuild -> rust(
                    """
                    interceptors: self.interceptors,
                    """,
                )

                ServiceConfig.ToRuntimePlugin -> rust(
                    """
                    self.interceptors.iter().for_each(|interceptor| {
                        interceptors.register(interceptor.clone());
                    });
                    """,
                )

                else -> emptySection
            }
        }
}
