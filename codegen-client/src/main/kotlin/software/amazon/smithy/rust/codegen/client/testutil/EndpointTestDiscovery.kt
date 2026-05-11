/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.testutil

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.loader.ModelAssembler
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.util.letIf

class EndpointTestDiscovery {
    fun testCases(): List<Model> {
        // Load the full model from classpath (includes smithy-rules-engine-tests).
        // As of Smithy 1.69, test models are compiled into a single model.json
        // rather than discoverable as individual .smithy files.
        val fullModel = ModelAssembler().discoverModels().assemble().unwrap()

        val transformer = ModelTransformer.create()
        // Extract per-service models for endpoint rule test services
        return fullModel.serviceShapes
            .filter { it.id.namespace == "smithy.rules.tests" }
            .map { service ->
                val connected = Walker(fullModel).walkShapes(service).map { it.id }.toSet()
                val perServiceModel =
                    transformer.filterShapes(fullModel) { shape ->
                        connected.contains(shape.id) || !shape.id.namespace.startsWith("smithy.rules.tests")
                    }
                // Add protocol trait if needed
                if (ServiceIndex.of(perServiceModel).getProtocols(service).isEmpty()) {
                    transformer.mapShapes(perServiceModel) { s ->
                        s.letIf(s == service) {
                            s.asServiceShape().get().toBuilder().addTrait(
                                AwsJson1_0Trait.builder().build(),
                            ).build()
                        }
                    }
                } else {
                    perServiceModel
                }
            }
    }
}
