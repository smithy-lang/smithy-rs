/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.util.orNull
import java.nio.file.Path
import java.nio.file.Paths

/**
 * SDK-specific settings within the Rust codegen `customizationConfig.awsSdk` object.
 */
class SdkSettings private constructor(private val awsSdk: ObjectNode?) {
    companion object {
        fun from(coreRustSettings: CoreRustSettings): SdkSettings =
            SdkSettings(coreRustSettings.customizationConfig?.getObjectMember("awsSdk")?.orNull())
    }

    /** Path to the `sdk-default-configuration.json` config file */
    val defaultsConfigPath: Path get() =
        Paths.get(
            awsSdk?.getStringMember("defaultConfigPath")?.orNull()?.value
                ?: throw IllegalStateException("missing defaultConfigPath property"),
        )

    /** Path to the `sdk-endpoints.json` configuration */
    val endpointsConfigPath: Path get() =
        Paths.get(
            awsSdk?.getStringMember("endpointsConfigPath")?.orNull()?.value
                ?: throw IllegalStateException("missing endpointsConfigPath property"),
        )

    /** Path to AWS SDK integration tests */
    val integrationTestPath: String get() =
        awsSdk?.getStringMember("integrationTestPath")?.orNull()?.value ?: "aws/sdk/integration-tests"

    /** Version number of the `aws-config` crate */
    val awsConfigVersion: String get() =
        awsSdk?.getStringMember("awsConfigVersion")?.orNull()?.value
            ?: throw IllegalStateException("missing `awsConfigVersion` codegen setting")
}
