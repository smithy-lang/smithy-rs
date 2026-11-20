/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// Clients used only by the manual server compatibility matrix.
val properties = PropertyRetriever(rootProject, project)
val runtimeMode = properties.get("smithy.runtime.mode") ?: "orchestrator"
val compatibilityCodegenTests =
    listOf(null, "rpcv2cbor", "awsjson10", "awsjson11", "restjson", "restxml").map { protocol ->
        val selectedProtocol = protocol ?: "restjson"
        CodegenTest(
            "smithy.rust.codegen.test.multiprotocol#MultiProtocolService",
            "multi-protocol-client${protocol?.let { "-$it" }.orEmpty()}",
            imports =
                listOf(
                    "../codegen-server-test/custom-test-models/multi-protocol-compat/multi-protocol-$selectedProtocol.smithy",
                    "../codegen-server-test/custom-test-models/multi-protocol-compat/multi-protocol-common.smithy",
                ),
            extraCodegenConfig =
                "\"addMessageToErrors\": true,\n\"renameErrors\": true\n,\"enableNewSmithyRuntime\": \"$runtimeMode\"",
        )
    }

extra["clientCompatibilityCodegenTests"] = project.optionalServerCompatibilityTests(compatibilityCodegenTests)
