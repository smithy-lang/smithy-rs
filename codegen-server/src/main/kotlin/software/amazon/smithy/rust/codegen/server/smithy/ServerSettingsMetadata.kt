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
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations

/**
 * Records settings that differ from their defaults using their smithy-build.json names.
 * Nullable settings default to automatic: an explicit false is still a non-default override.
 */
internal fun ServerRustSettings.manifestSettingsMetadata(servedProtocols: Collection<ShapeId>): ManifestCustomizations {
    val codegenDefaults = ServerCodegenConfig().metadataMap()
    val codegen = codegenConfig.metadataMap().filter { (key, value) -> value != codegenDefaults[key] }
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
            if (codegen.isNotEmpty()) put("codegen", codegen)
            if (customization.isNotEmpty()) put("customizationConfig", customization)
        }
    return mapOf("package" to mapOf("metadata" to metadata))
}

private fun ServerCodegenConfig.metadataMap(): Map<String, Any> =
    listOfNotNull(
        "formatTimeoutSeconds" to formatTimeoutSeconds,
        "debugMode" to debugMode,
        "flattenCollectionAccessors" to flattenCollectionAccessors,
        "publicConstrainedTypes" to publicConstrainedTypes,
        "ignoreUnsupportedConstraints" to ignoreUnsupportedConstraints,
        experimentalCustomValidationExceptionWithReasonPleaseDoNotUse?.let {
            "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse" to it
        },
        addValidationExceptionToConstrainedOperations?.let { "addValidationExceptionToConstrainedOperations" to it },
        "alwaysSendEventStreamInitialResponse" to alwaysSendEventStreamInitialResponse,
        ServerCodegenConfig.HTTP_1X_CONFIG_KEY to http1x,
        "allowMissingUnionVariant" to allowMissingUnionVariant,
        ServerCodegenConfig.RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE_CONFIG_KEY to rpcV2CborAddCapitalizedRoute,
        ServerCodegenConfig.SCHEMA_SERDE_CONFIG_KEY to schemaSerde,
    ).toMap()

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
