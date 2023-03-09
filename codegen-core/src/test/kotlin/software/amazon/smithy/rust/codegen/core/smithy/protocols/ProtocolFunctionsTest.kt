/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.lookup

class ProtocolFunctionsTest {
    private val testModel = """
        namespace test

        structure SomeStruct1 {
            some_string: String,
            some_int: Integer,
        }

        union SomeUnion1 {
            int: Integer,
            long: Long,
        }

        map SomeMap1 {
            key: String,
            value: SomeStruct1,
        }

        list SomeList1 {
            member: Integer,
        }

        set SomeSet1 {
            member: Integer,
        }

        structure Op1Input {
            some_struct: SomeStruct1,
            some_list: SomeList1,
            some_set: SomeSet1,
            some_union: SomeUnion1,
            some_map: SomeMap1,
        }

        operation Op1 {
            input: Op1Input,
        }

        structure SomeStruct2 {
            some_string: String,
            some_int: Integer,
        }

        union SomeUnion2 {
            int: Integer,
            long: Long,
        }

        map SomeMap2 {
            key: String,
            value: SomeStruct2,
        }

        list SomeList2 {
            member: Integer,
        }

        structure Op2Input {
            some_struct: SomeStruct2,
            some_list: SomeList2,
            some_union: SomeUnion2,
            some_map: SomeMap2,
        }

        operation Op2 {
            input: Op1Input,
        }
    """.asSmithyModel()

    @Test
    fun `generates function names for shapes`() {
        val symbolProvider = testSymbolProvider(testModel)

        fun test(shapeId: String, expected: String) {
            symbolProvider.shapeFunctionName(null, testModel.lookup(shapeId)) shouldBe expected
        }

        test("test#Op1", "op1")
        test("test#SomeList1", "some_list1")
        test("test#SomeMap1", "some_map1")
        test("test#SomeSet1", "some_set1")
        test("test#SomeStruct1", "some_struct1")
        test("test#SomeUnion1", "some_union1")
        test("test#SomeStruct1\$some_string", "some_string")
    }
}
