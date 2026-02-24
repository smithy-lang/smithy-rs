/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.testutil

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.loader.ModelAssembler
import software.amazon.smithy.model.loader.ModelDiscovery
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.letIf

class EndpointTestDiscovery {
    fun testCases(): List<Model> {
        // Find models from smithy-rules-engine-tests on the classpath
        val testModels =
            ModelDiscovery.findModels().filter { url ->
                url.toString().contains("smithy-rules-engine-tests") &&
                    url.toString().endsWith(".smithy")
            }

        // Find trait definitions (exclude test models)
        val traitModels =
            ModelDiscovery.findModels().filter { url ->
                val urlString = url.toString()
                // Include trait definitions but exclude test models
                !urlString.contains("smithy-rules-engine-tests") &&
                    !urlString.contains("smithy-protocol-tests") &&
                    urlString.endsWith(".smithy")
            }

        // Assemble each test model individually with trait definitions
        val assembledModels =
            testModels.map { testUrl ->
                val assembler = ModelAssembler()
                // Add all trait definitions
                traitModels.forEach { assembler.addImport(it) }
                // Add the specific test model
                assembler.addImport(testUrl)
                assembler.assemble().unwrap()
            }

        // Add protocol trait if needed
        return assembledModels.map { model ->
            if (model.serviceShapes.size > 1) {
                PANIC("too many services")
            }
            val service = model.serviceShapes.first()
            if (ServiceIndex.of(model).getProtocols(service).isEmpty()) {
                ModelTransformer.create().mapShapes(model) { s ->
                    s.letIf(s == service) {
                        s.asServiceShape().get().toBuilder().addTrait(
                            AwsJson1_0Trait.builder().build(),
                        ).build()
                    }
                }
            } else {
                model
            }
        }
    }
}
