/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.CODEGEN_SETTINGS
import software.amazon.smithy.rust.codegen.core.smithy.CoreCodegenConfig
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import java.util.Optional

/*
 * [ClientRustSettings] and [ClientCodegenConfig] classes.
 *
 * These are specializations of [CoreRustSettings] and [CodegenConfig] for the `rust-client-codegen`
 * client Smithy plugin. Refer to the documentation of those for the inherited properties.
 */

/**
 * Settings used by [RustClientCodegenPlugin].
 */
data class ClientRustSettings(
    override val service: ShapeId,
    override val moduleName: String,
    override val moduleVersion: String,
    override val moduleAuthors: List<String>,
    override val moduleDescription: String?,
    override val moduleRepository: String?,
    override val runtimeConfig: RuntimeConfig,
    override val codegenConfig: ClientCodegenConfig,
    override val license: String?,
    override val examplesUri: String?,
    override val minimumSupportedRustVersion: String? = null,
    override val hintMostlyUnused: Boolean = true,
    override val customizationConfig: ObjectNode?,
) : CoreRustSettings(
        service,
        moduleName,
        moduleVersion,
        moduleAuthors,
        moduleDescription,
        moduleRepository,
        runtimeConfig,
        codegenConfig,
        license,
        examplesUri,
        minimumSupportedRustVersion,
        hintMostlyUnused,
        customizationConfig,
    ) {
    companion object {
        fun from(
            model: Model,
            config: ObjectNode,
        ): ClientRustSettings {
            val coreRustSettings = CoreRustSettings.from(model, config)
            val codegenSettingsNode = config.getObjectMember(CODEGEN_SETTINGS)
            val coreCodegenConfig = CoreCodegenConfig.fromNode(codegenSettingsNode)
            return ClientRustSettings(
                service = coreRustSettings.service,
                moduleName = coreRustSettings.moduleName,
                moduleVersion = coreRustSettings.moduleVersion,
                moduleAuthors = coreRustSettings.moduleAuthors,
                moduleDescription = coreRustSettings.moduleDescription,
                moduleRepository = coreRustSettings.moduleRepository,
                runtimeConfig = coreRustSettings.runtimeConfig,
                codegenConfig = ClientCodegenConfig.fromCodegenConfigAndNode(coreCodegenConfig, codegenSettingsNode),
                license = coreRustSettings.license,
                examplesUri = coreRustSettings.examplesUri,
                minimumSupportedRustVersion = coreRustSettings.minimumSupportedRustVersion,
                hintMostlyUnused = coreRustSettings.hintMostlyUnused,
                customizationConfig = coreRustSettings.customizationConfig,
            )
        }
    }
}

/**
 * [renameExceptions]: Rename `Exception` to `Error` in the generated SDK
 * [includeFluentClient]: Generate a `client` module in the generated SDK (currently the AWS SDK sets this to `false`
 *   and generates its own client)
 * [addMessageToErrors]: Adds a `message` field automatically to all error shapes
 */
data class ClientCodegenConfig(
    override val formatTimeoutSeconds: Int = DEFAULT_FORMAT_TIMEOUT_SECONDS,
    override val debugMode: Boolean = DEFAULT_DEBUG_MODE,
    override val flattenCollectionAccessors: Boolean = DEFAULT_FLATTEN_ACCESSORS,
    val nullabilityCheckMode: NullableIndex.CheckMode = NullableIndex.CheckMode.CLIENT,
    val renameExceptions: Boolean = DEFAULT_RENAME_EXCEPTIONS,
    val includeFluentClient: Boolean = DEFAULT_INCLUDE_FLUENT_CLIENT,
    val addMessageToErrors: Boolean = DEFAULT_ADD_MESSAGE_TO_ERRORS,
    /** If true, adds `endpoint_url`/`set_endpoint_url` methods to the service config */
    val includeEndpointUrlConfig: Boolean = DEFAULT_INCLUDE_ENDPOINT_URL_CONFIG,
    val enableUserConfigurableRuntimePlugins: Boolean = DEFAULT_ENABLE_USER_CONFIGURABLE_RUNTIME_PLUGINS,
) : CoreCodegenConfig(
        formatTimeoutSeconds, debugMode, DEFAULT_FLATTEN_ACCESSORS,
    ) {
    companion object {
        private const val DEFAULT_RENAME_EXCEPTIONS = true
        private const val DEFAULT_INCLUDE_FLUENT_CLIENT = true
        private const val DEFAULT_ADD_MESSAGE_TO_ERRORS = true
        private const val DEFAULT_INCLUDE_ENDPOINT_URL_CONFIG = true
        private const val DEFAULT_ENABLE_USER_CONFIGURABLE_RUNTIME_PLUGINS = true
        private const val DEFAULT_NULLABILITY_CHECK_MODE = "CLIENT"

        // Note: only clients default to true, servers default to false
        private const val DEFAULT_FLATTEN_ACCESSORS = true

        fun fromCodegenConfigAndNode(
            coreCodegenConfig: CoreCodegenConfig,
            node: Optional<ObjectNode>,
        ) = if (node.isPresent) {
            ClientCodegenConfig(
                formatTimeoutSeconds = coreCodegenConfig.formatTimeoutSeconds,
                flattenCollectionAccessors = node.get().getBooleanMemberOrDefault("flattenCollectionAccessors", DEFAULT_FLATTEN_ACCESSORS),
                debugMode = coreCodegenConfig.debugMode,
                renameExceptions = node.get().getBooleanMemberOrDefault("renameErrors", DEFAULT_RENAME_EXCEPTIONS),
                includeFluentClient = node.get().getBooleanMemberOrDefault("includeFluentClient", DEFAULT_INCLUDE_FLUENT_CLIENT),
                addMessageToErrors = node.get().getBooleanMemberOrDefault("addMessageToErrors", DEFAULT_ADD_MESSAGE_TO_ERRORS),
                includeEndpointUrlConfig = node.get().getBooleanMemberOrDefault("includeEndpointUrlConfig", DEFAULT_INCLUDE_ENDPOINT_URL_CONFIG),
                enableUserConfigurableRuntimePlugins = node.get().getBooleanMemberOrDefault("enableUserConfigurableRuntimePlugins", DEFAULT_ENABLE_USER_CONFIGURABLE_RUNTIME_PLUGINS),
                nullabilityCheckMode = NullableIndex.CheckMode.valueOf(node.get().getStringMemberOrDefault("nullabilityCheckMode", DEFAULT_NULLABILITY_CHECK_MODE)),
            )
        } else {
            ClientCodegenConfig(
                formatTimeoutSeconds = coreCodegenConfig.formatTimeoutSeconds,
                debugMode = coreCodegenConfig.debugMode,
                nullabilityCheckMode = NullableIndex.CheckMode.valueOf(DEFAULT_NULLABILITY_CHECK_MODE),
            )
        }
    }
}
