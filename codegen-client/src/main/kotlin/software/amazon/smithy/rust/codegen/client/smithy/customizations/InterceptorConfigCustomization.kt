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
                is ServiceConfig.ConfigStruct -> rustTemplate(
                    """
                    // TODO(enableNewSmithyRuntime): Unused until we completely switch to the orchestrator
                    ##[allow(dead_code)]
                    pub(crate) interceptors: Vec<#{SharedInterceptor}>,
                    """,
                    *codegenScope,
                )

                is ServiceConfig.BuilderStruct ->
                    rustTemplate(
                        """
                        interceptors: Vec<#{SharedInterceptor}>,
                        """,
                        *codegenScope,
                    )

                is ServiceConfig.ConfigImpl -> writable {
                    rustTemplate(
                        """
                        /// Returns interceptors currently registered by the user
                        pub fn interceptors(&self) -> impl Iterator<Item = &#{SharedInterceptor}> + '_ {
                            self.interceptors.iter()
                        }
                        """,
                        *codegenScope,
                    )
                }

                ServiceConfig.BuilderImpl ->
                    rustTemplate(
                        """
                        // TODO(enableNewSmithyRuntime): Remove this #[doc(hidden)] upon launch
                        ##[doc(hidden)]
                        /// Sets an [`Interceptor`](#{Interceptor}) that runs at specific stages of the request execution pipeline.
                        ///
                        /// Interceptors targeted at a certain stage are executed according to the pre-defined priority.
                        /// SDK provides the default set of interceptors. An interceptor configured by this method
                        /// will run after those default interceptors.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// ## ##[cfg(test)]
                        /// ## mod tests {
                        /// ## ##[test]
                        /// ## fn example() {
                        /// use aws_http::user_agent::AwsUserAgent;
                        /// use aws_smithy_runtime_api::client::interceptors::{Interceptor, InterceptorContext};
                        /// use aws_smithy_runtime_api::config_bag::ConfigBag;
                        /// use http::header::USER_AGENT;
                        /// use http::{HeaderName, HeaderValue};
                        /// use $moduleUseName::config::Config;
                        ///
                        /// ##[derive(Debug)]
                        /// struct TestUserAgentInterceptor;
                        /// impl Interceptor for TestUserAgentInterceptor {
                        ///     fn modify_before_signing(
                        ///         &self,
                        ///         context: &mut InterceptorContext,
                        ///         _cfg: &mut ConfigBag,
                        ///     ) -> Result<(), aws_smithy_runtime_api::client::interceptors::BoxError> {
                        ///         let headers = context.request_mut()?.headers_mut();
                        ///         let user_agent = AwsUserAgent::for_tests();
                        ///         headers.insert(USER_AGENT, HeaderValue::try_from(user_agent.ua_header())?);
                        ///         headers.insert(
                        ///             HeaderName::from_static("x-amz-user-agent"),
                        ///             HeaderValue::try_from(user_agent.aws_ua_header())?,
                        ///         );
                        ///
                        ///         Ok(())
                        ///     }
                        /// }
                        ///
                        /// let config = Config::builder()
                        ///     .interceptor(TestUserAgentInterceptor)
                        ///     .build();
                        /// ## }
                        /// ## }
                        /// ```
                        pub fn interceptor(mut self, interceptor: impl #{Interceptor} + Send + Sync + 'static) -> Self {
                            self.set_interceptor(#{SharedInterceptor}::new(interceptor));
                            self
                        }

                        // TODO(enableNewSmithyRuntime): Remove this #[doc(hidden)] upon launch
                        ##[doc(hidden)]
                        /// Sets an [`Interceptor`](#{Interceptor}) that runs at specific stages of the request execution pipeline.
                        ///
                        /// Interceptors targeted at a certain stage are executed according to the pre-defined priority.
                        /// SDK provides the default set of interceptors. An interceptor configured by this method
                        /// will run after those default interceptors.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// ## ##[cfg(test)]
                        /// ## mod tests {
                        /// ## ##[test]
                        /// ## fn example() {
                        /// use aws_http::user_agent::AwsUserAgent;
                        /// use aws_smithy_runtime_api::client::interceptors::{Interceptor, InterceptorContext, SharedInterceptor};
                        /// use aws_smithy_runtime_api::config_bag::ConfigBag;
                        /// use http::header::USER_AGENT;
                        /// use http::{HeaderName, HeaderValue};
                        /// use $moduleUseName::config::{Builder, Config};
                        ///
                        /// fn override_user_agent(builder: &mut Builder) {
                        ///     ##[derive(Debug)]
                        ///     pub struct TestUserAgentInterceptor;
                        ///     impl Interceptor for TestUserAgentInterceptor {
                        ///         fn modify_before_signing(
                        ///             &self,
                        ///             context: &mut InterceptorContext,
                        ///             _cfg: &mut ConfigBag,
                        ///         ) -> Result<(), aws_smithy_runtime_api::client::interceptors::BoxError> {
                        ///             let headers = context.request_mut()?.headers_mut();
                        ///             let user_agent = AwsUserAgent::for_tests();
                        ///             headers.insert(USER_AGENT, HeaderValue::try_from(user_agent.ua_header())?);
                        ///             headers.insert(
                        ///                 HeaderName::from_static("x-amz-user-agent"),
                        ///                 HeaderValue::try_from(user_agent.aws_ua_header())?,
                        ///             );
                        ///
                        ///             Ok(())
                        ///         }
                        ///     }
                        ///     builder.set_interceptor(SharedInterceptor::new(TestUserAgentInterceptor));
                        /// }
                        ///
                        /// let mut builder = Config::builder();
                        /// override_user_agent(&mut builder);
                        /// let config = builder.build();
                        /// ## }
                        /// ## }
                        /// ```
                        pub fn set_interceptor(&mut self, interceptor: #{SharedInterceptor}) -> &mut Self {
                            self.interceptors.push(interceptor);
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
