/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.testutil

import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams

object TestCodegenSettings {
    // TODO(enableNewSmithyRuntimeCleanup): Delete this when removing `enableNewSmithyRuntime` feature gate
    fun middlewareMode(): ObjectNode = ObjectNode.objectNodeBuilder()
        .withMember(
            "codegen",
            ObjectNode.objectNodeBuilder()
                .withMember("enableNewSmithyRuntime", StringNode.from("middleware")).build(),
        )
        .build()

    // TODO(enableNewSmithyRuntimeCleanup): Delete this when removing `enableNewSmithyRuntime` feature gate
    fun orchestratorMode(): ObjectNode = ObjectNode.objectNodeBuilder()
        .withMember(
            "codegen",
            ObjectNode.objectNodeBuilder()
                .withMember("enableNewSmithyRuntime", StringNode.from("orchestrator")).build(),
        )
        .build()

    // TODO(enableNewSmithyRuntimeCleanup): Delete this when removing `enableNewSmithyRuntime` feature gate
    val middlewareModeTestParams get(): IntegrationTestParams =
        IntegrationTestParams(additionalSettings = middlewareMode())

    // TODO(enableNewSmithyRuntimeCleanup): Delete this when removing `enableNewSmithyRuntime` feature gate
    val orchestratorModeTestParams get(): IntegrationTestParams =
        IntegrationTestParams(additionalSettings = orchestratorMode())
}
