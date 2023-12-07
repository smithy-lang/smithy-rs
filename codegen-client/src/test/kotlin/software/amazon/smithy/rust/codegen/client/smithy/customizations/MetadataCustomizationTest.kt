/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.BasicTestModels
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

class MetadataCustomizationTest {

    @Test
    fun `extract metadata via customizable operation`() {
        clientIntegrationTest(BasicTestModels.AwsJson10TestModel) { clientCodegenContext, rustCrate ->
            val runtimeConfig = clientCodegenContext.runtimeConfig
            val codegenScope = arrayOf(
                *preludeScope,
                "BeforeTransmitInterceptorContextMut" to RuntimeType.beforeTransmitInterceptorContextMut(runtimeConfig),
                "BoxError" to RuntimeType.boxError(runtimeConfig),
                "ConfigBag" to RuntimeType.configBag(runtimeConfig),
                "Intercept" to RuntimeType.intercept(runtimeConfig),
                "Metadata" to RuntimeType.operationModule(runtimeConfig).resolve("Metadata"),
                "capture_request" to RuntimeType.captureRequest(runtimeConfig),
                "RuntimeComponents" to RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::runtime_components::RuntimeComponents"),
            )
            rustCrate.testModule {
                addDependency(CargoDependency.Tokio.toDevDependency().withFeature("test-util"))
                tokioTest("test_extract_metadata_via_customizable_operation") {
                    rustTemplate(
                        """
                        // Interceptors aren’t supposed to store states, but it is done this way for a testing purpose.
                        ##[derive(Debug)]
                        struct ExtractMetadataInterceptor(
                            ::std::sync::Mutex<#{Option}<::std::sync::mpsc::Sender<(String, String)>>>,
                        );

                        impl #{Intercept} for ExtractMetadataInterceptor {
                            fn name(&self) -> &'static str {
                                "ExtractMetadataInterceptor"
                            }

                            fn modify_before_signing(
                                &self,
                                _context: &mut #{BeforeTransmitInterceptorContextMut}<'_>,
                                _runtime_components: &#{RuntimeComponents},
                                cfg: &mut #{ConfigBag},
                            ) -> #{Result}<(), #{BoxError}> {
                                let metadata = cfg
                                    .load::<#{Metadata}>()
                                    .expect("metadata should exist");
                                let service_name = metadata.service().to_string();
                                let operation_name = metadata.name().to_string();
                                let tx = self.0.lock().unwrap().take().unwrap();
                                tx.send((service_name, operation_name)).unwrap();
                                #{Ok}(())
                            }
                        }

                        let (tx, rx) = ::std::sync::mpsc::channel();

                        let (http_client, _captured_request) = #{capture_request}(#{None});
                        let client_config = crate::config::Config::builder()
                            .endpoint_url("http://localhost:1234/")
                            .http_client(http_client)
                            .build();
                        let client = crate::client::Client::from_conf(client_config);
                        let _ = client
                            .say_hello()
                            .customize()
                            .interceptor(ExtractMetadataInterceptor(::std::sync::Mutex::new(#{Some}(tx))))
                            .send()
                            .await;

                        match rx.recv() {
                            #{Ok}((service_name, operation_name)) => {
                                assert_eq!("HelloService", &service_name);
                                assert_eq!("SayHello", &operation_name);
                            }
                            #{Err}(_) => panic!(
                                "failed to receive service name and operation name from `ExtractMetadataInterceptor`"
                            ),
                        }
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }
}
