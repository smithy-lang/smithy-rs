/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

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
        "AwsUserAgent" to AwsRuntimeType.awsHttp(runtimeConfig)
            .resolve("user_agent::AwsUserAgent"),
        "BeforeTransmitInterceptorContextMut" to RuntimeType.smithyRuntimeApi(runtimeConfig)
            .resolve("client::interceptors::BeforeTransmitInterceptorContextMut"),
        "ConfigBag" to RuntimeType.smithyTypes(runtimeConfig).resolve("config_bag::ConfigBag"),
        "ConfigBagAccessors" to RuntimeType.smithyRuntimeApi(runtimeConfig)
            .resolve("client::orchestrator::ConfigBagAccessors"),
        "http" to CargoDependency.Http.toType(),
        "InterceptorContext" to RuntimeType.smithyRuntimeApi(runtimeConfig)
            .resolve("client::interceptors::InterceptorContext"),
        "SharedTimeSource" to CargoDependency.smithyAsync(runtimeConfig).withFeature("test-util").toType()
            .resolve("time::SharedTimeSource"),
        "SharedInterceptor" to RuntimeType.smithyRuntimeApi(runtimeConfig)
            .resolve("client::interceptors::SharedInterceptor"),
        "TestParamsSetterInterceptor" to CargoDependency.smithyRuntime(runtimeConfig).withFeature("test-util")
            .toType().resolve("client::test_util::interceptors::TestParamsSetterInterceptor"),
    )

    override fun section(section: CustomizableOperationSection): Writable =
        writable {
            if (section is CustomizableOperationSection.CustomizableOperationImpl) {
                if (section.isRuntimeModeOrchestrator) {
                    rustTemplate(
                        """
                        ##[doc(hidden)]
                        // This is a temporary method for testing. NEVER use it in production
                        pub fn request_time_for_tests(mut self, request_time: ::std::time::SystemTime) -> Self {
                            use #{ConfigBagAccessors};
                            let interceptor = #{TestParamsSetterInterceptor}::new(move |_: &mut #{BeforeTransmitInterceptorContextMut}<'_>, cfg: &mut #{ConfigBag}| {
                                cfg.interceptor_state().set_request_time(#{SharedTimeSource}::new(request_time));
                            });
                            self.interceptors.push(#{SharedInterceptor}::new(interceptor));
                            self
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
                    // TODO(enableNewSmithyRuntime): Delete this branch when middleware is no longer used
                    rustTemplate(
                        """
                        ##[doc(hidden)]
                        // This is a temporary method for testing. NEVER use it in production
                        pub fn request_time_for_tests(mut self, request_time: ::std::time::SystemTime) -> Self {
                            self.operation.properties_mut().insert(#{SharedTimeSource}::new(request_time));
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
