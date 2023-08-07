/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.rust.codegen.client.smithy.SmithyRuntimeMode
import software.amazon.smithy.rust.codegen.client.testutil.validateConfigCustomizations
import software.amazon.smithy.rust.codegen.client.testutil.withSmithyRuntimeMode
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace

class HttpConnectorConfigCustomizationTest {
    @ParameterizedTest
    @ValueSource(strings = ["middleware", "orchestrator"])
    fun `generates a valid config`(smithyRuntimeModeStr: String) {
        val project = TestWorkspace.testProject()
        val smithyRuntimeMode = SmithyRuntimeMode.fromString(smithyRuntimeModeStr)
        val codegenContext = awsTestCodegenContext().withSmithyRuntimeMode(smithyRuntimeMode)
        validateConfigCustomizations(codegenContext, HttpConnectorConfigCustomization(codegenContext), project)
    }
}
