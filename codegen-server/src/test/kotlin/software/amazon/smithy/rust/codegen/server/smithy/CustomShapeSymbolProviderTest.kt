/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider

class CustomShapeSymbolProviderTest {
    private val baseModel =
        """
        namespace test

        service TestService {
            version: "1"
            operations: [TestOperation]
        }

        operation TestOperation {
            input: TestInputOutput
            output: TestInputOutput
        }

        structure TestInputOutput {
            myString: String,
        }
        """.asSmithyModel(smithyVersion = "2.0")
    private val serviceShape = baseModel.lookup<ServiceShape>("test#TestService")
    private val rustType = RustType.Opaque("fake-type")
    private val symbol = Symbol.builder()
        .name("fake-symbol")
        .rustType(rustType)
        .build()
    private val model = ModelTransformer.create()
        .mapShapes(baseModel) {
            if (it is MemberShape) {
                it.toBuilder().addTrait(SyntheticCustomShapeTrait(ShapeId.from("some#id"), symbol)).build()
            } else {
                it
            }
        }
    private val symbolProvider = serverTestSymbolProvider(baseModel, serviceShape)
        .let { CustomShapeSymbolProvider(it) }

    @Test
    fun `override with custom symbol`() {
        val shape = model.lookup<Shape>("test#TestInputOutput\$myString")
        symbolProvider.toSymbol(shape) shouldBe symbol
    }
}
