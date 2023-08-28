/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.testClientRustSettings
import software.amazon.smithy.rust.codegen.client.testutil.validateConfigCustomizations
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.rustSettings

internal class RegionProviderConfigTest {
    @Test
    fun `generates a valid config`() {
        val project = TestWorkspace.testProject()
        val codegenContext = awsTestCodegenContext(
            settings = testClientRustSettings(
                moduleName = project.rustSettings().moduleName,
                runtimeConfig = AwsTestRuntimeConfig,
            ),
        )
        validateConfigCustomizations(codegenContext, RegionProviderConfig(codegenContext), project)
    }
}
