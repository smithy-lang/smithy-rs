/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.routing

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
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
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Sends known-operation requests and protocol-specific routing failures to single-protocol
 * services generated with legacy and schema serde. It captures response status, all headers,
 * and body and requires identical observations for each protocol. The test isolates routing
 * rejections from multi-protocol priority and request-claiming behavior.
 */
internal class SingleProtocolRoutingCompatibilityTest {
    @Test
    fun `single protocol routing rejection responses match legacy`() {
        val observations = Files.createTempDirectory("single-protocol-routing-")
        for (protocol in listOf("restJson1", "restXml", "awsJson1_0", "awsJson1_1", "rpcv2Cbor")) {
            val uri =
                if (protocol == "rpcv2Cbor") {
                    "/service/Routes/operation/Read"
                } else if (protocol.startsWith("awsJson")) {
                    "/"
                } else {
                    "/read"
                }
            val contentType =
                when (protocol) {
                    "restXml" -> "application/xml"
                    "awsJson1_0" -> "application/x-amz-json-1.0"
                    "awsJson1_1" -> "application/x-amz-json-1.1"
                    "rpcv2Cbor" -> "application/cbor"
                    else -> "application/json"
                }
            val model =
                """
                ${'$'}version: "2"
                namespace test.routingcompat
                use ${if (protocol == "rpcv2Cbor") "smithy.protocols" else "aws.protocols"}#$protocol
                @$protocol service Routes { version: "1", operations: [Read] }
                @http(method: "POST", uri: "/read")
                operation Read { input := {}, output := {} }
                """.asSmithyModel()
            for (schemaSerde in listOf(false, true)) {
                val snapshot = observations.resolve("$protocol-$schemaSerde.txt")
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                        cargoCommand = "cargo test --quiet --lib",
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    crate.testModule {
                        tokioTest("routing_rejections") {
                            rustTemplate(
                                """
                                let app = crate::Routes::builder(crate::RoutesConfig::builder().build())
                                    .read(|_: crate::input::ReadInput| async { crate::output::ReadOutput::builder().build() })
                                    .build().unwrap();
                                let mut records = #{String}::new();
                                for (name, method, path, identified, target) in [
                                    ("known", "POST", ${uri.dq()}, true, "Routes.Read"),
                                    ("missing", "POST", "/no/such/operation", true, "Routes.Missing"),
                                    ("method", "GET", ${uri.dq()}, true, "Routes.Read"),
                                    ("unidentified", "POST", ${uri.dq()}, false, ""),
                                    ("unidentified-missing", "POST", "/no/such/operation", false, ""),
                                ] {
                                    let _ = target;
                                    let mut request = #{Http}::Request::builder().method(method).uri(path);
                                    if identified {
                                        request = request.header("content-type", ${contentType.dq()});
                                        ${if (protocol == "rpcv2Cbor") {
                                    """request = request.header("smithy-protocol", "rpc-v2-cbor");"""
                                } else if (protocol.startsWith("awsJson")) {
                                    """request = request.header("x-amz-target", target);"""
                                } else {
                                    "let _ = target;"
                                }}
                                    }
                                    let request = request.body(#{Server}::body::to_boxed_sync("")).unwrap();
                                    let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                    let status = response.status().as_u16();
                                    if name == "known" { assert_eq!(status, 200); }
                                    let mut headers: #{Vec}<_> = response.headers().iter()
                                        .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string())).collect();
                                    headers.sort();
                                    #{Collect}
                                    records.push_str(&format!("{name}: {status} {headers:?} {:?}\n", body.as_ref()));
                                }
                                ::std::fs::write(${snapshot.toString().dq()}, records).unwrap();
                                """,
                                *RuntimeType.preludeScope,
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                            )
                        }
                    }
                }
            }
            assertEquals(Files.readString(observations.resolve("$protocol-false.txt")), Files.readString(observations.resolve("$protocol-true.txt")), "$protocol: $observations")
        }
    }
}

/**
 * Uploads raw abc bytes to a Blob payload with no modeled media type, using absent, arbitrary,
 * and application/octet-stream Content-Type values. Legacy and schema REST-only services must
 * both accept all three and return the blob size. A schema REST JSON plus CBOR service must
 * accept application/octet-stream and return the pinned 404 fallback for the other two values.
 * The test records this multi-protocol routing difference explicitly rather than treating it
 * as a serde compatibility result.
 */
internal class BlobPayloadRoutingCompatibilityTest {
    @Test
    fun `blob content types preserve single protocol behavior and determine multi protocol routing`() {
        val observations = Files.createTempDirectory("untyped-blob-routing-")
        System.err.println("Untyped blob routing observations: $observations")
        // Generate REST JSON with generated serde, then REST JSON and REST JSON + CBOR with schema serde.
        // All use HTTP 1.x and send identical requests. Generated serde serves one protocol.
        for ((schemaSerde, multiProtocol) in listOf(false to false, true to false, true to true)) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.uploadrouting
                use aws.protocols#restJson1
                @restJson1
                ${if (multiProtocol) "@smithy.protocols#rpcv2Cbor" else ""}
                service Uploads { version: "1", operations: [Upload] }
                @http(method: "POST", uri: "/upload")
                operation Upload {
                    input := { @required @httpPayload data: Blob }
                    output := { @required size: Integer }
                }
                """.asSmithyModel()
            val snapshot = observations.resolve("$schemaSerde-$multiProtocol.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                    overrideTestDir = observations.resolve("$schemaSerde-$multiProtocol").toFile(),
                    cargoCommand = "cargo test --quiet --lib -- --nocapture",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                crate.testModule {
                    tokioTest("upload_wire_observations") {
                        rustTemplate(
                            """
                            let app = crate::Uploads::builder(crate::UploadsConfig::builder().build())
                                .upload(|input: crate::input::UploadInput| async move {
                                    #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput { size: input.data.as_ref().len() as i32 })
                                }).build().unwrap();
                            let mut records = #{String}::new();
                            // Arbitrary, default Blob, and absent Content-Type, with the same raw bytes.
                            for content_type in [#{Some}("video/mp4"), #{Some}("application/octet-stream"), #{None}] {
                                let mut request = #{Http}::Request::post("/upload");
                                if let #{Some}(value) = content_type { request = request.header("content-type", value); }
                                let request = request.body(#{Server}::body::to_boxed_sync("abc")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                // REST-only servers accept all three cases. With CBOR also enabled,
                                // only application/octet-stream identifies REST JSON for this payload.
                                let expected_status = if $multiProtocol && content_type != #{Some}("application/octet-stream") { 404 } else { 200 };
                                assert_eq!(status, expected_status, "{content_type:?}");
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.as_bytes().to_vec())).collect();
                                headers.sort();
                                #{Collect}
                                if status == 200 {
                                    assert_eq!(body.as_ref(), b"{\"size\":3}");
                                } else {
                                    // No protocol claimed the request: pin the shared fallback's
                                    // headers and body as well as its 404 status.
                                    assert!(headers.is_empty());
                                    assert_eq!(body.as_ref(), b"<UnknownOperationException/>\n");
                                }
                                let record = format!("{content_type:?}: {status} {headers:?} {:?}\n", body.as_ref());
                                print!("{record}");
                                records.push_str(&record);
                            }
                            ::std::fs::write(${snapshot.toString().dq()}, records).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        )
                    }
                }
            }
        }
        // Compare complete REST-only responses: status, sorted headers, and body bytes.
        // The multi-protocol behavior is asserted separately in its generated Rust test.
        assertEquals(Files.readString(observations.resolve("false-false.txt")), Files.readString(observations.resolve("true-false.txt")))
    }
}

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
