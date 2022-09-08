/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.config

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.testutil.validateConfigCustomizations

class IdempotencyTokenProviderCustomizationTest {
    @Test
    fun `generates a valid config`() {
        validateConfigCustomizations(IdempotencyTokenProviderCustomization())
    }
}
