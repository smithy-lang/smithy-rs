/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.util.orNull

/**
 * SDK-specific settings within the Rust codegen `customizationConfig.awsSdk` object.
 */
class SdkSettings private constructor(private val awsSdk: ObjectNode?) {
    companion object {
        fun from(coreRustSettings: CoreRustSettings): SdkSettings =
            SdkSettings(coreRustSettings.customizationConfig?.getObjectMember("awsSdk")?.orNull())
    }

    /** Path to AWS SDK integration tests */
    val integrationTestPath: String get() =
        awsSdk?.getStringMember("integrationTestPath")?.orNull()?.value ?: "aws/sdk/integration-tests"
}
