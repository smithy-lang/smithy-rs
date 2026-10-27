/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.ClientAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import kotlin.io.path.exists
import kotlin.io.path.readText

class MultiProtocolSchemaNamespaceTest {
    @Test
    fun `explicit opt out preserves legacy request and response`() =
        checkClient("com.example.multiprotocol", expectSchema = false, disableSchemaSerde = true)

    @Test
    fun `example namespace uses legacy serde by default`() =
        checkClient("com.example.multiprotocol", expectSchema = false)

    @Test
    fun `fixture namespace uses schema serde by default`() =
        checkClient("smithy.rust.codegen.test.multiprotocol", expectSchema = true)

    @Test
    fun `fixture namespace can opt out of schema serde`() =
        checkClient("smithy.rust.codegen.test.multiprotocol", expectSchema = false, disableSchemaSerde = true)

    private fun checkClient(
        namespace: String,
        expectSchema: Boolean,
        disableSchemaSerde: Boolean = false,
    ) {
        val model =
            """
            namespace $namespace
            use aws.protocols#restJson1
            @restJson1
            service NamespaceService { version: "1", operations: [Echo] }
            @optionalAuth
            @http(method: "POST", uri: "/echo")
            operation Echo { input: EchoInput, output: EchoOutput }
            structure EchoInput { value: String }
            structure EchoOutput { value: String }
            """.asSmithyModel()
        val generated =
            clientIntegrationTest(
                model,
                IntegrationTestParams(
                    cargoCommand = "cargo test --quiet --features behavior-version-latest",
                    additionalSettings =
                        if (disableSchemaSerde) {
                            ClientAdditionalSettings.builder().disableSchemaSerde(true).build().toObjectNode()
                        } else {
                            ObjectNode.builder().build()
                        },
                ),
            ) { context, crate ->
                check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(context) == expectSchema)
                crate.testModule {
                    tokioTest("request_and_response_retain_rest_json_wire_format") {
                        rustTemplate(
                            """
                            let http_client = #{infallible_client_fn}(|request| {
                                assert_eq!(request.method().as_str(), "POST");
                                assert_eq!(request.uri().path(), "/echo");
                                assert_eq!(request.headers()["content-type"], "application/json");
                                assert_eq!(request.body().bytes().unwrap(), br##"{"value":"request"}"##);
                                #{Http}::Response::builder()
                                    .status(200)
                                    .header("content-type", "application/json")
                                    .body(#{SdkBody}::from(r##"{"value":"response"}"##))
                                    .unwrap()
                            });
                            let client = crate::Client::from_conf(
                                crate::Config::builder()
                                    .http_client(http_client)
                                    .endpoint_url("http://localhost:1234")
                                    .build(),
                            );
                            let output = client.echo().value("request").send().await.unwrap();
                            assert_eq!(output.value(), #{Some}("response"));
                            """,
                            *RuntimeType.preludeScope,
                            "Http" to CargoDependency.Http1x.toType(),
                            "SdkBody" to RuntimeType.sdkBody(context.runtimeConfig),
                            "infallible_client_fn" to
                                CargoDependency.smithyHttpClientTestUtil(context.runtimeConfig)
                                    .toType().resolve("test_util::infallible_client_fn"),
                        )
                    }
                }
            }
        check(generated.resolve("src/protocol_serde.rs").exists() == !expectSchema)
        check(generated.resolve("src/config.rs").readText().contains("pub fn protocol(") == expectSchema)
    }
}
