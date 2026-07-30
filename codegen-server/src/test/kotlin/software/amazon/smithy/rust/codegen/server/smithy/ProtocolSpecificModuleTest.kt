/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.ints.shouldBeLessThan
import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import io.kotest.matchers.string.shouldNotContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File

class ProtocolSpecificModuleTest {
    private fun generatedSource(root: File): String =
        root.resolve("src").walkTopDown()
            .filter { it.isFile && it.extension == "rs" }
            .joinToString("\n") { it.readText() }

    @Test
    fun `single protocol preserves the legacy serde module and metadata`() {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.RestJson)
        val generatedServers =
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    service = serviceShapeId.toString(),
                    command = {},
                ),
                testCoverage = HttpTestType.Default,
            ) { _, _ -> }

        generatedServers.forEach { generatedServer ->
            val root = generatedServer.path.toFile()
            root.resolve("src/protocol_serde").isDirectory shouldBe true
            root.resolve("src/protocol_serde_rest_json1").exists() shouldBe false
            root.resolve("src/lib.rs").readText().also { libRs ->
                libRs shouldContain "pub(crate) mod protocol_serde;"
                libRs shouldNotContain "protocol_serde_rest_json1"
            }
            root.resolve("Cargo.toml").readText().also { cargoToml ->
                cargoToml shouldContain "protocol = \"aws.protocols#restJson1\""
                cargoToml shouldNotContain "protocols ="
            }
        }
    }

    @Test
    fun `multi protocol isolates serde validation and event stream helpers in detection order`() {
        val (restJsonModel, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.RestJson)
        val service =
            restJsonModel.expectShape(serviceShapeId, ServiceShape::class.java).toBuilder()
                .addTrait(Rpcv2CborTrait.builder().build())
                .build()
        val model: Model = ModelTransformer.create().replaceShapes(restJsonModel, listOf(service))
        val generatedServers =
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    service = serviceShapeId.toString(),
                    command = {},
                    additionalSettings =
                        ServerAdditionalSettings.builder()
                            .alwaysSendEventStreamInitialResponse()
                            .withHttp1x()
                            .toObjectNode(),
                ),
                testCoverage = HttpTestType.Default,
            ) { _, _ -> }

        generatedServers.forEach { generatedServer ->
            val root = generatedServer.path.toFile()
            root.resolve("src/protocol_rpcv2_cbor/serde").isDirectory shouldBe true
            root.resolve("src/protocol_rest_json1/serde").isDirectory shouldBe true
            root.resolve("src/protocol_rpcv2_cbor/operations.rs").isFile shouldBe true
            root.resolve("src/protocol_rest_json1/operations.rs").isFile shouldBe true

            val source = generatedSource(root)
            source shouldContain "crate::protocol_rpcv2_cbor::serde::shape_validation_exception"
            source shouldContain "crate::protocol_rest_json1::serde::shape_validation_exception"
            root.resolve("src/protocol_rpcv2_cbor/event_stream_serde.rs").isFile shouldBe true
            root.resolve("src/protocol_rest_json1/event_stream_serde.rs").isFile shouldBe true
            source shouldContain "crate::protocol_rpcv2_cbor::event_stream_serde::"
            source shouldContain "crate::protocol_rest_json1::event_stream_serde::"
            listOf("operation.rs", "input.rs").forEach { sharedFile ->
                root.resolve("src/$sharedFile").readText().also { sharedSource ->
                    sharedSource shouldNotContain "protocol_rpcv2_cbor::serde"
                    sharedSource shouldNotContain "protocol_rest_json1::serde"
                }
            }
            val protocolRoots = listOf("protocol_rpcv2_cbor", "protocol_rest_json1")
            protocolRoots.forEach { owner ->
                val ownedSource =
                    root.resolve("src/$owner").walkTopDown()
                        .filter { it.isFile && it.extension == "rs" }
                        .joinToString("\n") { it.readText() }
                protocolRoots.filterNot { it == owner }.forEach { other ->
                    ownedSource shouldNotContain "crate::$other::serde"
                    ownedSource shouldNotContain "crate::$other::event_stream_serde"
                }
            }
            source shouldNotContain "MarshallerForRpcv2Cbor"
            source shouldNotContain "MarshallerForRestJson1"
            val serviceSource = root.resolve("src/service.rs").readText()
            serviceSource shouldContain "protocol::rpc_v2_cbor::RpcV2Cbor"
            serviceSource shouldContain "protocol::rest_json_1::RestJson1"
            serviceSource.indexOf("protocol::rpc_v2_cbor::RpcV2Cbor")
                .shouldBeLessThan(serviceSource.indexOf("protocol::rest_json_1::RestJson1"))

            val cargoToml = root.resolve("Cargo.toml").readText()
            cargoToml shouldContain "protocol = \"smithy.protocols#rpcv2Cbor\""
            cargoToml shouldNotContain "protocols ="
        }
    }

    @Test
    fun `generated service dispatches ambiguous REST requests by order and accept header`() {
        val model =
            """
            ${'$'}version: "2"

            namespace test

            use aws.protocols#restJson1
            use aws.protocols#restXml

            @restJson1
            @restXml
            service MultiProtocolService {
                operations: [GetValue]
            }

            @http(method: "GET", uri: "/value", code: 200)
            operation GetValue {
                output := {
                    value: String
                }
            }
            """.asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = "test#MultiProtocolService",
                additionalSettings =
                    ServerAdditionalSettings.builder()
                        .withHttp1x()
                        .toObjectNode(),
            ),
            testCoverage = HttpTestType.Default,
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                tokioTest("dispatches_by_order_and_accept_header") {
                    rustTemplate(
                        """
                        use #{Tower}::ServiceExt;

                        async fn get_value(_input: crate::input::GetValueInput) -> crate::output::GetValueOutput {
                            crate::output::GetValueOutput::builder()
                                .value(Some("ok".to_owned()))
                                .build()
                        }

                        let config = crate::MultiProtocolServiceConfig::builder().build();
                        let service = crate::MultiProtocolService::builder(config)
                            .get_value(get_value)
                            .build()
                            .unwrap();
                        let request = #{Http}::Request::builder()
                            .method(#{Http}::Method::GET)
                            .uri("/value")
                            .body(#{BoxBody}::default())
                            .unwrap();
                        let response = service.oneshot(request).await.unwrap();
                        assert_eq!(
                            response.headers().get(#{Http}::header::CONTENT_TYPE).unwrap(),
                            "application/json"
                        );

                        let config = crate::MultiProtocolServiceConfig::builder().build();
                        let service = crate::MultiProtocolService::builder(config)
                            .get_value(get_value)
                            .build()
                            .unwrap();
                        let request = #{Http}::Request::builder()
                            .method(#{Http}::Method::GET)
                            .uri("/value")
                            .header(#{Http}::header::ACCEPT, "application/xml")
                            .body(#{BoxBody}::default())
                            .unwrap();
                        let response = service.oneshot(request).await.unwrap();
                        assert_eq!(
                            response.headers().get(#{Http}::header::CONTENT_TYPE).unwrap(),
                            "application/xml"
                        );
                        """,
                        "Tower" to ServerCargoDependency.Tower.toType(),
                        "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                        "BoxBody" to
                            ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType()
                                .resolve("body::BoxBody"),
                    )
                }
            }
        }
    }

    @Test
    fun `multi protocol rejects the legacy HTTP runtime`() {
        val (restJsonModel, serviceShapeId) = loadSmithyConstraintsModelForProtocol(ModelProtocol.RestJson)
        val service =
            restJsonModel.expectShape(serviceShapeId, ServiceShape::class.java).toBuilder()
                .addTrait(Rpcv2CborTrait.builder().build())
                .build()
        val model: Model = ModelTransformer.create().replaceShapes(restJsonModel, listOf(service))

        shouldThrow<CodegenException> {
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    service = serviceShapeId.toString(),
                    command = {},
                    additionalSettings =
                        ServerAdditionalSettings.builder()
                            .withHttp1x(false)
                            .toObjectNode(),
                ),
                testCoverage = HttpTestType.Default,
            ) { _, _ -> }
        }
    }
}
