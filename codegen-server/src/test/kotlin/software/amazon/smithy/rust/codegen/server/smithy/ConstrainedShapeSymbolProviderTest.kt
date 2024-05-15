/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.MethodSource
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import java.util.stream.Stream

const val BASE_MODEL_STRING =
    """
    namespace test

    service TestService {
        version: "123",
        operations: [TestOperation]
    }

    operation TestOperation {
        input: TestInputOutput,
        output: TestInputOutput,
    }

    structure TestInputOutput {
        constrainedString: ConstrainedString,
        constrainedBlob: ConstrainedBlob,
        constrainedInteger: ConstrainedInteger,
        constrainedShort: ConstrainedShort,
        constrainedMap: ConstrainedMap,
        unconstrainedMap: TransitivelyConstrainedMap
    }

    @length(min: 1, max: 69)
    string ConstrainedString

    @length(min: 1, max: 70)
    blob ConstrainedBlob

    @range(min: -2, max: 10)
    short ConstrainedShort

    @range(min: -2, max: 1000)
    long ConstrainedLong

    @range(min: -2, max: 10)
    byte ConstrainedByte

    @range(min: 10, max: 29)
    integer ConstrainedInteger

    string UnconstrainedString

    @length(min: 1, max: 69)
    map ConstrainedMap {
        key: String,
        value: String
    }

    map TransitivelyConstrainedMap {
        key: String,
        value: ConstrainedMap
    }

    @length(min: 1, max: 69)
    list ConstrainedCollection {
        member: String
    }
    """

class ConstrainedShapeSymbolProviderTest {
    private val model = BASE_MODEL_STRING.asSmithyModel()
    private val serviceShape = model.lookup<ServiceShape>("test#TestService")
    private val symbolProvider = serverTestSymbolProvider(model, serviceShape)
    private val constrainedShapeSymbolProvider = ConstrainedShapeSymbolProvider(symbolProvider, serviceShape, true)

    companion object {
        @JvmStatic
        fun getConstrainedShapes(): Stream<Arguments> =
            Stream.of(
                Arguments.of("ConstrainedInteger", { s: Shape -> s is IntegerShape }),
                Arguments.of("ConstrainedShort", { s: Shape -> s is ShortShape }),
                Arguments.of("ConstrainedLong", { s: Shape -> s is LongShape }),
                Arguments.of("ConstrainedByte", { s: Shape -> s is ByteShape }),
                Arguments.of("ConstrainedString", { s: Shape -> s is StringShape }),
                Arguments.of("ConstrainedMap", { s: Shape -> s is MapShape }),
                Arguments.of("ConstrainedBlob", { s: Shape -> s is BlobShape }),
            )
    }

    @ParameterizedTest
    @MethodSource("getConstrainedShapes")
    fun `it should return a constrained type for a constrained shape`(
        shapeName: String,
        shapeCheck: (Shape) -> Boolean,
    ) {
        val constrainedShape = model.lookup<Shape>("test#$shapeName")
        assert(shapeCheck(constrainedShape))
        val constrainedType = constrainedShapeSymbolProvider.toSymbol(constrainedShape).rustType()

        constrainedType shouldBe RustType.Opaque(shapeName, "crate::model")
    }

    @Test
    fun `it should not blindly delegate to the base symbol provider when the shape is an aggregate shape and is not directly constrained`() {
        val constrainedMapShape = model.lookup<MapShape>("test#ConstrainedMap")
        val constrainedMapType = constrainedShapeSymbolProvider.toSymbol(constrainedMapShape).rustType()
        val unconstrainedMapShape = model.lookup<MapShape>("test#TransitivelyConstrainedMap")
        val unconstrainedMapType = constrainedShapeSymbolProvider.toSymbol(unconstrainedMapShape).rustType()

        unconstrainedMapType shouldBe RustType.HashMap(RustType.String, constrainedMapType)
    }

    @Test
    fun `it should delegate to the base symbol provider for unconstrained simple shapes`() {
        val unconstrainedStringShape = model.lookup<StringShape>("test#UnconstrainedString")
        val unconstrainedStringSymbol = constrainedShapeSymbolProvider.toSymbol(unconstrainedStringShape)

        unconstrainedStringSymbol shouldBe symbolProvider.toSymbol(unconstrainedStringShape)
    }
}
