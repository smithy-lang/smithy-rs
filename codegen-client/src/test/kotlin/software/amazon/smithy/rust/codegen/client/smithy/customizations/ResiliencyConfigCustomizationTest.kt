/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.BasicTestModels
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

internal class ResiliencyConfigCustomizationTest {

    @Test
    fun `generates a valid config`() {
        clientIntegrationTest(BasicTestModels.AwsJson10TestModel) { _, crate ->
            crate.unitTest("resiliency_fields") {
                rustTemplate(
                    """
                    let mut conf = crate::Config::builder();
                    conf.set_sleep_impl(None);
                    conf.set_retry_config(None);
                    """,
                )
            }
        }
    }
}
