/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.CustomizableOperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.CustomizableOperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

class CustomizableOperationTestHelpers(runtimeConfig: RuntimeConfig) :
    CustomizableOperationCustomization() {
    private val codegenScope = arrayOf(
        *RuntimeType.preludeScope,
        "AwsUserAgent" to AwsRuntimeType.awsHttp(runtimeConfig).resolve("user_agent::AwsUserAgent"),
        "BeforeTransmitInterceptorContextMut" to RuntimeType.beforeTransmitInterceptorContextMut(runtimeConfig),
        "ConfigBag" to RuntimeType.configBag(runtimeConfig),
        "http" to CargoDependency.Http.toType(),
        "InterceptorContext" to RuntimeType.interceptorContext(runtimeConfig),
        "RuntimeComponentsBuilder" to RuntimeType.runtimeComponentsBuilder(runtimeConfig),
        "SharedInterceptor" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::interceptors::SharedInterceptor"),
        "SharedTimeSource" to CargoDependency.smithyAsync(runtimeConfig).toType().resolve("time::SharedTimeSource"),
        "StaticRuntimePlugin" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::runtime_plugin::StaticRuntimePlugin"),
        "StaticTimeSource" to CargoDependency.smithyAsync(runtimeConfig).toType().resolve("time::StaticTimeSource"),
        "TestParamsSetterInterceptor" to testParamsSetterInterceptor(),
    )

    // TODO(enableNewSmithyRuntimeCleanup): Delete this once test helpers on `CustomizableOperation` have been removed
    private fun testParamsSetterInterceptor(): RuntimeType = RuntimeType.forInlineFun("TestParamsSetterInterceptor", ClientRustModule.Client.customize) {
        rustTemplate(
            """
            mod test_params_setter_interceptor {
                use aws_smithy_runtime_api::box_error::BoxError;
                use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut;
                use aws_smithy_runtime_api::client::interceptors::Interceptor;
                use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
                use aws_smithy_types::config_bag::ConfigBag;
                use std::fmt;

                pub(super) struct TestParamsSetterInterceptor<F> { f: F }

                impl<F> fmt::Debug for TestParamsSetterInterceptor<F> {
                    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        write!(f, "TestParamsSetterInterceptor")
                    }
                }

                impl<F> TestParamsSetterInterceptor<F> {
                    pub fn new(f: F) -> Self { Self { f } }
                }

                impl<F> Interceptor for TestParamsSetterInterceptor<F>
                where
                    F: Fn(&mut BeforeTransmitInterceptorContextMut<'_>, &mut ConfigBag) + Send + Sync + 'static,
                {
                    fn name(&self) -> &'static str {
                        "TestParamsSetterInterceptor"
                    }

                    fn modify_before_signing(
                        &self,
                        context: &mut BeforeTransmitInterceptorContextMut<'_>,
                        _runtime_components: &RuntimeComponents,
                        cfg: &mut ConfigBag,
                    ) -> Result<(), BoxError> {
                        (self.f)(context, cfg);
                        Ok(())
                    }
                }
            }
            use test_params_setter_interceptor::TestParamsSetterInterceptor;
            """,
            *codegenScope,
        )
    }

    override fun section(section: CustomizableOperationSection): Writable =
        writable {
            if (section is CustomizableOperationSection.CustomizableOperationImpl) {
                if (section.isRuntimeModeOrchestrator) {
                    // TODO(enableNewSmithyRuntimeCleanup): Delete these utilities
                    rustTemplate(
                        """
                        ##[doc(hidden)]
                        // This is a temporary method for testing. NEVER use it in production
                        pub fn request_time_for_tests(self, request_time: ::std::time::SystemTime) -> Self {
                            self.runtime_plugin(
                                #{StaticRuntimePlugin}::new()
                                    .with_runtime_components(
                                        #{RuntimeComponentsBuilder}::new("request_time_for_tests")
                                            .with_time_source(Some(#{SharedTimeSource}::new(#{StaticTimeSource}::new(request_time))))
                                    )
                            )
                        }

                        ##[doc(hidden)]
                        // This is a temporary method for testing. NEVER use it in production
                        pub fn user_agent_for_tests(mut self) -> Self {
                            let interceptor = #{TestParamsSetterInterceptor}::new(|context: &mut #{BeforeTransmitInterceptorContextMut}<'_>, _: &mut #{ConfigBag}| {
                                let headers = context.request_mut().headers_mut();
                                let user_agent = #{AwsUserAgent}::for_tests();
                                headers.insert(
                                    #{http}::header::USER_AGENT,
                                    #{http}::HeaderValue::try_from(user_agent.ua_header()).unwrap(),
                                );
                                headers.insert(
                                    #{http}::HeaderName::from_static("x-amz-user-agent"),
                                    #{http}::HeaderValue::try_from(user_agent.aws_ua_header()).unwrap(),
                                );
                            });
                            self.interceptors.push(#{SharedInterceptor}::new(interceptor));
                            self
                        }

                        ##[doc(hidden)]
                        // This is a temporary method for testing. NEVER use it in production
                        pub fn remove_invocation_id_for_tests(mut self) -> Self {
                            let interceptor = #{TestParamsSetterInterceptor}::new(|context: &mut #{BeforeTransmitInterceptorContextMut}<'_>, _: &mut #{ConfigBag}| {
                                context.request_mut().headers_mut().remove("amz-sdk-invocation-id");
                            });
                            self.interceptors.push(#{SharedInterceptor}::new(interceptor));
                            self
                        }
                        """,
                        *codegenScope,
                    )
                } else {
                    // TODO(enableNewSmithyRuntimeCleanup): Delete this branch when middleware is no longer used
                    rustTemplate(
                        """
                        ##[doc(hidden)]
                        // This is a temporary method for testing. NEVER use it in production
                        pub fn request_time_for_tests(mut self, request_time: ::std::time::SystemTime) -> Self {
                            self.operation.properties_mut().insert(
                                #{SharedTimeSource}::new(#{StaticTimeSource}::new(request_time))
                            );
                            self
                        }

                        ##[doc(hidden)]
                        // This is a temporary method for testing. NEVER use it in production
                        pub fn user_agent_for_tests(mut self) -> Self {
                            self.operation.properties_mut().insert(#{AwsUserAgent}::for_tests());
                            self
                        }

                        ##[doc(hidden)]
                        // This is a temporary method for testing. NEVER use it in production
                        pub fn remove_invocation_id_for_tests(self) -> Self {
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }
            }
        }
}
