/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.NumberNode
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.Version
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations

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
                                "codegen-release" to Version.fromDefaultResource().codegenVersion,
                            ),
                    ),
            ),
    )

/**
 * Records supplied schema-generation settings that differ from their defaults using their smithy-build.json names.
 * Nullable settings default to automatic: an explicit false is still a non-default override.
 */
internal fun ServerRustSettings.manifestSettingsMetadata(
    servedProtocols: Collection<ShapeId>,
    codegenSettings: ObjectNode,
): ManifestCustomizations {
    val codegen = ServerCodegenConfig.nonDefaultSettings(codegenSettings).metadataMap()
    // Runtime extensions own their defaults. Only remove defaults known to built-in protocols.
    val protocolDefaults =
        servedProtocols.mapNotNull { protocol ->
            when (protocol.toString()) {
                ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID -> protocol.toString() to mapOf(ServerRustSettings.CAPITALIZE_ROUTES_KEY to false)
                "aws.protocols#restXml" -> protocol.toString() to mapOf("strictCollectionElementNames" to true, "validateDocument" to false, "acceptTextXml" to false)
                "aws.protocols#restJson1", "aws.protocols#awsJson1_0", "aws.protocols#awsJson1_1" -> protocol.toString() to mapOf("validateSkippedValues" to false)
                else -> null
            }
        }.toMap()
    val protocols =
        protocolSettings().mapValues { (protocol, section) ->
            section.metadataMap().filter { (key, value) -> value != protocolDefaults[protocol]?.get(key) }
        }.filterValues { it.isNotEmpty() }
    val customization =
        (customizationConfig?.metadataMap().orEmpty() - ServerRustSettings.PROTOCOLS_CUSTOMIZATION_KEY) +
            (if (protocols.isEmpty()) emptyMap() else mapOf(ServerRustSettings.PROTOCOLS_CUSTOMIZATION_KEY to protocols))
    val metadata =
        buildMap {
            // Schema server crates can serve several protocols, so replace the
            // core generator's single-protocol scalar with the served protocol array.
            put("smithy", mapOf("protocol" to servedProtocols.map(ShapeId::toString)))
            if (codegen.isNotEmpty()) put("codegen", codegen)
            if (customization.isNotEmpty()) put("customizationConfig", customization)
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

private fun ObjectNode.metadataMap(): Map<String, Any?> =
    members.entries.mapNotNull { (key, value) -> value.metadataValue()?.let { key.value to it } }.toMap()
