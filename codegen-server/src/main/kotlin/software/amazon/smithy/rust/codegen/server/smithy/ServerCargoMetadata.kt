/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.NumberNode
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations

internal fun serverCodegenRelease(): String =
    ServerRustSettings::class.java.getResource("codegen-release.txt")?.readText()
        ?: throw CodegenException("codegen-release.txt does not exist")

/**
 * Records the schema code generator's artifact version in `package.metadata.smithy.codegen-release`.
 * The source commit remains in `codegen-version`, written by the core manifest generator.
 */
internal fun serverCodegenVersionMetadata(): ManifestCustomizations =
    mapOf(
        "package" to
            mapOf(
                "metadata" to
                    mapOf(
                        "smithy" to
                            mapOf(
                                "codegen-release" to serverCodegenRelease(),
                            ),
                    ),
            ),
    )

/** SDK API and compatibility choices recorded in Cargo metadata, independently of accepted configuration keys. */
private val CODEGEN_METADATA_KEYS =
    setOf(
        ServerCodegenConfig.SCHEMA_SERDE_CONFIG_KEY,
        ServerCodegenConfig.HTTP_1X_CONFIG_KEY,
        "publicConstrainedTypes",
        "ignoreUnsupportedConstraints",
        "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse",
        "addValidationExceptionToConstrainedOperations",
        "alwaysSendEventStreamInitialResponse",
        "allowMissingUnionVariant",
    )

/** Per-protocol metadata allowlists and their defaults. Operational settings are intentionally excluded. */
private val PROTOCOL_METADATA_DEFAULTS =
    mapOf(
        "aws.protocols#restJson1" to mapOf("validateSkippedValues" to false),
        "aws.protocols#restXml" to
            mapOf(
                "strictCollectionElementNames" to true,
                "validateDocument" to false,
                "acceptTextXml" to false,
            ),
        "aws.protocols#awsJson1_0" to
            mapOf(
                ServerRustSettings.CAPITALIZE_ROUTES_KEY to false,
                "responseBindings" to AwsJsonResponseBindings.HISTORICAL.setting,
                "validateSkippedValues" to false,
            ),
        "aws.protocols#awsJson1_1" to
            mapOf(
                ServerRustSettings.CAPITALIZE_ROUTES_KEY to false,
                "responseBindings" to AwsJsonResponseBindings.HISTORICAL.setting,
                "validateSkippedValues" to false,
            ),
        ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID to
            mapOf(
                ServerRustSettings.CAPITALIZE_ROUTES_KEY to false,
                "methodNotAllowedAsNotFound" to false,
            ),
    )

/**
 * Records allowlisted SDK API and compatibility settings that differ from their defaults.
 * Nullable settings default to automatic: an explicit false is still a non-default override.
 * Route aliases use normalized protocol settings after applying legacy flags and precedence.
 * Timeouts, resource limits, tooling settings, and arbitrary customization values are omitted.
 */
internal fun ServerRustSettings.manifestSettingsMetadata(
    servedProtocols: Collection<ShapeId>,
    codegenSettings: ObjectNode,
): ManifestCustomizations {
    val codegen = ServerCodegenConfig.nonDefaultSettings(codegenSettings).metadataMap(CODEGEN_METADATA_KEYS)
    val settings = protocolSettings()
    val protocols =
        servedProtocols.mapNotNull { protocol ->
            val id = protocol.toString()
            val defaults = PROTOCOL_METADATA_DEFAULTS[id] ?: return@mapNotNull null
            val values =
                settings[id]?.metadataMap(defaults.keys)
                    ?.filter { (key, value) -> value != defaults[key] }
                    .orEmpty()
            if (values.isEmpty()) {
                null
            } else {
                id to values
            }
        }.toMap()
    val metadata =
        buildMap {
            // Schema server crates can serve several protocols, so replace the
            // core generator's single-protocol scalar with the served protocol array.
            put("smithy", mapOf("protocol" to servedProtocols.map(ShapeId::toString)))
            if (codegen.isNotEmpty()) {
                put("codegen", codegen)
            }
            if (protocols.isNotEmpty()) {
                put("customizationConfig", mapOf(ServerRustSettings.PROTOCOLS_CUSTOMIZATION_KEY to protocols))
            }
        }
    return mapOf("package" to mapOf("metadata" to metadata))
}

/** Keep booleans, numbers, arrays and nested objects typed in TOML. Null object members are unset. */
private fun Node.metadataValue(): Any? =
    when (this) {
        is ObjectNode -> metadataMap()
        is ArrayNode ->
            elements.map { element ->
                requireNotNull(element.metadataValue()) { "Cargo metadata cannot represent a null customizationConfig array element" }
            }
        is BooleanNode -> value
        is NumberNode -> value
        is StringNode -> value
        else -> null
    }

private fun ObjectNode.metadataMap(allowedKeys: Set<String>? = null): Map<String, Any?> =
    members.entries
        .filter { (key, _) -> allowedKeys == null || key.value in allowedKeys }
        .mapNotNull { (key, value) -> value.metadataValue()?.let { key.value to it } }.toMap()
