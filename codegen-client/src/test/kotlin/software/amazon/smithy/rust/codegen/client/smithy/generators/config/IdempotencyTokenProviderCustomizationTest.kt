/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.config

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.rust.codegen.client.smithy.SmithyRuntimeMode
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.validateConfigCustomizations
import software.amazon.smithy.rust.codegen.client.testutil.withSmithyRuntimeMode
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class IdempotencyTokenProviderCustomizationTest {
    @ParameterizedTest
    @ValueSource(strings = ["middleware", "orchestrator"])
    fun `generates a valid config`(smithyRuntimeModeStr: String) {
        val smithyRuntimeMode = SmithyRuntimeMode.fromString(smithyRuntimeModeStr)
        val model = "namespace test".asSmithyModel()
        val codegenContext = testClientCodegenContext(model).withSmithyRuntimeMode(smithyRuntimeMode)
        validateConfigCustomizations(
            codegenContext,
            IdempotencyTokenProviderCustomization(codegenContext),
        )
    }
}
