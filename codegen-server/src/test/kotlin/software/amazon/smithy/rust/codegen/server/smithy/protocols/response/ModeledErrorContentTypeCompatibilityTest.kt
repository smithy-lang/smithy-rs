/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.response

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Disabled
import org.junit.jupiter.api.Tag
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

/**
 * Calls operations with empty, document, text, blob, media-typed, and streaming outputs, returning
 * both success values and modeled errors. Schema errors use the protocol content type unless
 * the error binds an explicit header, regardless of the success output. Legacy behavior stays
 * covered, and wire captures match apart from the intentional REST error content-type difference.
 * The CBOR comparison is disabled pending a fix for legacy error header serialization.
 */
@Tag("server-compatibility")
internal class ModeledErrorContentTypeCompatibilityTest {
    @Test
    fun `schema modeled error content type is independent of the operation output`() {
        checkContentTypes(listOf("JsonErrorContentType", "XmlErrorContentType", "Json10ErrorContentType", "Json11ErrorContentType"))
    }

    // TODO(Fix with legacy): In CBOR, @httpHeader on a modeled error field makes generated serde uncompilable.
    @Disabled("Legacy CBOR error header bindings generate uncompilable Rust; reproduced on mainline at 0075e635d")
    @Test
    fun `CBOR modeled error content type matches legacy`() {
        checkContentTypes(listOf("CborErrorContentType"))
    }

    private fun checkContentTypes(protocols: List<String>) {
        val root = Path.of("..").toAbsolutePath().normalize()
        val model = Files.readString(root.resolve("codegen-server/src/test/resources/protocol-compat/models/error-content-type.smithy")).asSmithyModel()
        val run =
            Files.createTempDirectory("C05-")
        for (schemaSerde in listOf(false, true)) {
            for (service in protocols) {
                val isRest = service == "JsonErrorContentType" || service == "XmlErrorContentType"
                val isCbor = service == "CborErrorContentType"
                val documentType =
                    when (service) {
                        "JsonErrorContentType" -> "application/json"
                        "XmlErrorContentType" -> "application/xml"
                        "Json10ErrorContentType" -> "application/x-amz-json-1.0"
                        "Json11ErrorContentType" -> "application/x-amz-json-1.1"
                        else -> "application/cbor"
                    }
                val mode =
                    if (schemaSerde) {
                        "schema"
                    } else {
                        "legacy"
                    }
                val streamHandler =
                    if (isRest) {
                        """
                        .stream(move |_| async move {
                            if scenario == "success" {
                                #{Ok}(crate::output::StreamOutput { body: #{Types}::byte_stream::ByteStream::from_static(b"stream") })
                            } else {
                                #{Err}(crate::error::StreamError::Failure(crate::error::Failure {
                                    message: #{Some}("failure".into()),
                                    content_type: content_type.map(str::to_owned),
                                }))
                            }
                        })
                        """
                    } else {
                        ""
                    }
                val settings =
                    ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode().toBuilder()
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        service = "compat.errorcontenttype#$service",
                        additionalSettings = settings.build(),
                        overrideTestDir = run.resolve("$service-$mode").toFile(),
                        command = { path ->
                            val log = run.resolve("$service-$mode-cargo.log")
                            val process =
                                ProcessBuilder("cargo", "test", "--quiet", "--lib", "modeled_error_content_type")
                                    .directory(path.toFile()).redirectErrorStream(true)
                            process.environment()["RUSTFLAGS"] = "-D warnings"
                            val running = process.start()
                            Files.newBufferedWriter(log).use { output ->
                                running.inputStream.bufferedReader().useLines { lines ->
                                    lines.forEach { line ->
                                        println(line)
                                        output.write(line)
                                        output.newLine()
                                        output.flush()
                                    }
                                }
                            }
                            check(running.waitFor() == 0) { "Cargo failed: $log" }
                        },
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    check(context.usesSchemaHttpSerde == schemaSerde)
                    crate.testModule {
                        tokioTest("modeled_error_content_type") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                use #{BodyUtil}::BodyExt;
                                use #{Json}::json;
                                let mut observations = #{Vec}::new();
                                for scenario in ["success", "absent", "explicit", "empty", "invalid", "document-type"] {
                                    let content_type = match scenario {
                                        "explicit" => #{Some}("custom/override"),
                                        "empty" => #{Some}(""),
                                        "invalid" => #{Some}("invalid\nvalue"),
                                        "document-type" => #{Some}("$documentType"),
                                        _ => #{None},
                                    };
                                    let app = crate::$service::builder(crate::${service}Config::builder().build())
                                        .empty(move |_| async move {
                                            if scenario == "success" {
                                                #{Ok}(crate::output::EmptyOutput {  })
                                            } else {
                                                #{Err}(crate::error::EmptyError::Failure(crate::error::Failure {
                                                    message: #{Some}("failure".into()),
                                                    content_type: content_type.map(str::to_owned),
                                                }))
                                            }
                                        })
                                        .document(move |_| async move {
                                            if scenario == "success" {
                                                #{Ok}(crate::output::DocumentOutput { value: #{Some}("value".into()) })
                                            } else {
                                                #{Err}(crate::error::DocumentError::Failure(crate::error::Failure {
                                                    message: #{Some}("failure".into()),
                                                    content_type: content_type.map(str::to_owned),
                                                }))
                                            }
                                        })
                                        .text(move |_| async move {
                                            if scenario == "success" {
                                                #{Ok}(crate::output::TextOutput { body: #{Some}("text".into()) })
                                            } else {
                                                #{Err}(crate::error::TextError::Failure(crate::error::Failure {
                                                    message: #{Some}("failure".into()),
                                                    content_type: content_type.map(str::to_owned),
                                                }))
                                            }
                                        })
                                        .media_text(move |_| async move {
                                            if scenario == "success" {
                                                #{Ok}(crate::output::MediaTextOutput { body: #{Some}("text".into()) })
                                            } else {
                                                #{Err}(crate::error::MediaTextError::Failure(crate::error::Failure {
                                                    message: #{Some}("failure".into()),
                                                    content_type: content_type.map(str::to_owned),
                                                }))
                                            }
                                        })
                                        .raw_blob(move |_| async move {
                                            if scenario == "success" {
                                                #{Ok}(crate::output::RawBlobOutput { body: #{Some}(#{Types}::Blob::new("blob")) })
                                            } else {
                                                #{Err}(crate::error::RawBlobError::Failure(crate::error::Failure {
                                                    message: #{Some}("failure".into()),
                                                    content_type: content_type.map(str::to_owned),
                                                }))
                                            }
                                        })
                                        .media_blob(move |_| async move {
                                            if scenario == "success" {
                                                #{Ok}(crate::output::MediaBlobOutput { body: #{Some}(#{Types}::Blob::new("blob")) })
                                            } else {
                                                #{Err}(crate::error::MediaBlobError::Failure(crate::error::Failure {
                                                    message: #{Some}("failure".into()),
                                                    content_type: content_type.map(str::to_owned),
                                                }))
                                            }
                                        })
                                        $streamHandler
                                        .build().unwrap();
                                    let document_type = "$documentType";
                                    let empty_type = if "$service" == "JsonErrorContentType" { #{Some}(document_type) } else { #{None} };
                                    let blob_type = if "$service" == "JsonErrorContentType" { #{None} } else { #{Some}("application/octet-stream") };
                                    for (operation, default_type) in [
                                        ("Empty", empty_type), ("Document", #{Some}(document_type)),
                                        ("Text", #{Some}("text/plain")), ("MediaText", #{Some}("application/custom-text")),
                                        ("RawBlob", blob_type), ("MediaBlob", #{Some}("application/custom-binary")),
                                        ("Stream", #{Some}("application/octet-stream")),
                                    ] {
                                        if !$isRest && operation == "Stream" { continue; }
                                        let request = if $isRest {
                                            #{Http}::Request::get(format!("/{operation}"))
                                                .body(#{Server}::body::Body::empty()).unwrap()
                                        } else if $isCbor {
                                            #{Http}::Request::post(format!("/service/$service/operation/{operation}"))
                                                .header("content-type", document_type).header("smithy-protocol", "rpc-v2-cbor")
                                                .body(#{Server}::body::Body::from_bytes(#{Bytes}::from_static(b"\xa0"))).unwrap()
                                        } else {
                                            #{Http}::Request::post("/").header("content-type", document_type)
                                                .header("x-amz-target", format!("$service.{operation}"))
                                                .body(#{Server}::body::Body::from_bytes(#{Bytes}::from_static(b"{}"))).unwrap()
                                        };
                                        let response = app.clone().oneshot(request).await.unwrap();
                                        let status = response.status().as_u16();
                                        assert_eq!(status, if scenario == "success" { 200 } else if !$isCbor && scenario == "invalid" { 400 } else { 422 }, "{operation}/{scenario}");
                                        if scenario != "invalid" {
                                            // Generated serde treats an empty scalar header as absent.
                                            let default_type = if $isRest && (scenario == "success" || ${!schemaSerde}) {
                                                default_type
                                            } else {
                                                #{Some}(document_type)
                                            };
                                            let expected = if scenario == "success" || $isCbor { default_type } else {
                                                content_type.filter(|value| !value.is_empty()).or(default_type)
                                            };
                                            assert_eq!(response.headers().get("content-type").map(|h| h.to_str().unwrap()), expected, "{operation}/{scenario}");
                                        }
                                        let mut headers = response.headers().iter().map(|(name, value)|
                                            (name.as_str().to_owned(), value.as_bytes().to_vec())).collect::<#{Vec}<_>>();
                                        headers.sort();
                                        let body = response.into_body().collect().await.unwrap().to_bytes();
                                        observations.push(json!({"operation":operation,"scenario":scenario,"status":status,"headers":headers,"body":body.to_vec()}));
                                    }
                                }
                                std::fs::write(#{Snapshot:S}, #{Json}::to_string_pretty(&observations).unwrap()).unwrap();
                                """.trimIndent(),
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                                "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "Json" to CargoDependency.SerdeJson.toType(),
                                "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                                "Bytes" to RuntimeType.Bytes,
                                *RuntimeType.preludeScope,
                                "Snapshot" to run.resolve("$service-$mode.json").toString(),
                            )
                        }
                    }
                }
            }
        }
        for (service in protocols) {
            val schema = Node.parse(Files.readString(run.resolve("$service-schema.json"))).expectArrayNode().elements
            val legacy = Node.parse(Files.readString(run.resolve("$service-legacy.json"))).expectArrayNode().elements
            schema.size shouldBe legacy.size
            for ((schemaNode, legacyNode) in schema.zip(legacy)) {
                val schemaResponse = schemaNode.expectObjectNode()
                val legacyResponse = legacyNode.expectObjectNode()
                val scenario = schemaResponse.expectStringMember("scenario").value
                if ((service == "JsonErrorContentType" || service == "XmlErrorContentType") && scenario in listOf("absent", "empty")) {
                    // Only the default Content-Type changes; bodies, statuses, and other headers stay identical.
                    schemaResponse.withoutMember("headers") shouldBe legacyResponse.withoutMember("headers")

                    fun headersWithoutContentType(response: ObjectNode) =
                        response.expectArrayMember("headers").elements.filter {
                            it.expectArrayNode().elements.first().expectStringNode().value != "content-type"
                        }
                    headersWithoutContentType(schemaResponse) shouldBe headersWithoutContentType(legacyResponse)
                } else {
                    schemaResponse shouldBe legacyResponse
                }
            }
        }
    }
}
