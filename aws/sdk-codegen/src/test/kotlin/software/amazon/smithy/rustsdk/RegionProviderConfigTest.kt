/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.rustSettings
import software.amazon.smithy.rust.codegen.testutil.validateConfigCustomizations

internal class RegionProviderConfigTest {
    @Test
    fun `generates a valid config`() {
        val project = TestWorkspace.testProject()
        val projectSettings = project.rustSettings()
        val coreRustSettings = CoreRustSettings(
            service = projectSettings.service,
            moduleName = projectSettings.moduleName,
            moduleVersion = projectSettings.moduleVersion,
            moduleAuthors = projectSettings.moduleAuthors,
            moduleDescription = projectSettings.moduleDescription,
            moduleRepository = projectSettings.moduleRepository,
            runtimeConfig = AwsTestRuntimeConfig,
            codegenConfig = projectSettings.codegenConfig,
            license = projectSettings.license,
            examplesUri = projectSettings.examplesUri,
            customizationConfig = projectSettings.customizationConfig,
        )
        val codegenContext = awsTestCodegenContext(coreRustSettings = coreRustSettings)
        validateConfigCustomizations(RegionProviderConfig(codegenContext), project)
    }
}
