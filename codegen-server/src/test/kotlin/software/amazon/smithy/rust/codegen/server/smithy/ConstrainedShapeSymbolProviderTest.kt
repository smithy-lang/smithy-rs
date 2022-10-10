/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider

const val baseModelString =
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
        constrainedMap: ConstrainedMap,
        unconstrainedMap: TransitivelyConstrainedMap
    }

    @length(min: 1, max: 69)
    string ConstrainedString
    
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
    private val model = baseModelString.asSmithyModel()
    private val serviceShape = model.lookup<ServiceShape>("test#TestService")
    private val symbolProvider = serverTestSymbolProvider(model, serviceShape)
    private val constrainedShapeSymbolProvider = ConstrainedShapeSymbolProvider(symbolProvider, model, serviceShape)

    private val constrainedMapShape = model.lookup<MapShape>("test#ConstrainedMap")
    private val constrainedMapType = constrainedShapeSymbolProvider.toSymbol(constrainedMapShape).rustType()

    @Test
    fun `it should return a constrained string type for a constrained string shape`() {
        val constrainedStringShape = model.lookup<StringShape>("test#ConstrainedString")
        val constrainedStringType = constrainedShapeSymbolProvider.toSymbol(constrainedStringShape).rustType()

        constrainedStringType shouldBe RustType.Opaque("ConstrainedString", "crate::model")
    }

    @Test
    fun `it should return a constrained map type for a constrained map shape`() {
        constrainedMapType shouldBe RustType.Opaque("ConstrainedMap", "crate::model")
    }

    @Test
    fun `it should not blindly delegate to the base symbol provider when the shape is an aggregate shape and is not directly constrained`() {
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
