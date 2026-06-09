/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

class OutputExtensionsDecoratorTest {
    private fun codegenScope(runtimeConfig: RuntimeConfig): Array<Pair<String, Any>> =
        arrayOf(
            "capture_request" to RuntimeType.captureRequest(runtimeConfig),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "ProvideExtensions" to
                RuntimeType.smithyTypes(runtimeConfig).resolve("extensions::ProvideExtensions"),
            "http_1x" to software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Http1x.toType(),
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

        structure TestOutput {
           message: String,
        }
        """.asSmithyModel()

    @Test
    fun `operation output exposes empty extensions by default`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("output_extensions") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn output_has_extensions_accessor() {
                        let (http_client, _r) = #{capture_request}(Some(
                            #{http_1x}::Response::builder()
                                .status(200)
                                .body(#{SdkBody}::from("{}"))
                                .unwrap(),
                        ));

                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client.clone())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let out = client.say_hello().send().await.expect("success");

                        // The `ProvideExtensions` trait is implemented for the output,
                        // and is empty when nothing populated it.
                        use #{ProvideExtensions};
                        assert!(out.extensions().is_empty());
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }
}
