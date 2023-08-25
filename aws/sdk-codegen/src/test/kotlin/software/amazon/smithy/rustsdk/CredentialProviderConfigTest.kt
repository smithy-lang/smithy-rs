/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.validateConfigCustomizations

internal class CredentialProviderConfigTest {
    @Test
    fun `generates a valid config`() {
        val codegenContext = awsTestCodegenContext()
        validateConfigCustomizations(codegenContext, CredentialProviderConfig(codegenContext))
    }
}
