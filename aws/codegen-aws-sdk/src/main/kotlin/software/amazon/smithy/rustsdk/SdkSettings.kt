/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.util.orNull
import java.nio.file.Path
import java.nio.file.Paths
import java.util.logging.Logger

/**
 * SDK-specific settings within the Rust codegen `customizationConfig.awsSdk` object.
 */
class SdkSettings private constructor(private val awsSdk: ObjectNode?) {
    private fun warnOnUnusedProperties() {
        if (awsSdk == null) {
            return
        }
        val logger = Logger.getLogger("SdkSettings")
        if (awsSdk.getMember("generateReadme").isPresent) {
            logger.warning(
                "`generateReadme` parameter is now ignored. Readmes are now only generated when " +
                    "`awsSdkBuild` is set to `true`. You can use `suppressReadme` to explicitly suppress the readme in that case.",
            )
        }

        if (awsSdk.getMember("requireEndpointResolver").isPresent) {
            logger.warning(
                "`requireEndpointResolver` is no a no-op and you may remove it from your configuration. " +
                    "An endpoint resolver is only required when `awsSdkBuild` is set to true.",
            )
        }
    }

    companion object {
        fun from(coreRustSettings: CoreRustSettings): SdkSettings {
            val settings = SdkSettings(coreRustSettings.customizationConfig?.getObjectMember("awsSdk")?.orNull())
            if (shouldPrintWarning()) {
                settings.warnOnUnusedProperties()
                warningPrinted()
            }
            return settings
        }

        @Volatile
        var warningPrinted = false

        private fun warningPrinted() {
            synchronized(this) {
                this.warningPrinted = true
            }
        }

        private fun shouldPrintWarning(): Boolean {
            synchronized(this) {
                return !this.warningPrinted
            }
        }
    }

    /** Path to the `sdk-default-configuration.json` config file */
    val defaultsConfigPath: Path?
        get() =
            awsSdk?.getStringMember("defaultConfigPath")?.orNull()?.value.let { Paths.get(it) }

    /** Path to the `default-partitions.json` configuration */
    val partitionsConfigPath: Path?
        get() =
            awsSdk?.getStringMember("partitionsConfigPath")?.orNull()?.value?.let { Paths.get(it) }

    val awsSdkBuild: Boolean
        get() = awsSdk?.getBooleanMember("awsSdkBuild")?.orNull()?.value ?: false

    /** Path to AWS SDK integration tests */
    val integrationTestPath: String
        get() =
            awsSdk?.getStringMember("integrationTestPath")?.orNull()?.value ?: "aws/sdk/integration-tests"

    /** Version number of the `aws-config` crate. This is used to set the dev-dependency when generating readme's  */
    val awsConfigVersion: String?
        get() =
            awsSdk?.getStringMember("awsConfigVersion")?.orNull()?.value

    /** Whether to generate a README */
    val generateReadme: Boolean
        get() = awsSdkBuild && !(awsSdk?.getBooleanMember("suppressReadme")?.orNull()?.value ?: false)

    val requireEndpointResolver: Boolean
        get() = awsSdkBuild
}

fun ClientCodegenContext.sdkSettings() = SdkSettings.from(this.settings)
