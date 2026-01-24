/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

class InterceptorConfigCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val moduleUseName = codegenContext.moduleUseName()
    private val runtimeConfig = codegenContext.runtimeConfig

    private val codegenScope =
        arrayOf(
            "Intercept" to configReexport(RuntimeType.intercept(runtimeConfig)),
            "SharedInterceptor" to configReexport(RuntimeType.sharedInterceptor(runtimeConfig)),
            // TODO(Http1x): Update this dependency to Http1x
            "Http" to CargoDependency.Http0x.toType(),
        )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.ConfigImpl ->
                    rustTemplate(
                        """
                        /// Returns interceptors currently registered by the user.
                        pub fn interceptors(&self) -> impl Iterator<Item = #{SharedInterceptor}> + '_ {
                            self.runtime_components.interceptors()
                        }
                        """,
                        *codegenScope,
                    )

                ServiceConfig.BuilderImpl ->
                    rustTemplate(
                        """
                        /// Add an [interceptor](#{Intercept}) that runs at specific stages of the request execution pipeline.
                        ///
                        /// Interceptors targeted at a certain stage are executed according to the pre-defined priority.
                        /// The SDK provides a default set of interceptors. An interceptor configured by this method
                        /// will run after those default interceptors.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// ## fn example() {
                        /// use aws_smithy_runtime_api::box_error::BoxError;
                        /// use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut;
                        /// use aws_smithy_runtime_api::client::interceptors::Intercept;
                        /// use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
                        /// use aws_smithy_types::config_bag::ConfigBag;
                        /// use $moduleUseName::config::Config;
                        /// use #{Http}::uri::Uri;
                        ///
                        /// fn base_url() -> String {
                        ///     // ...
                        ///     ## String::new()
                        /// }
                        ///
                        /// ##[derive(Debug)]
                        /// pub struct UriModifierInterceptor;
                        /// impl Intercept for UriModifierInterceptor {
                        ///     fn name(&self) -> &'static str {
                        ///         "UriModifierInterceptor"
                        ///     }
                        ///     fn modify_before_signing(
                        ///         &self,
                        ///         context: &mut BeforeTransmitInterceptorContextMut<'_>,
                        ///         _runtime_components: &RuntimeComponents,
                        ///         _cfg: &mut ConfigBag,
                        ///     ) -> Result<(), BoxError> {
                        ///         let request = context.request_mut();
                        ///         let uri = format!("{}{}", base_url(), request.uri());
                        ///         *request.uri_mut() = uri.parse::<Uri>()?.into();
                        ///
                        ///         Ok(())
                        ///     }
                        /// }
                        ///
                        /// let config = Config::builder()
                        ///     .interceptor(UriModifierInterceptor)
                        ///     .build();
                        /// ## }
                        /// ```
                        pub fn interceptor(mut self, interceptor: impl #{Intercept} + 'static) -> Self {
                            self.push_interceptor(#{SharedInterceptor}::new(interceptor));
                            self
                        }

                        /// Like [`Self::interceptor`], but takes a [`SharedInterceptor`](#{SharedInterceptor}).
                        pub fn push_interceptor(&mut self, interceptor: #{SharedInterceptor}) -> &mut Self {
                            self.runtime_components.push_interceptor(interceptor);
                            self
                        }

                        /// Set [`SharedInterceptor`](#{SharedInterceptor})s for the builder.
                        pub fn set_interceptors(&mut self, interceptors: impl IntoIterator<Item = #{SharedInterceptor}>) -> &mut Self {
                            self.runtime_components.set_interceptors(interceptors.into_iter());
                            self
                        }
                        """,
                        *codegenScope,
                    )

                else -> emptySection
            }
        }
}
