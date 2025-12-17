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
    fun testCases(prefix: String = ""): List<Model> {
        var models = ModelDiscovery.findModels(javaClass.getResource("/META-INF/smithy/manif3st"))

        if (prefix != "") {
            models = models.filter { it.path.contains(prefix) }
        }

        val assembledModels = models.map { url -> ModelAssembler().discoverModels().addImport(url).assemble().unwrap() }

        // add a protocol trait so we can generate of it
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
