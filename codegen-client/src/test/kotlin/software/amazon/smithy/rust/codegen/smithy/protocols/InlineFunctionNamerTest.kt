/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import io.kotest.assertions.withClue
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.util.lookup

class InlineFunctionNamerTest {
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

    class UniqueChecker {
        private val names = HashSet<String>()

        fun checkName(value: String) {
            withClue("Name '$value' should be unique") {
                names.contains(value) shouldBe false
            }
            names.add(value)
        }
    }

    @Test
    fun `generates function names for shapes`() {
        val symbolProvider = testSymbolProvider(testModel)

        fun test(shapeId: String, suffix: String) {
            symbolProvider.serializeFunctionName(testModel.lookup(shapeId)) shouldBe "serialize_$suffix"
            symbolProvider.deserializeFunctionName(testModel.lookup(shapeId)) shouldBe "deser_$suffix"
        }

        test("test#Op1", "operation_crate_operation_op1")
        test("test#SomeList1", "list_test_some_list1")
        test("test#SomeMap1", "map_test_some_map1")
        test("test#SomeSet1", "set_test_some_set1")
        test("test#SomeStruct1", "structure_crate_model_some_struct1")
        test("test#SomeUnion1", "union_crate_model_some_union1")
        test("test#SomeStruct1\$some_string", "member_test_some_struct1_some_string")
    }

    @Test
    fun `generates unique function names for member shapes`() {
        val symbolProvider = testSymbolProvider(testModel)
        UniqueChecker().also { checker ->
            for (shape in testModel.shapes().filter { it.id.namespace == "test" }) {
                for (member in shape.members()) {
                    checker.checkName(symbolProvider.serializeFunctionName(member))
                    checker.checkName(symbolProvider.deserializeFunctionName(member))
                }
            }
        }
    }
}
