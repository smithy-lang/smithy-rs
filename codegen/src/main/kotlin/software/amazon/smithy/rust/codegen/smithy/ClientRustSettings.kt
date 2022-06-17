/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId

/**
 * [ClientRustSettings] and [ClientCodegenConfig] classes.
 *
 * These are specializations of [RustSettings] and [CodegenConfig] for the `rust-codegen` client Smithy plugin. Refer
 * to the documentation of those for the inherited properties.
 */

/**
 * Settings used by [RustCodegenPlugin].
 */
data class ClientRustSettings(
    override val service: ShapeId,
    override val moduleName: String,
    override val moduleVersion: String,
    override val moduleAuthors: List<String>,
    override val moduleDescription: String?,
    override val moduleRepository: String?,
    override val runtimeConfig: RuntimeConfig,
    override val coreCodegenConfig: ClientCodegenConfig,
    override val license: String?,
    override val examplesUri: String? = null,
    override val customizationConfig: ObjectNode? = null
): CoreRustSettings(
    service, moduleName, moduleVersion, moduleAuthors, moduleDescription, moduleRepository, runtimeConfig, coreCodegenConfig, license
) {

    companion object {
        fun from(model: Model, config: ObjectNode): ClientRustSettings {
            val coreRustSettings = CoreRustSettings.from(model, config)
            val codegenSettings = config.getObjectMember(CODEGEN_SETTINGS)
            val coreCodegenConfig = CoreCodegenConfig.fromNode(codegenSettings)
            return ClientRustSettings(
                service = coreRustSettings.service,
                moduleName = coreRustSettings.moduleName,
                moduleVersion = coreRustSettings.moduleVersion,
                moduleAuthors = coreRustSettings.moduleAuthors,
                moduleDescription = coreRustSettings.moduleDescription,
                moduleRepository = coreRustSettings.moduleRepository,
                runtimeConfig = coreRustSettings.runtimeConfig,
                coreCodegenConfig = ClientCodegenConfig.fromCodegenConfigAndNode(coreCodegenConfig, config),
                license = coreRustSettings.license,
                examplesUri = coreRustSettings.examplesUri,
                customizationConfig = coreRustSettings.customizationConfig
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
    override val formatTimeoutSeconds: Int = 20,
    override val debugMode: Boolean = false,
    override val eventStreamAllowList: Set<String> = emptySet(),
    val renameExceptions: Boolean = true,
    val includeFluentClient: Boolean = true,
    val addMessageToErrors: Boolean = true,
): CoreCodegenConfig(
    formatTimeoutSeconds, debugMode, eventStreamAllowList
) {
    companion object {
        fun fromCodegenConfigAndNode(coreCodegenConfig: CoreCodegenConfig, node: ObjectNode) =
            ClientCodegenConfig(
                formatTimeoutSeconds = coreCodegenConfig.formatTimeoutSeconds,
                debugMode = coreCodegenConfig.debugMode,
                eventStreamAllowList = coreCodegenConfig.eventStreamAllowList,
                renameExceptions = node.getBooleanMemberOrDefault("renameErrors", true),
                includeFluentClient = node.getBooleanMemberOrDefault("includeFluentClient", true),
                addMessageToErrors = node.getBooleanMemberOrDefault("addMessageToErrors", true),
            )
    }
}
