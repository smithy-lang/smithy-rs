/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.smithy.RuntimeCrateLocation
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.testutil.testRustSettings
import java.io.File

// In aws-sdk-codegen, the working dir when gradle runs tests is actually `./aws`. So, to find the smithy runtime, we need
// to go up one more level
val AwsTestRuntimeConfig = TestRuntimeConfig.copy(
    runtimeCrateLocation = run {
        val path = File("../../rust-runtime")
        check(path.exists()) { "$path must exist to generate a working SDK" }
        RuntimeCrateLocation.Path(path.absolutePath)
    }
)

fun awsTestCodegenContext(model: Model? = null, coreRustSettings: CoreRustSettings?) =
    testCodegenContext(
        model ?: "namespace test".asSmithyModel(),
        settings = coreRustSettings ?: testRustSettings(runtimeConfig = AwsTestRuntimeConfig)
    )
