/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import com.moandjiezana.toml.Toml
import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
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
 * served protocol IDs, and allowlisted non-default SDK API and compatibility settings.
 * It parses the generated TOML and verifies that operational and arbitrary customization settings
 * are omitted, route aliases are normalized, and legacy servers retain their baseline metadata.
 * Reflection checks that the accepted configuration defaults agree with constructor defaults.
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
    fun `only allowlisted codegen and protocol settings are recorded`() {
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
        codegen.toMap().size shouldBe 3
        val protocols = metadata.protocolTables()
        protocols shouldBe mapOf("aws.protocols#restXml" to mapOf("acceptTextXml" to true))
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
    fun `SDK API and generated compatibility choices are recorded`() {
        val metadata =
            metadata(
                """, "codegen": {
                "schemaSerde": true, "http-1x": true,
                "publicConstrainedTypes": false, "ignoreUnsupportedConstraints": true,
                "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse": "test#ValidationException",
                "addValidationExceptionToConstrainedOperations": false,
                "alwaysSendEventStreamInitialResponse": true, "allowMissingUnionVariant": true
            }""",
            )
        metadata.getTable("codegen").toMap() shouldBe
            mapOf(
                "schemaSerde" to true,
                "http-1x" to true,
                "publicConstrainedTypes" to false,
                "ignoreUnsupportedConstraints" to true,
                "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse" to "test#ValidationException",
                "addValidationExceptionToConstrainedOperations" to false,
                "alwaysSendEventStreamInitialResponse" to true,
                "allowMissingUnionVariant" to true,
            )
    }

    @Test
    fun `explicit protocol defaults and unknown settings are omitted`() {
        val metadata =
            metadata(
                """, "codegen": {"schemaSerde": true}, "customizationConfig": {"protocols": {
                "aws.protocols#restXml": {"strictCollectionElementNames": true, "validateDocument": false, "acceptTextXml": false},
                "aws.protocols#restJson1": {"validateSkippedValues": false},
                "aws.protocols#awsJson1_0": {"validateSkippedValues": false, "capitalizeRoutes": false, "responseBindings": "historical"},
                "aws.protocols#awsJson1_1": {"validateSkippedValues": false, "capitalizeRoutes": false, "responseBindings": "historical"},
                "smithy.protocols#rpcv2Cbor": {"capitalizeRoutes": false, "methodNotAllowedAsNotFound": false, "extension": false},
                "example#custom": {"legacyMode": false}
            }}""",
            )
        metadata.contains("customizationConfig") shouldBe false
    }

    @Test
    fun `non-default compatibility settings are recorded for each served protocol`() {
        val metadata =
            metadata(
                """, "codegen": {"schemaSerde": true}, "customizationConfig": {"protocols": {
                "aws.protocols#restXml": {"strictCollectionElementNames": false, "validateDocument": true, "acceptTextXml": true},
                "aws.protocols#restJson1": {"validateSkippedValues": true},
                "aws.protocols#awsJson1_0": {"validateSkippedValues": true, "capitalizeRoutes": true, "responseBindings": "smithy-specs"},
                "aws.protocols#awsJson1_1": {"validateSkippedValues": true, "capitalizeRoutes": true, "responseBindings": "smithy-specs"},
                "smithy.protocols#rpcv2Cbor": {"capitalizeRoutes": true, "methodNotAllowedAsNotFound": true}
            }}""",
            )
        metadata.protocolTables() shouldBe
            mapOf(
                "aws.protocols#restXml" to mapOf("strictCollectionElementNames" to false, "validateDocument" to true, "acceptTextXml" to true),
                "aws.protocols#restJson1" to mapOf("validateSkippedValues" to true),
                "aws.protocols#awsJson1_0" to mapOf("validateSkippedValues" to true, "capitalizeRoutes" to true, "responseBindings" to "smithy-specs"),
                "aws.protocols#awsJson1_1" to mapOf("validateSkippedValues" to true, "capitalizeRoutes" to true, "responseBindings" to "smithy-specs"),
                ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID to mapOf("capitalizeRoutes" to true, "methodNotAllowedAsNotFound" to true),
            )
    }

    @Test
    fun `settings for unserved protocols are omitted`() {
        val metadata =
            metadata(
                """, "customizationConfig": {"protocols": {
                "aws.protocols#restJson1": {"validateSkippedValues": true},
                "aws.protocols#restXml": {"acceptTextXml": true},
                "smithy.protocols#rpcv2Cbor": {"capitalizeRoutes": true}
            }}""",
                servedProtocols = listOf("aws.protocols#restJson1"),
            )
        metadata.protocolTables() shouldBe mapOf("aws.protocols#restJson1" to mapOf("validateSkippedValues" to true))
    }

    @Test
    fun `empty codegen configuration emits no flags`() {
        metadata(""", "codegen": {}""").getTable("codegen")?.toMap().orEmpty() shouldBe emptyMap()
    }

    @Test
    fun `metadata records the normalized legacy route flag once`() {
        val metadata = metadata(""", "codegen": {"schemaSerde": true, "rpcV2CborAddCapitalizedRoute": true}""")
        metadata.protocolTables()[ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID] shouldBe mapOf("capitalizeRoutes" to true)
        metadata.contains("codegen.rpcV2CborAddCapitalizedRoute") shouldBe false
    }

    @Test
    fun `explicit protocol route setting overrides the legacy flag in metadata`() {
        val metadata =
            metadata(
                """, "codegen": {"schemaSerde": true, "rpcV2CborAddCapitalizedRoute": true},
                "customizationConfig": {"protocols": {"smithy.protocols#rpcv2Cbor": {"capitalizeRoutes": false}}}""",
            )
        metadata.contains("codegen.rpcV2CborAddCapitalizedRoute") shouldBe false
        metadata.contains("customizationConfig") shouldBe false
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
        metadata.contains("customizationConfig") shouldBe false
        metadata.getBoolean("codegen.schemaSerde") shouldBe true
        metadata.getBoolean("codegen.http-1x") shouldBe true
        metadata.getList<String>("smithy.protocol") shouldBe listOf("aws.protocols#restJson1")
        metadata.getString("smithy.codegen-release") shouldBe serverCodegenRelease()
        metadata.getString("smithy.codegen-version") shouldBe Version.fromDefaultResource().gitHash
        metadata.contains("codegen.debugMode") shouldBe false
        metadata.contains("codegen.formatTimeoutSeconds") shouldBe false
        metadata.getBoolean("codegen.publicConstrainedTypes") shouldBe false
        server.path.resolve("src/service_config.rs").toFile().readText() shouldContain "NonZeroUsize::new(1024usize)"
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
    fun `nullable codegen compatibility settings are recorded`() {
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
                "addValidationExceptionToConstrainedOperations" to false,
                "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse" to "test#ValidationException",
            )
    }

    @Test
    fun `metadata records supplied values rather than programmatic overrides`() {
        val metadata =
            metadata(""", "codegen": {"publicConstrainedTypes": false}""") {
                it.copy(publicConstrainedTypes = true, schemaSerde = true)
            }
        metadata.getTable("codegen").toMap() shouldBe mapOf("publicConstrainedTypes" to false)
    }

    @Test
    fun `body limits are omitted regardless of their value`() {
        metadata().contains("codegen.requestBodyMaxBytes") shouldBe false
        for (value in listOf(0, 4096)) {
            val metadata = metadata(""", "codegen": {"requestBodyMaxBytes": $value}""")
            metadata.contains("codegen") shouldBe false
            metadata.contains("customizationConfig") shouldBe false
        }
    }

    @Test
    fun `operational settings tooling and arbitrary customizations are omitted`() {
        val metadata =
            metadata(
                """, "codegen": {"schemaSerde": true, "formatTimeoutSeconds": 60, "debugMode": true},
                "customizationConfig": {
                    "example": {"values": [null]},
                    "protocols": {
                        "common": {"request": {
                            "header": {"readTimeout": "1s"},
                            "body": {
                                "maxBytes": "4 KiB", "readPollBudget": 8,
                                "readTimeouts": {"defaultNonPayload": "2s", "defaultPayload": "3s", "perOperation": {"test#Ping": "4s"}},
                                "eventStreamTimeouts": {"initialMessage": "1s", "messageCompletion": "2s", "idle": "3s"}
                            }
                        }},
                        "aws.protocols#restJson1": {"extension": {"values": [null]}},
                        "example#custom": {"enabled": true}
                    }
                }""",
            )
        metadata.getTable("codegen").toMap() shouldBe mapOf("schemaSerde" to true)
        metadata.contains("customizationConfig") shouldBe false
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
                if (it == ServerCodegenConfig.HTTP_1X_CONFIG_KEY) {
                    "http1x"
                } else {
                    it
                }
            }.toSet()
        defaultProperties shouldBe settingFields.keys
        defaults.forEach { (key, wrapper) ->
            val property =
                if (key == ServerCodegenConfig.HTTP_1X_CONFIG_KEY) {
                    "http1x"
                } else {
                    key as String
                }
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
    fun `both boolean overrides of a null default are recorded`() {
        for (value in listOf(false, true)) {
            val metadata = metadata(""", "codegen": {"addValidationExceptionToConstrainedOperations": $value}""")
            metadata.getTable("codegen").toMap() shouldBe mapOf("addValidationExceptionToConstrainedOperations" to value)
        }
    }
}
