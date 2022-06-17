/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId

/**
 * [ServerRustSettings] and [ServerCodegenConfig] classes.
 *
 * These classes are entirely analogous to [ClientRustSettings] and [ClientCodegenConfig]. Refer to the documentation
 * for those.
 *
 * These classes have to live in the `codegen` subproject because they are referenced in [ServerCodegenContext],
 * which is used in common generators to both client and server.
 */

/**
 * Settings used by [RustCodegenServerPlugin].
 */
data class ServerRustSettings(
    override val service: ShapeId,
    override val moduleName: String,
    override val moduleVersion: String,
    override val moduleAuthors: List<String>,
    override val moduleDescription: String?,
    override val moduleRepository: String?,
    override val runtimeConfig: RuntimeConfig,
    override val coreCodegenConfig: ServerCodegenConfig,
    override val license: String?,
    override val examplesUri: String?,
    override val customizationConfig: ObjectNode?
) : CoreRustSettings(
    service,
    moduleName,
    moduleVersion,
    moduleAuthors,
    moduleDescription,
    moduleRepository,
    runtimeConfig,
    coreCodegenConfig,
    license,
    examplesUri,
    customizationConfig
) {
    companion object {
        fun from(model: Model, config: ObjectNode): ServerRustSettings {
            val coreRustSettings = CoreRustSettings.from(model, config)
            val codegenSettings = config.getObjectMember(CODEGEN_SETTINGS)
            val coreCodegenConfig = CoreCodegenConfig.fromNode(codegenSettings)
            return ServerRustSettings(
                service = coreRustSettings.service,
                moduleName = coreRustSettings.moduleName,
                moduleVersion = coreRustSettings.moduleVersion,
                moduleAuthors = coreRustSettings.moduleAuthors,
                moduleDescription = coreRustSettings.moduleDescription,
                moduleRepository = coreRustSettings.moduleRepository,
                runtimeConfig = coreRustSettings.runtimeConfig,
                coreCodegenConfig = ServerCodegenConfig.fromCodegenConfigAndNode(coreCodegenConfig, config),
                license = coreRustSettings.license,
                examplesUri = coreRustSettings.examplesUri,
                customizationConfig = coreRustSettings.customizationConfig
            )
        }
    }
}

data class ServerCodegenConfig(
    override val formatTimeoutSeconds: Int,
    override val debugMode: Boolean,
    override val eventStreamAllowList: Set<String>,
) : CoreCodegenConfig(
    formatTimeoutSeconds, debugMode, eventStreamAllowList
) {
    companion object {
        // Note `node` is unused, because at the moment `ServerCodegenConfig` has the same properties as
        // `CodegenConfig`. In the future, the server will have server-specific codegen options just like the client
        // does.
        fun fromCodegenConfigAndNode(coreCodegenConfig: CoreCodegenConfig, node: ObjectNode) =
            ServerCodegenConfig(
                formatTimeoutSeconds = coreCodegenConfig.formatTimeoutSeconds,
                debugMode = coreCodegenConfig.debugMode,
                eventStreamAllowList = coreCodegenConfig.eventStreamAllowList,
            )
    }
}
