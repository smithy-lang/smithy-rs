/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.RustClientCodegenPlugin
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.testutil.ClientDecoratableBuildPlugin
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.testClientRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeCrateLocation
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import java.io.File

// In aws-sdk-codegen, the working dir when gradle runs tests is actually `./aws`. So, to find the smithy runtime, we need
// to go up one more level
val AwsTestRuntimeConfig =
    TestRuntimeConfig.copy(
        runtimeCrateLocation =
            run {
                val path = File("../../rust-runtime")
                check(path.exists()) { "$path must exist to generate a working SDK" }
                RuntimeCrateLocation.path(path.absolutePath)
            },
    )

fun awsTestCodegenContext(
    model: Model? = null,
    settings: ClientRustSettings? = null,
) = testClientCodegenContext(
    model ?: "namespace test".asSmithyModel(),
    settings = settings ?: testClientRustSettings(runtimeConfig = AwsTestRuntimeConfig),
)

fun awsSdkIntegrationTest(
    model: Model,
    params: IntegrationTestParams = awsIntegrationTestParams(),
    additionalDecorators: List<ClientCodegenDecorator> = listOf(),
    buildPlugin: ClientDecoratableBuildPlugin = RustClientCodegenPlugin(),
    environment: Map<String, String> = mapOf(),
    test: (ClientCodegenContext, RustCrate) -> Unit = { _, _ -> },
) = clientIntegrationTest(
    model,
    params,
    additionalDecorators = additionalDecorators,
    buildPlugin = buildPlugin,
    environment = environment,
    test = test,
)

fun awsIntegrationTestParams() =
    IntegrationTestParams(
        cargoCommand = "cargo test --features test-util,behavior-version-latest --tests --lib",
        runtimeConfig = AwsTestRuntimeConfig,
        additionalSettings =
            ObjectNode.builder().withMember(
                "customizationConfig",
                ObjectNode.builder()
                    .withMember(
                        "awsSdk",
                        ObjectNode.builder()
                            .withMember("awsSdkBuild", true)
                            .withMember("suppressReadme", true)
                            .withMember("integrationTestPath", "../sdk/integration-tests")
                            .withMember("partitionsConfigPath", "../sdk/aws-models/sdk-partitions.json")
                            .build(),
                    ).build(),
            )
                .withMember(
                    "codegen",
                    ObjectNode.builder()
                        .withMember("includeFluentClient", false)
                        .withMember("includeEndpointUrlConfig", false)
                        .build(),
                ).build(),
    )
