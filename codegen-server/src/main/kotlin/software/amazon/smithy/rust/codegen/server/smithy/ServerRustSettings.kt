/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.CODEGEN_SETTINGS
import software.amazon.smithy.rust.codegen.core.smithy.CoreCodegenConfig
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import java.util.Optional

/*
 * [ServerRustSettings] and [ServerCodegenConfig] classes.
 *
 * These classes are entirely analogous to [ClientRustSettings] and [ClientCodegenConfig]. Refer to the documentation
 * for those.
 *
 * These classes have to live in the `codegen` subproject because they are referenced in [ServerCodegenContext],
 * which is used in common generators to both client and server.
 */

/**
 * Settings used by [RustServerCodegenPlugin].
 */
data class ServerRustSettings(
    override val service: ShapeId,
    override val moduleName: String,
    override val moduleVersion: String,
    override val moduleAuthors: List<String>,
    override val moduleDescription: String?,
    override val moduleRepository: String?,
    override val runtimeConfig: RuntimeConfig,
    override val codegenConfig: ServerCodegenConfig,
    override val license: String?,
    override val examplesUri: String?,
    override val minimumSupportedRustVersion: String? = null,
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
        customizationConfig,
    ) {
    companion object {
        fun from(
            model: Model,
            config: ObjectNode,
        ): ServerRustSettings {
            val coreRustSettings = CoreRustSettings.from(model, config)
            val codegenSettingsNode = config.getObjectMember(CODEGEN_SETTINGS)
            val coreCodegenConfig = CoreCodegenConfig.fromNode(codegenSettingsNode)
            return ServerRustSettings(
                service = coreRustSettings.service,
                moduleName = coreRustSettings.moduleName,
                moduleVersion = coreRustSettings.moduleVersion,
                moduleAuthors = coreRustSettings.moduleAuthors,
                moduleDescription = coreRustSettings.moduleDescription,
                moduleRepository = coreRustSettings.moduleRepository,
                runtimeConfig = coreRustSettings.runtimeConfig,
                codegenConfig = ServerCodegenConfig.fromCodegenConfigAndNode(coreCodegenConfig, codegenSettingsNode),
                license = coreRustSettings.license,
                examplesUri = coreRustSettings.examplesUri,
                minimumSupportedRustVersion = coreRustSettings.minimumSupportedRustVersion,
                customizationConfig = coreRustSettings.customizationConfig,
            )
        }
    }
}

/**
 * [publicConstrainedTypes]: Generate constrained wrapper newtypes for constrained shapes
 * [ignoreUnsupportedConstraints]: Generate model even though unsupported constraints are present
 */
data class ServerCodegenConfig(
    override val formatTimeoutSeconds: Int = DEFAULT_FORMAT_TIMEOUT_SECONDS,
    override val debugMode: Boolean = DEFAULT_DEBUG_MODE,
    val publicConstrainedTypes: Boolean = DEFAULT_PUBLIC_CONSTRAINED_TYPES,
    val ignoreUnsupportedConstraints: Boolean = DEFAULT_IGNORE_UNSUPPORTED_CONSTRAINTS,
    /**
     * A flag to enable _experimental_ support for custom validation exceptions via the
     * [CustomValidationExceptionWithReasonDecorator] decorator.
     * TODO(https://github.com/smithy-lang/smithy-rs/pull/2053): this will go away once we implement the RFC, when users will be
     *  able to define the converters in their Rust application code.
     */
    val experimentalCustomValidationExceptionWithReasonPleaseDoNotUse: String? = defaultExperimentalCustomValidationExceptionWithReasonPleaseDoNotUse,
    val addValidationExceptionToConstrainedOperations: Boolean = DEFAULT_ADD_VALIDATION_EXCEPTION_TO_CONSTRAINED_OPERATIONS,
    val replaceInvalidUtf8: Boolean = DEFAULT_REPLACE_INVALID_UTF8,
) : CoreCodegenConfig(
        formatTimeoutSeconds, debugMode,
    ) {
    companion object {
        private const val DEFAULT_PUBLIC_CONSTRAINED_TYPES = true
        private const val DEFAULT_IGNORE_UNSUPPORTED_CONSTRAINTS = false
        private val defaultExperimentalCustomValidationExceptionWithReasonPleaseDoNotUse = null
        private const val DEFAULT_ADD_VALIDATION_EXCEPTION_TO_CONSTRAINED_OPERATIONS = false
        private const val DEFAULT_REPLACE_INVALID_UTF8 = false

        fun fromCodegenConfigAndNode(
            coreCodegenConfig: CoreCodegenConfig,
            node: Optional<ObjectNode>,
        ) = if (node.isPresent) {
            ServerCodegenConfig(
                formatTimeoutSeconds = coreCodegenConfig.formatTimeoutSeconds,
                debugMode = coreCodegenConfig.debugMode,
                publicConstrainedTypes = node.get().getBooleanMemberOrDefault("publicConstrainedTypes", DEFAULT_PUBLIC_CONSTRAINED_TYPES),
                ignoreUnsupportedConstraints = node.get().getBooleanMemberOrDefault("ignoreUnsupportedConstraints", DEFAULT_IGNORE_UNSUPPORTED_CONSTRAINTS),
                experimentalCustomValidationExceptionWithReasonPleaseDoNotUse = node.get().getStringMemberOrDefault("experimentalCustomValidationExceptionWithReasonPleaseDoNotUse", defaultExperimentalCustomValidationExceptionWithReasonPleaseDoNotUse),
                addValidationExceptionToConstrainedOperations = node.get().getBooleanMemberOrDefault("addValidationExceptionToConstrainedOperations", DEFAULT_ADD_VALIDATION_EXCEPTION_TO_CONSTRAINED_OPERATIONS),
                replaceInvalidUtf8 = node.get().getBooleanMemberOrDefault("replaceInvalidUtf8", DEFAULT_REPLACE_INVALID_UTF8),
            )
        } else {
            ServerCodegenConfig(
                formatTimeoutSeconds = coreCodegenConfig.formatTimeoutSeconds,
                debugMode = coreCodegenConfig.debugMode,
            )
        }
    }
}
