/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.routing

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Builds services with a missing operation handler and invokes them with routing context intact
 * and with request extensions discarded by middleware. The intact-context schema error must
 * match legacy. The legacy response also survives discarded extensions; the schema response
 * instead must be the explicitly pinned empty 500 fallback, with neither path panicking.
 */
internal class MissingHandlerContextCompatibilityTest {
    @ParameterizedTest
    @ValueSource(strings = ["restJson1", "restXml", "awsJson1_0", "awsJson1_1", "rpcv2Cbor"])
    fun `missing handler responses survive removed routing context`(protocol: String) {
        val namespace = if (protocol == "rpcv2Cbor") "smithy.protocols" else "aws.protocols"
        val uri =
            when (protocol) {
                "restJson1", "restXml" -> "/call"
                "rpcv2Cbor" -> "/service/Failures/operation/Call"
                else -> "/"
            }
        val headers =
            when (protocol) {
                "rpcv2Cbor" -> """.header("smithy-protocol", "rpc-v2-cbor")"""
                "awsJson1_0", "awsJson1_1" -> """.header("x-amz-target", "Failures.Call")"""
                else -> ""
            }
        val model =
            """
            ${'$'}version: "2"
            namespace test.missinghandler
            use $namespace#$protocol
            @$protocol
            service Failures { version: "1", operations: [Call] }
            @http(method: "POST", uri: "/call")
            operation Call { input := {} output := {} }
            """.asSmithyModel()
        val run =
            Files.createTempDirectory("missing-handler-$protocol-")
        for (schemaSerde in listOf(false, true)) {
            val mode = if (schemaSerde) "schema" else "legacy"
            val settings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode().toBuilder()
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = settings.build(),
                    overrideTestDir = run.resolve(mode).toFile(),
                    cargoCommand = "cargo test --quiet --lib -- --nocapture",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("missing_handler_preserves_framing_with_context_and_handles_context_loss") {
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            use #{Futures}::FutureExt;
                            let mut observations = #{Vec}::new();
                            for remove_context in [false, true] {
                                let layer = #{Tower}::ServiceBuilder::new().map_request(move |request: #{Http}::Request<#{Server}::body::Body>| {
                                    if remove_context {
                                        let (mut parts, body) = request.into_parts();
                                        parts.extensions = #{Default}::default();
                                        #{Http}::Request::from_parts(parts, body)
                                    } else {
                                        request
                                    }
                                });
                                let config = crate::FailuresConfig::builder().layer(layer).build();
                                let service = crate::Failures::builder(config).build_unchecked();
                                let request = #{Http}::Request::builder().method("POST").uri("$uri")
                                    $headers
                                    .body(#{Server}::body::Body::empty()).unwrap();
                                let response = ::std::panic::AssertUnwindSafe(service.oneshot(request)).catch_unwind().await;
                                let observation = match response {
                                    #{Ok}(response) => {
                                        let response = response.unwrap();
                                        let status = response.status().as_u16();
                                        let mut headers = response.headers().iter().map(|(name, value)|
                                            (name.as_str().to_owned(), value.as_bytes().to_vec())
                                        ).collect::<#{Vec}<_>>();
                                        headers.sort();
                                        let body = #{BodyUtil}::BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
                                        println!("$protocol schemaSerde=$schemaSerde remove_context={remove_context}: {status} {headers:?} {body:?}");
                                        assert_eq!(status, 500);
                                        #{Json}::json!({"remove_context":remove_context,"panic":false,"status":status,"headers":headers,"body":body.to_vec()})
                                    }
                                    #{Err}(_) => #{Json}::json!({"remove_context":remove_context,"panic":true}),
                                };
                                observations.push(observation);
                            }
                            ::std::fs::write(#{Snapshot:S}, #{Json}::to_string_pretty(&observations).unwrap()).unwrap();
                            if $schemaSerde {
                                let legacy: #{Vec}<#{Json}::Value> = #{Json}::from_str(
                                    &::std::fs::read_to_string(#{LegacySnapshot:S}).unwrap()).unwrap();
                                // Preserve the generated serde observation for the intact-context request.
                                assert_eq!(observations[0], legacy[0]);
                                // Schema routing cannot recover the selected protocol after
                                // middleware discards extensions, so use the empty 500 fallback.
                                assert_eq!(observations[1], #{Json}::json!({
                                    "remove_context":true,"panic":false,"status":500,"headers":[],"body":[]
                                }));
                            } else {
                                assert!(observations.iter().all(|row| row["panic"] == false));
                                let mut control = observations[0].clone();
                                control["remove_context"] = #{Json}::json!(true);
                                assert_eq!(control, observations[1]);
                            }
                            """,
                            *preludeScope,
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Futures" to CargoDependency.FuturesUtil.copy(features = setOf("std", "async-await")).toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Json" to CargoDependency.SerdeJson.toType(),
                            "Snapshot" to run.resolve("$mode.json").toString(),
                            "LegacySnapshot" to run.resolve("legacy.json").toString(),
                        )
                    }
                }
            }
        }
    }
}
