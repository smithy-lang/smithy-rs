/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.rust.codegen.client.smithy.SmithyRuntimeMode
import software.amazon.smithy.rust.codegen.client.testutil.stubConfigProject
import software.amazon.smithy.rust.codegen.client.testutil.testClientRustSettings
import software.amazon.smithy.rust.codegen.client.testutil.withSmithyRuntimeMode
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

internal class SigV4SigningDecoratorTest {
    @ParameterizedTest
    @ValueSource(strings = ["middleware", "orchestrator"])
    fun `generates a valid config`(smithyRuntimeModeStr: String) {
        val smithyRuntimeMode = SmithyRuntimeMode.fromString(smithyRuntimeModeStr)
        val codegenContext = awsTestCodegenContext(
            settings = testClientRustSettings(
                runtimeConfig = AwsTestRuntimeConfig,
            ),
        ).withSmithyRuntimeMode(smithyRuntimeMode)
        val project = stubConfigProject(
            codegenContext,
            SigV4SigningConfig(
                codegenContext.runtimeConfig,
                codegenContext.smithyRuntimeMode,
                true,
                SigV4Trait.builder().name("test-service").build(),
            ),
            TestWorkspace.testProject(),
        )
        project.lib {
            unitTest(
                "signing_service_override",
                """
                let conf = crate::config::Config::builder().build();
                assert_eq!(conf.signing_service(), "test-service");
                """,
            )
        }
        project.compileAndTest()
    }
}
