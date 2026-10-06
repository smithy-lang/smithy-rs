/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import com.moandjiezana.toml.Toml
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.generators.CargoTomlGenerator
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class ServerSettingsMetadataTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace test
        use aws.protocols#restJson1
        @restJson1
        service TestService { version: "1", operations: [Ping] }
        @http(method: "GET", uri: "/ping")
        operation Ping {}
    """.asSmithyModel()

    private fun metadata(config: String = ""): Toml {
        val settings =
            ServerRustSettings.from(
                model,
                Node.parse(
                    """{
                "module": "test-service", "moduleVersion": "1.0.0", "moduleAuthors": ["test"],
                "service": "test#TestService" $config
            }""",
                ).expectObjectNode(),
            )
        val protocols = listOf("aws.protocols#restXml", "aws.protocols#restJson1", "aws.protocols#awsJson1_0", "aws.protocols#awsJson1_1", ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID).map(ShapeId::from)
        val writer = RustWriter.toml("Cargo.toml")
        CargoTomlGenerator(settings, protocols.first().toString(), writer, settings.manifestSettingsMetadata(protocols), emptyList(), emptyList()).render()
        return Toml().read(writer.toString()).getTable("package.metadata")
    }

    // toml4j retains the quotation marks on quoted table names in toMap().
    private fun Toml.protocolTables(): Map<String, Any> =
        getTable("customizationConfig.protocols")?.toMap().orEmpty().mapKeys { (key, _) -> key.removeSurrounding("\"") }

    @Test
    fun `manifest omits default codegen and protocol settings`() {
        val metadata = metadata()
        metadata.contains("codegen") shouldBe false
        metadata.contains("customizationConfig") shouldBe false
    }

    @Test
    fun `configured flags are recorded and nested customization values keep their types`() {
        val metadata =
            metadata(
                """,
            "codegen": {"schemaSerde": true, "http-1x": true,
                        "addValidationExceptionToConstrainedOperations": false, "rpcV2CborAddCapitalizedRoute": true},
            "customizationConfig": {"protocols": {
                "global": {"requestBodyMaxBytes": 4096},
                "smithy.protocols#rpcv2Cbor": {"capitalizeRoutes": false},
                "aws.protocols#restXml": {"acceptTextXml": true},
                "example#custom": {"nested": {"enabled": true, "limit": 3, "names": ["one", "two"]}}
            }}
        """,
            )
        val codegen = metadata.getTable("codegen")
        codegen.getBoolean("schemaSerde") shouldBe true
        codegen.getBoolean("http-1x") shouldBe true
        codegen.getBoolean("addValidationExceptionToConstrainedOperations") shouldBe false
        codegen.contains("debugMode") shouldBe false
        codegen.toMap().size shouldBe 4
        val protocols = metadata.protocolTables()
        protocols.containsKey(ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID) shouldBe false
        protocols["global"] shouldBe mapOf("requestBodyMaxBytes" to 4096L)
        protocols["aws.protocols#restXml"] shouldBe mapOf("acceptTextXml" to true)
        protocols["example#custom"] shouldBe mapOf("nested" to mapOf("enabled" to true, "limit" to 3L, "names" to listOf("one", "two")))
    }

    @Test
    fun `explicit codegen defaults are omitted`() {
        val metadata =
            metadata(
                """, "codegen": {"debugMode": false, "formatTimeoutSeconds": 20,
                    "publicConstrainedTypes": true}""",
            )
        metadata.contains("codegen") shouldBe false
    }

    @Test
    fun `false overrides of true defaults are recorded`() {
        val metadata = metadata(""", "codegen": {"publicConstrainedTypes": false}""")
        metadata.getTable("codegen").toMap() shouldBe mapOf("publicConstrainedTypes" to false)
    }

    @Test
    fun `explicit protocol defaults are omitted without dropping unknown settings`() {
        val metadata =
            metadata(
                """, "customizationConfig": {"protocols": {
                "aws.protocols#restXml": {"strictCollectionElementNames": true, "validateDocument": false, "acceptTextXml": false},
                "aws.protocols#restJson1": {"validateSkippedValues": false},
                "aws.protocols#awsJson1_0": {"validateSkippedValues": false},
                "aws.protocols#awsJson1_1": {"validateSkippedValues": false},
                "smithy.protocols#rpcv2Cbor": {"capitalizeRoutes": false, "extension": false},
                "example#custom": {"legacyMode": false}
            }}""",
            )
        metadata.protocolTables() shouldBe
            mapOf(
                ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID to mapOf("extension" to false),
                "example#custom" to mapOf("legacyMode" to false),
            )
    }

    @Test
    fun `empty codegen configuration emits no flags`() {
        metadata(""", "codegen": {}""").getTable("codegen")?.toMap().orEmpty() shouldBe emptyMap()
    }

    @Test
    fun `metadata records the legacy route flag folded into customization settings`() {
        val metadata = metadata(""", "codegen": {"rpcV2CborAddCapitalizedRoute": true}""")
        metadata.protocolTables()[ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID] shouldBe mapOf("capitalizeRoutes" to true)
    }

    @Test
    fun `server plugin includes only non-default codegen settings in the generated manifest`() {
        val server =
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings =
                        Node.parse(
                            """{
                "codegen": {"debugMode": false, "publicConstrainedTypes": false},
                "customizationConfig": {"example": {"enabled": true},
                                        "protocols": {"global": {"requestBodyMaxBytes": 1024}}}
            }""",
                        ).expectObjectNode(),
                ),
                testCoverage = HttpTestType.Default,
            ).single()
        val metadata = Toml().read(server.path.resolve("Cargo.toml").toFile()).getTable("package.metadata")
        metadata.getLong("customizationConfig.protocols.global.requestBodyMaxBytes") shouldBe 1024L
        metadata.contains("codegen.schemaSerde") shouldBe false
        metadata.contains("codegen.debugMode") shouldBe false
        metadata.getBoolean("codegen.publicConstrainedTypes") shouldBe false
        metadata.getBoolean("customizationConfig.example.enabled") shouldBe true
    }
}
