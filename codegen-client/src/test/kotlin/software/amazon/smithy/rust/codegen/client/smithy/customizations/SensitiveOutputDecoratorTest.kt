/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

class SensitiveOutputDecoratorTest {
    private fun codegenScope(runtimeConfig: RuntimeConfig): Array<Pair<String, Any>> =
        arrayOf(
            "capture_test_logs" to
                CargoDependency.smithyRuntimeTestUtil(runtimeConfig).toType()
                    .resolve("test_util::capture_test_logs::capture_test_logs"),
            "capture_request" to RuntimeType.captureRequest(runtimeConfig),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "http_1x" to CargoDependency.Http1x.toType(),
        )

    private val model =
        """
        namespace com.example
        use aws.protocols#awsJson1_0
        @awsJson1_0
        service HelloService {
            operations: [SayHello],
            version: "1"
        }
        @optionalAuth
        operation SayHello { output: TestOutput }

        @sensitive
        structure Credentials {
           username: String,
           password: String
        }

        structure TestOutput {
           credentials: Credentials,
        }
        """.asSmithyModel()

    @Test
    fun `sensitive output in model should redact response body`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("redacting_sensitive_response_body") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn redacting_sensitive_response_body() {
                        let (_logs, logs_rx) = #{capture_test_logs}();
                        let (http_client, _r) = #{capture_request}(Some(
                            #{http_1x}::Response::builder()
                                .status(200)
                                .body(#{SdkBody}::from(""))
                                .unwrap(),
                        ));

                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client.clone())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.say_hello()
                            .send()
                            .await
                            .expect("success");

                        let log_contents = logs_rx.contents();
                        assert!(log_contents.contains("** REDACTED **"));
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }
}
