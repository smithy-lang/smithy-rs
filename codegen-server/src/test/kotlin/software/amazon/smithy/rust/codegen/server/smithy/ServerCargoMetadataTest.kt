/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import com.moandjiezana.toml.Toml
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.Version
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.generators.CargoTomlGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.lang.reflect.Modifier

/**
 * Checks the Cargo.toml metadata emitted for generated servers: the codegen commit and release,
 * served protocol IDs, explicitly supplied non-default codegen settings, and customization values.
 * It parses the generated TOML and verifies that nested values retain their types, defaults are
 * omitted, and legacy servers retain their baseline metadata. Reflection checks that the recorded
 * setting defaults cover all server settings and agree with their constructor defaults.
 */
internal class ServerCargoMetadataTest {
    private fun manifest(manifestCustomizations: ManifestCustomizations): Toml {
        val writer = RustWriter.toml("Cargo.toml")
        CargoTomlGenerator(
            moduleName = "test-service",
            moduleVersion = "1.0.0",
            moduleAuthors = listOf("test"),
            moduleDescription = null,
            moduleLicense = null,
            moduleRepository = null,
            minimumSupportedRustVersion = null,
            protocolId = "aws.protocols#restJson1",
            writer = writer,
            manifestCustomizations = manifestCustomizations,
        ).render()
        return Toml().read(writer.toString()).getTable("package.metadata.smithy")
    }

    @Test
    fun `schema manifest adds the codegen release while retaining the commit`() {
        val metadata = manifest(serverCodegenVersionMetadata())
        metadata.getString("codegen-release") shouldBe serverCodegenRelease()
        metadata.getString("codegen-version") shouldBe Version.fromDefaultResource().gitHash
    }

    @Test
    fun `non-server manifest is unchanged`() {
        val metadata = manifest(emptyMap())
        metadata.getString("codegen-version") shouldBe Version.fromDefaultResource().gitHash
        metadata.getString("protocol") shouldBe "aws.protocols#restJson1"
        metadata.contains("codegen-release") shouldBe false
    }

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

    private fun metadata(
        config: String = "",
        servedProtocols: List<String>? = null,
        transform: (ServerCodegenConfig) -> ServerCodegenConfig = { it },
    ): Toml {
        val suppliedConfig =
            Node.parse(
                """{
                "module": "test-service", "moduleVersion": "1.0.0", "moduleAuthors": ["test"],
                "service": "test#TestService" $config
            }""",
            ).expectObjectNode()
        val settings = ServerRustSettings.from(model, suppliedConfig)
        val effectiveSettings = settings.copy(codegenConfig = transform(settings.codegenConfig))
        val protocols =
            (
                servedProtocols
                    ?: listOf("aws.protocols#restXml", "aws.protocols#restJson1", "aws.protocols#awsJson1_0", "aws.protocols#awsJson1_1", ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID)
            ).map(ShapeId::from)
        val writer = RustWriter.toml("Cargo.toml")
        CargoTomlGenerator(effectiveSettings, protocols.first().toString(), writer, effectiveSettings.manifestSettingsMetadata(protocols, suppliedConfig.getObjectMember("codegen").orElse(ObjectNode.objectNode())), emptyList(), emptyList()).render()
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
    fun `multi-protocol services list every served protocol`() {
        val metadata =
            metadata(servedProtocols = listOf("aws.protocols#restJson1", ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID))
        metadata.getList<String>("smithy.protocol") shouldBe
            listOf("aws.protocols#restJson1", ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID)
    }

    @Test
    fun `single-protocol services record a one-element protocol array`() {
        val metadata = metadata(servedProtocols = listOf("aws.protocols#restJson1"))
        metadata.getList<String>("smithy.protocol") shouldBe listOf("aws.protocols#restJson1")
    }

    @Test
    fun `configured flags are recorded and nested customization values keep their types`() {
        val metadata =
            metadata(
                """,
                "codegen": {"schemaSerde": true, "http-1x": true,
                            "addValidationExceptionToConstrainedOperations": false, "rpcV2CborAddCapitalizedRoute": true},
                "customizationConfig": {"protocols": {
                    "common": {"request": {"body": {"maxBytes": 4096}}},
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
        protocols["common"] shouldBe mapOf("request" to mapOf("body" to mapOf("maxBytes" to 4096L)))
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
                """, "codegen": {"schemaSerde": true}, "customizationConfig": {"protocols": {
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
        val metadata = metadata(""", "codegen": {"schemaSerde": true, "rpcV2CborAddCapitalizedRoute": true}""")
        metadata.protocolTables()[ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID] shouldBe mapOf("capitalizeRoutes" to true)
    }

    @Test
    fun `schema server plugin includes only non-default codegen settings in the generated manifest`() {
        val server =
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings =
                        Node.parse(
                            """{
                            "codegen": {"schemaSerde": true, "http-1x": true,
                                        "debugMode": false, "publicConstrainedTypes": false, "formatTimeoutSeconds": 20.0},
                            "customizationConfig": {"example": {"enabled": true},
                                                    "protocols": {"common": {"request": {"body": {"maxBytes": 1024}}}}}
            }""",
                        ).expectObjectNode(),
                ),
                testCoverage = HttpTestType.Default,
            ).single()
        val metadata = Toml().read(server.path.resolve("Cargo.toml").toFile()).getTable("package.metadata")
        metadata.getLong("customizationConfig.protocols.common.request.body.maxBytes") shouldBe 1024L
        metadata.getBoolean("codegen.schemaSerde") shouldBe true
        metadata.getBoolean("codegen.http-1x") shouldBe true
        metadata.getList<String>("smithy.protocol") shouldBe listOf("aws.protocols#restJson1")
        metadata.getString("smithy.codegen-release") shouldBe serverCodegenRelease()
        metadata.getString("smithy.codegen-version") shouldBe Version.fromDefaultResource().gitHash
        metadata.contains("codegen.debugMode") shouldBe false
        metadata.contains("codegen.formatTimeoutSeconds") shouldBe false
        metadata.getBoolean("codegen.publicConstrainedTypes") shouldBe false
        metadata.getBoolean("customizationConfig.example.enabled") shouldBe true
    }

    @Test
    fun `legacy server plugin retains baseline metadata for omitted and disabled schema serde`() {
        for ((http1x, schemaSerde) in listOf(false to null, false to false, true to null, true to false)) {
            var codegen =
                Node.objectNode()
                    .withMember("http-1x", http1x)
                    .withMember("publicConstrainedTypes", false)
            if (schemaSerde != null) {
                codegen = codegen.withMember("schemaSerde", schemaSerde)
            }
            val server =
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings =
                            Node.objectNode().withMember("codegen", codegen).withMember(
                                "customizationConfig",
                                Node.parse(
                                    """{"example": {"enabled": true}, "protocols": {"common": {"request": {"body": {"maxBytes": 1024}}}}}""",
                                ),
                            ),
                        cargoCommand = "cargo test --quiet --lib --tests",
                    ),
                    testCoverage = HttpTestType.Default,
                ).single()
            val metadata = Toml().read(server.path.resolve("Cargo.toml").toFile()).getTable("package.metadata")
            metadata.toMap() shouldBe
                mapOf(
                    "smithy" to
                        mapOf(
                            "codegen-version" to Version.fromDefaultResource().gitHash,
                            "protocol" to "aws.protocols#restJson1",
                        ),
                )
        }
    }

    @Test
    fun `unknown settings are not recorded as effective settings`() {
        val metadata = metadata(""", "codegen": {"futureSetting": "someValue"}""")
        metadata.contains("codegen") shouldBe false
    }

    @Test
    fun `inherited and nullable settings are recorded`() {
        val metadata =
            metadata(
                """, "codegen": {
                "formatTimeoutSeconds": 60,
                "addValidationExceptionToConstrainedOperations": false,
                "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse": "test#ValidationException"
                }""",
            )
        metadata.getTable("codegen").toMap() shouldBe
            mapOf(
                "formatTimeoutSeconds" to 60L,
                "addValidationExceptionToConstrainedOperations" to false,
                "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse" to "test#ValidationException",
            )
    }

    @Test
    fun `metadata records supplied values rather than programmatic overrides`() {
        val metadata =
            metadata(""", "codegen": {"debugMode": true}""") {
                it.copy(debugMode = false, schemaSerde = true)
            }
        metadata.getTable("codegen").toMap() shouldBe mapOf("debugMode" to true)
    }

    @Test
    fun `default map covers every server setting and matches constructor defaults`() {
        val defaults =
            ServerCodegenConfig::class.java.getDeclaredField("KNOWN_CONFIG_KEYS")
                .apply { isAccessible = true }
                .get(null) as Map<*, *>
        val defaultConfig = ServerCodegenConfig()
        val settingFields =
            ServerCodegenConfig::class.java.declaredFields
                .filterNot { Modifier.isStatic(it.modifiers) || it.isSynthetic }
                .associateBy { it.name }
        val defaultProperties =
            defaults.keys.map {
                if (it == ServerCodegenConfig.HTTP_1X_CONFIG_KEY) "http1x" else it
            }.toSet()
        defaultProperties shouldBe settingFields.keys
        defaults.forEach { (key, wrapper) ->
            val property = if (key == ServerCodegenConfig.HTTP_1X_CONFIG_KEY) "http1x" else key as String
            val actualDefault = settingFields.getValue(property).apply { isAccessible = true }.get(defaultConfig)
            val recordedDefault = wrapper!!.javaClass.getDeclaredField("value").apply { isAccessible = true }.get(wrapper)
            recordedDefault shouldBe actualDefault
        }
    }

    @Test
    fun `core collection accessor setting remains unsupported by the server`() {
        val metadata =
            metadata(""", "codegen": {"flattenCollectionAccessors": true}""") {
                it.flattenCollectionAccessors shouldBe false
                it
            }
        metadata.contains("codegen") shouldBe false
    }

    @Test
    fun `numerically equivalent defaults are omitted`() {
        for (value in listOf("20", "20.0", "2e1")) {
            metadata(""", "codegen": {"formatTimeoutSeconds": $value}""").contains("codegen") shouldBe false
        }
    }

    @Test
    fun `both boolean overrides of a null default are recorded`() {
        for (value in listOf(false, true)) {
            val metadata = metadata(""", "codegen": {"addValidationExceptionToConstrainedOperations": $value}""")
            metadata.getTable("codegen").toMap() shouldBe mapOf("addValidationExceptionToConstrainedOperations" to value)
        }
    }
}
