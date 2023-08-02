/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.testClientRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeCrateLocation
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.letIf
import java.io.File

// In aws-sdk-codegen, the working dir when gradle runs tests is actually `./aws`. So, to find the smithy runtime, we need
// to go up one more level
val AwsTestRuntimeConfig = TestRuntimeConfig.copy(
    runtimeCrateLocation = run {
        val path = File("../../rust-runtime")
        check(path.exists()) { "$path must exist to generate a working SDK" }
        RuntimeCrateLocation.Path(path.absolutePath)
    },
)

fun awsTestCodegenContext(model: Model? = null, settings: ClientRustSettings? = null) =
    testClientCodegenContext(
        model ?: "namespace test".asSmithyModel(),
        settings = settings ?: testClientRustSettings(runtimeConfig = AwsTestRuntimeConfig),
    )

// TODO(enableNewSmithyRuntimeCleanup): Remove generateOrchestrator once the runtime switches to the orchestrator
fun awsSdkIntegrationTest(
    model: Model,
    generateOrchestrator: Boolean = true,
    test: (ClientCodegenContext, RustCrate) -> Unit = { _, _ -> },
) =
    clientIntegrationTest(
        model,
        IntegrationTestParams(
            cargoCommand = "cargo test --features test-util",
            runtimeConfig = AwsTestRuntimeConfig,
            additionalSettings = ObjectNode.builder().withMember(
                "customizationConfig",
                ObjectNode.builder()
                    .withMember(
                        "awsSdk",
                        ObjectNode.builder()
                            .withMember("generateReadme", false)
                            .withMember("integrationTestPath", "../sdk/integration-tests")
                            .build(),
                    ).build(),
            )
                .withMember(
                    "codegen",
                    ObjectNode.builder()
                        .withMember("includeFluentClient", false)
                        .letIf(generateOrchestrator) {
                            it.withMember("enableNewSmithyRuntime", StringNode.from("orchestrator"))
                        }
                        .build(),
                ).build(),
        ),
        test = test,
    )
