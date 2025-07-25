/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import SdkCodegenIntegrationTest
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

internal class RegionProviderConfigTest {
    @Test
    fun `generates a valid config`() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { _, crate ->
            crate.unitTest {
                rustTemplate("let conf: Option<crate::Config> = None; let _reg: Option<crate::config::Region> = conf.and_then(|c|c.region().cloned());")
            }
        }
    }
}
