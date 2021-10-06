/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.smithy.RetryConfigProviderConfig
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.validateConfigCustomizations

internal class RetryConfigProviderConfigTest {
    @Test
    fun `generates a valid config`() {
        validateConfigCustomizations(RetryConfigProviderConfig(TestRuntimeConfig))
    }
}
