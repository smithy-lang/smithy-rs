/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import io.kotest.matchers.collections.shouldContain
import io.kotest.matchers.collections.shouldNotContain
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import kotlin.io.path.pathString

class ProtocolFunctionsTest {
    private val testModel =
        """
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
    fun `protocol functions preserve the default module`() {
        val codegenContext = testCodegenContext(testModel)
        val serializeFn =
            ProtocolFunctions(codegenContext)
                .serializeFn(testModel.lookup("test#SomeStruct1")) { fnName ->
                    rust("pub fn $fnName() -> usize { 42 }")
                }
        val parseErrorMetadata =
            RestJson(codegenContext)
                .parseHttpErrorMetadata(testModel.lookup("test#Op1"))
        val crossOperationFn =
            ProtocolFunctions.crossOperationFn(codegenContext, "cross_operation") { fnName ->
                rust("pub fn $fnName() -> usize { 43 }")
            }

        serializeFn.render() shouldBe "crate::protocol_serde::shape_some_struct1::ser_some_struct1"
        parseErrorMetadata.render() shouldBe "crate::protocol_serde::parse_http_error_metadata"
        crossOperationFn.render() shouldBe "crate::protocol_serde::cross_operation"

        val project = TestWorkspace.testProject()
        project.lib {
            unitTest("uses_default_protocol_module") {
                rustTemplate(
                    """
                    assert_eq!(42, #{serializeFn}());
                    assert_eq!(43, #{crossOperationFn}());
                    """,
                    "serializeFn" to serializeFn,
                    "crossOperationFn" to crossOperationFn,
                )
            }
        }
        project.compileAndTest()

        val generatedFiles = project.generatedFiles().map { it.pathString }
        generatedFiles shouldContain "src/protocol_serde.rs"
        generatedFiles shouldContain "src/protocol_serde/shape_some_struct1.rs"
        generatedFiles shouldNotContain "src/protocol_serde_cbor.rs"
    }

    @Test
    fun `protocol functions can use an arbitrary module`() {
        val module = RustModule.private("protocol_serde_cbor")
        val codegenContext = testCodegenContext(testModel, protocolSerDeModule = module)
        val serializeFn =
            ProtocolFunctions(codegenContext)
                .serializeFn(testModel.lookup("test#SomeStruct1")) { fnName ->
                    rust("pub fn $fnName() -> usize { 42 }")
                }
        val parseErrorMetadata =
            RestJson(codegenContext)
                .parseHttpErrorMetadata(testModel.lookup("test#Op1"))
        val crossOperationFn =
            ProtocolFunctions.crossOperationFn(codegenContext, "cross_operation") { fnName ->
                rust("pub fn $fnName() -> usize { 43 }")
            }

        serializeFn.render() shouldBe "crate::protocol_serde_cbor::shape_some_struct1::ser_some_struct1"
        parseErrorMetadata.render() shouldBe "crate::protocol_serde_cbor::parse_http_error_metadata"
        crossOperationFn.render() shouldBe "crate::protocol_serde_cbor::cross_operation"

        val project = TestWorkspace.testProject()
        project.lib {
            unitTest("uses_custom_protocol_module") {
                rustTemplate(
                    """
                    assert_eq!(42, #{serializeFn}());
                    assert_eq!(43, #{crossOperationFn}());
                    """,
                    "serializeFn" to serializeFn,
                    "crossOperationFn" to crossOperationFn,
                )
            }
        }
        project.compileAndTest()

        val generatedFiles = project.generatedFiles().map { it.pathString }
        generatedFiles shouldContain "src/protocol_serde_cbor.rs"
        generatedFiles shouldContain "src/protocol_serde_cbor/shape_some_struct1.rs"
        generatedFiles shouldNotContain "src/protocol_serde.rs"
    }

    @Test
    fun `generates function names for shapes`() {
        val symbolProvider = testSymbolProvider(testModel)

        fun test(
            shapeId: String,
            expected: String,
        ) {
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
