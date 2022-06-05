/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.rustSettings
import software.amazon.smithy.rust.codegen.testutil.validateConfigCustomizations

internal class RegionProviderConfigTest {
    @Test
    fun `generates a valid config`() {
        val project = TestWorkspace.testProject()
        val codegenContext = awsTestCodegenContext().copy(settings = project.rustSettings())
        validateConfigCustomizations(RegionProviderConfig(codegenContext), project)
    }
}
