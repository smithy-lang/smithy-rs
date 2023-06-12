/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.config

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.SmithyRuntimeMode
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.client.testutil.validateConfigCustomizations
import software.amazon.smithy.rust.codegen.client.testutil.withSmithyRuntimeMode
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class IdempotencyTokenProviderCustomizationTest {
    @ParameterizedTest
    @ValueSource(strings = ["middleware", "orchestrator"])
    fun `generates a valid config`(smithyRuntimeModeStr: String) {
        val smithyRuntimeMode = SmithyRuntimeMode.fromString(smithyRuntimeModeStr)
        val model = "namespace test".asSmithyModel()
        val codegenContext = testClientCodegenContext(model).withSmithyRuntimeMode(smithyRuntimeMode)
        val symbolProvider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(symbolProvider)
        // The following is a workaround for the middleware mode, and without this, the synthesized test crate would
        // fail to compile. `IdempotencyTokenProvider` is a smithy inlineable and depends on `aws_smithy_types` crate
        // to use the `Storable` trait (there is no smithy runtime mode when handwriting Rust source files since that's
        // a codegen concept). When that inlinable is brought in during testing, the orchestrator mode is ok because a
        // service config already depends on the `aws_smithy_types` crate, causing it to appear in `Cargo.toml` of the
        // synthesized test crate. However, a service config in the middleware mode does not depend on the
        // `aws_smithy_types` crate, so `Cargo.toml` of the synthesized test crate will not include it, causing the
        // inlinable fail to compile. To work around it, the code below will force the `aws_smithy_types` to appear in
        // `Cargo.toml`.
        project.withModule(ClientRustModule.Config) {
            if (smithyRuntimeMode.defaultToMiddleware) {
                rustTemplate(
                    """
                    struct DummyToAddDependencyOnSmithyTypes(#{Layer});
                    """,
                    "Layer" to RuntimeType.smithyTypes(codegenContext.runtimeConfig).resolve("config_bag::Layer"),
                )
            }
        }
        validateConfigCustomizations(
            codegenContext,
            IdempotencyTokenProviderCustomization(codegenContext.smithyRuntimeMode),
            project,
        )
    }
}
