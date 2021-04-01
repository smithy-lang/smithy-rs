/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import java.io.File

// In aws-sdk-codegen, the working dir when gradle runs tests is actually `./aws`. So, to find the smithy runtime, we need
// to go up one more level
val AwsTestRuntimeConfig = TestRuntimeConfig.copy(
    relativePath = run {
        val path = File("../../rust-runtime")
        check(path.exists()) { "$path must exist to generate a working SDK" }
        path.absolutePath
    }
)
