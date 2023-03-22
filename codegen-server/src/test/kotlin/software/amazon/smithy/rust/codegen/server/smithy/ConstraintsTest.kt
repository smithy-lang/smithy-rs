/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.inspectors.forAll
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider

class ConstraintsTest {
    private val model =
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
            map: MapA,
            recursive: RecursiveShape
        }

        structure RecursiveShape {
            shape: RecursiveShape,
            mapB: MapB
        }

        @length(min: 1, max: 69)
        map MapA {
            key: String,
            value: MapB
        }

        map MapB {
            key: String,
            value: StructureA
        }

        @uniqueItems
        list ListA {
            member: MyString
        }

        @pattern("\\w+")
        string MyString

        @length(min: 1, max: 69)
        string LengthString

        structure StructureA {
            @range(min: 1, max: 69)
            int: Integer,
            @required
            string: String
        }

        // This shape is not in the service closure.
        structure StructureB {
            @pattern("\\w+")
            patternString: String,
            @required
            requiredString: String,
            mapA: MapA,
            @length(min: 1, max: 5)
            mapAPrecedence: MapA
        }

        structure StructWithInnerDefault {
            @default(false)
            inner: PrimitiveBoolean
        }
        """.asSmithyModel(smithyVersion = "2")
    private val symbolProvider = serverTestSymbolProvider(model)

    private val testInputOutput = model.lookup<StructureShape>("test#TestInputOutput")
    private val recursiveShape = model.lookup<StructureShape>("test#RecursiveShape")
    private val mapA = model.lookup<MapShape>("test#MapA")
    private val mapB = model.lookup<MapShape>("test#MapB")
    private val listA = model.lookup<ListShape>("test#ListA")
    private val lengthString = model.lookup<StringShape>("test#LengthString")
    private val structA = model.lookup<StructureShape>("test#StructureA")
    private val structAInt = model.lookup<MemberShape>("test#StructureA\$int")
    private val structAString = model.lookup<MemberShape>("test#StructureA\$string")
    private val structWithInnerDefault = model.lookup<StructureShape>("test#StructWithInnerDefault")
    private val primitiveBoolean = model.lookup<BooleanShape>("smithy.api#PrimitiveBoolean")

    @Test
    fun `it should detect supported constrained traits as constrained`() {
        listOf(listA, mapA, structA, lengthString).forAll {
            it.isDirectlyConstrained(symbolProvider) shouldBe true
        }
    }

    @Test
    fun `it should not detect unsupported constrained traits as constrained`() {
        listOf(structAInt, structAString).forAll {
            it.isDirectlyConstrained(symbolProvider) shouldBe false
        }
    }

    @Test
    fun `it should evaluate reachability of constrained shapes`() {
        mapA.canReachConstrainedShape(model, symbolProvider) shouldBe true
        structAInt.canReachConstrainedShape(model, symbolProvider) shouldBe false
        listA.canReachConstrainedShape(model, symbolProvider) shouldBe true

        // All of these eventually reach `StructureA`, which is constrained because one of its members is `required`.
        testInputOutput.canReachConstrainedShape(model, symbolProvider) shouldBe true
        mapB.canReachConstrainedShape(model, symbolProvider) shouldBe true
        recursiveShape.canReachConstrainedShape(model, symbolProvider) shouldBe true
    }

    @Test
    fun `it should not consider shapes with the default trait as constrained`() {
        structWithInnerDefault.canReachConstrainedShape(model, symbolProvider) shouldBe false
        primitiveBoolean.isDirectlyConstrained(symbolProvider) shouldBe false
    }
}
