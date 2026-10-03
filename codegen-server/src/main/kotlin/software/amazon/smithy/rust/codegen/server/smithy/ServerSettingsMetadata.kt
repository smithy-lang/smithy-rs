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
import software.amazon.smithy.rust.codegen.core.util.deepMergeWith

/**
 * Records effective server settings using their smithy-build.json names. Unset nullable settings
 * are omitted because TOML cannot represent null; in particular, an automatic validation-exception
 * decision must not be recorded as an explicit false override.
 */
internal fun ServerRustSettings.manifestSettingsMetadata(servedProtocols: Collection<ShapeId>): ManifestCustomizations {
    val codegen =
        with(codegenConfig) {
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
                ServerCodegenConfig.REQUEST_BODY_MAX_BYTES_CONFIG_KEY to requestBodyMaxBytes,
                "allowMissingUnionVariant" to allowMissingUnionVariant,
                ServerCodegenConfig.RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE_CONFIG_KEY to rpcV2CborAddCapitalizedRoute,
                ServerCodegenConfig.SCHEMA_SERDE_CONFIG_KEY to schemaSerde,
            ).toMap()
        }
    // Runtime extensions own their defaults. Preserve their supplied settings verbatim and add
    // the defaults known to the built-in server protocols, for protocols served by this crate.
    val protocolDefaults =
        servedProtocols.mapNotNull { protocol ->
            when (protocol.toString()) {
                ServerRustSettings.RPC_V2_CBOR_PROTOCOL_ID -> protocol.toString() to mapOf(ServerRustSettings.CAPITALIZE_ROUTES_KEY to false)
                "aws.protocols#restXml" -> protocol.toString() to mapOf("legacyMode" to false)
                else -> null
            }
        }.toMap()
    val protocols = protocolDefaults.deepMergeWith(protocolSettings().mapValues { (_, section) -> section.metadataMap() })
    val customization =
        (customizationConfig?.metadataMap() ?: emptyMap())
            .deepMergeWith(if (protocols.isEmpty()) emptyMap() else mapOf("protocols" to protocols))
    return mapOf("package" to mapOf("metadata" to mapOf("codegen" to codegen, "customizationConfig" to customization)))
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
