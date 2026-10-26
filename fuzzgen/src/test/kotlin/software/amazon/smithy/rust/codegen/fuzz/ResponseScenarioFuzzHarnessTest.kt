/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import org.junit.jupiter.api.Test
import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.runCommand
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class ResponseScenarioFuzzHarnessTest {
    @Test
    fun `declared fixtures produce populated defaults and errors with unredacted deterministic inputs`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.fuzzscenario
            use aws.protocols#restJson1
            use smithy.test#httpResponseTests
            @restJson1
            service Scenarios { version: "1", operations: [Payload, Constrained] }
            @http(method: "POST", uri: "/payload")
            @httpResponseTests([
                { id: "Absent", tags: ["fuzz:absent"], protocol: restJson1, code: 200, params: {} },
                { id: "Empty", tags: ["fuzz:empty"], protocol: restJson1, code: 200, params: { body: { label: "" } } },
                { id: "Populated", tags: ["fuzz:populated"], protocol: restJson1, code: 200,
                    params: { body: { label: "result", secret: "response-secret", values: { z: 2, a: 1 } } } },
                { id: "Default", tags: ["fuzz:default"], protocol: restJson1, code: 200, params: { body: {} } },
                { id: "Override", tags: ["fuzz:override"], protocol: restJson1, code: 200, params: { body: { label: "override" } } }
            ])
            operation Payload { input: PayloadInput, output: PayloadOutput, errors: [Failure] }
            structure PayloadInput {
                @httpHeader("x-scenario")
                scenario: String
                @httpPayload
                body: Target
            }
            structure PayloadOutput { @httpPayload body: Target }
            structure Target {
                label: String = "modeled-default"
                secret: Secret
                values: Values
            }
            @sensitive
            string Secret
            map Values { key: String, value: Integer }
            @error("client") @httpError(409)
            @httpResponseTests([
                { id: "Error", tags: ["fuzz:error"], protocol: restJson1, code: 409,
                    params: { message: "fixture-failure", details: { secret: "error-secret", label: "error-value" } } }
            ])
            structure Failure { message: String, details: Target }
            @http(method: "POST", uri: "/constrained")
            @httpResponseTests([
                { id: "Valid", tags: ["fuzz:populated"], protocol: restJson1, code: 200, params: { token: "valid" } }
            ])
            operation Constrained { input := { @httpHeader("x-scenario") scenario: String }, output := { @required token: Token } }
            @length(min: 3)
            string Token
        """.asSmithyModel()
        for (schema in listOf(false, true)) {
            val server =
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        service = "test.fuzzscenario#Scenarios",
                        additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schema).toObjectNode(),
                        command = { dir -> println("generated scenario schema=$schema at $dir") },
                    ),
                    testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                ).single()
            val manifest = FileManifest.create(TestWorkspace.subproject().toPath())
            FuzzHarnessBuildPlugin().execute(
                PluginContext.builder().model(model).fileManifest(manifest).settings(
                    Node.objectNode()
                        .withMember("service", "test.fuzzscenario#Scenarios")
                        .withMember(
                            "targetCrates",
                            ArrayNode.fromNodes(Node.objectNode().withMember("name", "target").withMember("relativePath", server.path.toString())),
                        )
                        .withMember("runtimeConfig", Node.objectNode().withMember("relativePath", TestRuntimeConfig.runtimeCrateLocation.path!!)),
                ).build(),
            )
            val target = manifest.baseDir.resolve("target")
            target.resolve("Cargo.toml").toFile().appendText("\n[dev-dependencies]\nserde_json = \"1\"\nhttp = \"1\"\n")
            target.resolve("src/lib.rs").toFile().appendText(
                """

                #[cfg(test)]
                mod scenario_tests {
                    fn invoke(path: &str, scenario: &str, body: &str) -> aws_smithy_fuzz::FuzzResult {
                        super::TARGET.lock().unwrap().invoke(http::Request::builder()
                            .method("POST").uri(path).header("content-type", "application/json")
                            .header("x-scenario", scenario)
                            .body(aws_smithy_fuzz::Body::from_bytes(body.as_bytes().to_vec())).unwrap())
                    }
                    #[test]
                    fn distinct_success_and_error_values_are_serialized() {
                        let absent = invoke("/payload", "absent", "{}");
                        assert_eq!(absent.response.status, 200);
                        assert_eq!(serde_json::from_slice::<serde_json::Value>(&absent.response.body).unwrap(), serde_json::json!({}));
                        for (scenario, label) in [
                            ("empty", ""), ("default", "modeled-default"),
                            ("populated", "result"), ("override", "override"),
                        ] {
                            let result = invoke("/payload", scenario, "{}");
                            assert_eq!(result.response.status, 200, "{result:?}");
                            let value: serde_json::Value = serde_json::from_slice(&result.response.body).unwrap();
                            assert_eq!(value["label"], label);
                            if scenario == "populated" {
                                assert_eq!(value["secret"], "response-secret");
                                assert_eq!(value["values"], serde_json::json!({"a": 1, "z": 2}));
                            }
                        }
                        let error = invoke("/payload", "error", "{}");
                        assert_eq!(error.response.status, 409, "{error:?}");
                        let value: serde_json::Value = serde_json::from_slice(&error.response.body).unwrap();
                        assert_eq!(value["message"], "fixture-failure");
                        assert_eq!(value["details"]["secret"], "error-secret");
                        assert_eq!(value["details"]["label"], "error-value");
                        let constrained = invoke("/constrained", "populated", "{}");
                        assert_eq!(constrained.response.status, 200, "{constrained:?}");
                        assert_eq!(serde_json::from_slice::<serde_json::Value>(&constrained.response.body).unwrap()["token"], "valid");
                    }
                    #[test]
                    fn typed_records_distinguish_secrets_and_ignore_map_iteration_order() {
                        let first = invoke("/payload", "populated", r#"{"secret":"secret-one","values":{"z":2,"a":1}}"#).input.unwrap();
                        let reordered = invoke("/payload", "populated", r#"{"values":{"a":1,"z":2},"secret":"secret-one"}"#).input.unwrap();
                        let changed = invoke("/payload", "populated", r#"{"secret":"secret-two","values":{"z":2,"a":1}}"#).input.unwrap();
                        assert_eq!(first, reordered);
                        assert_ne!(first, changed);
                        assert!(first.contains("secret-one"), "{first}");
                        assert!(!first.contains("Sensitive Data Redacted"), "{first}");
                    }
                }
                """.trimIndent(),
            )
            "cargo test --quiet".runCommand(target)
        }
    }
}
