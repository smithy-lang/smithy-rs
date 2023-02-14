/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestSymbolVisitorConfig
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup

class ModuleAttachingSymbolProviderTest {
    val model = """
        namespace test

        operation SomeOperation {
            input: Input,
            output: Output,
            errors: [SomeError]
        }

        structure Input {
            int: Integer,
            someStruct: SomeStruct,
            someList: SomeList,
            nestedList: NestedList,
            enumKeyedMap: EnumKeyedMap,
        }

        structure Output {
            stream: SomeEventStream,
        }

        @error("server")
        structure SomeError {
        }

        @streaming
        union SomeEventStream {
            event: Event,
        }

        structure Event {
            foo: String,
        }

        list SomeList {
            member: SomeStruct,
        }

        list NestedList {
            member: SomeList,
        }

        structure SomeStruct {
        }

        @enum([{ name: "test", value: "test" }])
        string SomeEnum

        map EnumKeyedMap {
            key: SomeEnum,
            value: SomeStruct,
        }
    """.asSmithyModel()

    val symbolProvider = EventStreamSymbolProvider(
        ModuleAttachingSymbolProvider(
            SymbolVisitor(model, TestSymbolVisitorConfig, null),
        ),
        TestRuntimeConfig,
        CodegenTarget.CLIENT,
    )

    @Test
    fun toSymbolSimpleInt() {
        val shape = model.lookup<MemberShape>("test#Input\$int")
        val symbol = symbolProvider.toSymbol(shape)
        symbol.rustType() shouldBe RustType.Option(RustType.Integer(32))
        assertThrows<IllegalArgumentException>("there should be no module") { symbol.module() }
        symbol.namespace shouldBe ""
    }

    @Test
    fun toSymbolStruct() {
        val shape = model.lookup<MemberShape>("test#Input\$someStruct")
        val symbol = symbolProvider.toSymbol(shape)
        symbol.rustType() shouldBe RustType.Option(RustType.Opaque("SomeStruct", "crate::test_model"))
        symbol.module().fullyQualifiedPath() shouldBe "crate::test_model"
        symbol.namespace shouldBe "crate::test_model"
    }

    @Test
    fun toSymbolList() {
        val shape = model.lookup<MemberShape>("test#Input\$someList")
        val symbol = symbolProvider.toSymbol(shape)
        symbol.rustType() shouldBe RustType.Option(RustType.Vec(RustType.Opaque("SomeStruct", "crate::test_model")))
        symbol.module().fullyQualifiedPath() shouldBe "crate::test_model"
        symbol.namespace shouldBe "crate::test_model"
    }

    @Test
    fun toSymbolNestedList() {
        val shape = model.lookup<MemberShape>("test#Input\$nestedList")
        val symbol = symbolProvider.toSymbol(shape)
        symbol.rustType() shouldBe RustType.Option(
            RustType.Vec(
                RustType.Vec(
                    RustType.Opaque(
                        "SomeStruct",
                        "crate::test_model",
                    ),
                ),
            ),
        )
        symbol.module().fullyQualifiedPath() shouldBe "crate::test_model"
        symbol.namespace shouldBe "crate::test_model"
    }

    @Test
    fun toSymbolEnumKeyedMap() {
        val shape = model.lookup<MemberShape>("test#Input\$enumKeyedMap")
        val symbol = symbolProvider.toSymbol(shape)
        symbol.rustType() shouldBe RustType.Option(
            RustType.HashMap(
                RustType.Opaque("SomeEnum", "crate::test_model"),
                RustType.Opaque("SomeStruct", "crate::test_model"),
            ),
        )
        symbol.module().fullyQualifiedPath() shouldBe "crate::test_model"
        symbol.namespace shouldBe "crate::test_model"
    }

    @Test
    fun symbolForOperationError() {
        val shape = model.lookup<OperationShape>("test#SomeOperation")
        val symbol = symbolProvider.symbolForOperationError(shape)
        symbol.rustType() shouldBe RustType.Opaque("SomeOperationError", "crate::test_error")
        symbol.module().fullyQualifiedPath() shouldBe "crate::test_error"
        symbol.namespace shouldBe "crate::test_error"
    }

    @Test
    fun symbolForEventStreamError() {
        val shape = model.lookup<UnionShape>("test#SomeEventStream")
        val symbol = symbolProvider.symbolForEventStreamError(shape)
        symbol.rustType() shouldBe RustType.Opaque("SomeEventStreamError", "crate::test_error")
        symbol.module().fullyQualifiedPath() shouldBe "crate::test_error"
        symbol.namespace shouldBe "crate::test_error"
    }
}
