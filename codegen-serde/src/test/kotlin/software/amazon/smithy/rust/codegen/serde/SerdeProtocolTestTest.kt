/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.util.letIf

class SerdeProtocolTestTest {
    @Test
    fun testSmithyModels() {
        val serviceShapeId = ShapeId.from(ServiceShapeId.REST_JSON)
        var model = Model.assembler().discoverModels().assemble().result.get()
        val service =
            model.expectShape(serviceShapeId, ServiceShape::class.java).toBuilder().addTrait(
                SerdeTrait(true, false, null, null, SourceLocation.NONE),
            ).build()
        model =
            ModelTransformer.create().mapShapes(model) { serviceShape ->
                serviceShape.letIf(serviceShape.id == serviceShapeId) {
                    service
                }
            }
        clientIntegrationTest(model, IntegrationTestParams(service = ServiceShapeId.REST_JSON, cargoCommand = "cargo test --all-features")) { clientCodegenContext, rustCrate ->
        }
    }
}
