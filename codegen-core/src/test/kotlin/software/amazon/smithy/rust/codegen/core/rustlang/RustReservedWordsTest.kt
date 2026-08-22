/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.renamedFrom
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testRustSettings
import software.amazon.smithy.rust.codegen.core.testutil.testRustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.util.lookup

internal class RustReservedWordSymbolProviderTest {
    private class TestSymbolProvider(model: Model, nullabilityCheckMode: NullableIndex.CheckMode) :
        WrappingSymbolProvider(SymbolVisitor(testRustSettings(), model, null, testRustSymbolProviderConfig(nullabilityCheckMode)))

    private val emptyConfig = RustReservedWordConfig(emptyMap(), emptyMap(), emptyMap())

    @Test
    fun `structs are escaped`() {
        val model =
            """
            namespace test
            structure Self {}
            """.asSmithyModel()
        val provider =
            RustReservedWordSymbolProvider(TestSymbolProvider(model, NullableIndex.CheckMode.CLIENT), emptyConfig)
        val symbol = provider.toSymbol(model.lookup("test#Self"))
        symbol.name shouldBe "SelfValue"
    }

    private fun mappingTest(
        config: RustReservedWordConfig,
        model: Model,
        id: String,
        test: (String) -> Unit,
    ) {
        val provider = RustReservedWordSymbolProvider(TestSymbolProvider(model, NullableIndex.CheckMode.CLIENT), config)
        val symbol = provider.toMemberName(model.lookup("test#Container\$$id"))
        test(symbol)
    }

    @Test
    fun `structs member names are mapped via config`() {
        val config =
            emptyConfig.copy(
                structureMemberMap =
                    mapOf(
                        "name_to_map" to "mapped_name",
                        "NameToMap" to "MappedName",
                    ),
            )
        var model =
            """
            namespace test
            structure Container {
                name_to_map: String
            }
            """.asSmithyModel()
        mappingTest(config, model, "name_to_map") { memberName ->
            memberName shouldBe "mapped_name"
        }

        model =
            """
            namespace test
            enum Container {
                NameToMap = "NameToMap"
            }
            """.asSmithyModel(smithyVersion = "2.0")
        mappingTest(config, model, "NameToMap") { memberName ->
            // Container was not a struct, so the field keeps its old name
            memberName shouldBe "NameToMap"
        }

        model =
            """
            namespace test
            union Container {
                NameToMap: String
            }
            """.asSmithyModel()
        mappingTest(config, model, "NameToMap") { memberName ->
            // Container was not a struct, so the field keeps its old name
            memberName shouldBe "NameToMap"
        }
    }

    @Test
    fun `union member names are mapped via config`() {
        val config =
            emptyConfig.copy(
                unionMemberMap =
                    mapOf(
                        "name_to_map" to "mapped_name",
                        "NameToMap" to "MappedName",
                    ),
            )

        var model =
            """
            namespace test
            union Container {
                NameToMap: String
            }
            """.asSmithyModel()
        mappingTest(config, model, "NameToMap") { memberName ->
            memberName shouldBe "MappedName"
        }

        model =
            """
            namespace test
            structure Container {
                name_to_map: String
            }
            """.asSmithyModel()
        mappingTest(config, model, "name_to_map") { memberName ->
            // Container was not a union, so the field keeps its old name
            memberName shouldBe "name_to_map"
        }

        model =
            """
            namespace test
            enum Container {
                NameToMap = "NameToMap"
            }
            """.asSmithyModel(smithyVersion = "2.0")
        mappingTest(config, model, "NameToMap") { memberName ->
            // Container was not a union, so the field keeps its old name
            memberName shouldBe "NameToMap"
        }
    }

    @Test
    fun `member names are escaped`() {
        val model =
            """
            namespace namespace
            structure container {
                async: String
            }
            """.asSmithyModel()
        val provider =
            RustReservedWordSymbolProvider(TestSymbolProvider(model, NullableIndex.CheckMode.CLIENT), emptyConfig)
        provider.toMemberName(
            MemberShape.builder().id("namespace#container\$async").target("namespace#Integer").build(),
        ) shouldBe "r##async"

        provider.toMemberName(
            MemberShape.builder().id("namespace#container\$self").target("namespace#Integer").build(),
        ) shouldBe "self_"
    }

    @Test
    fun `enum variant names are updated to avoid conflicts`() {
        val model =
            """
            namespace foo
            @enum([{ name: "dontcare", value: "dontcare" }]) string Container
            """.asSmithyModel()
        val provider =
            RustReservedWordSymbolProvider(
                TestSymbolProvider(model, NullableIndex.CheckMode.CLIENT),
                reservedWordConfig =
                    emptyConfig.copy(
                        enumMemberMap =
                            mapOf(
                                "Unknown" to "UnknownValue",
                                "UnknownValue" to "UnknownValue_",
                            ),
                    ),
            )

        fun expectEnumRename(
            original: String,
            expected: MaybeRenamed,
        ) {
            val symbol =
                provider.toSymbol(
                    MemberShape.builder()
                        .id(ShapeId.fromParts("foo", "Container").withMember(original))
                        .target("smithy.api#String")
                        .build(),
                )
            symbol.name shouldBe expected.name
            symbol.renamedFrom() shouldBe expected.renamedFrom
        }

        expectEnumRename("Unknown", MaybeRenamed("UnknownValue", "Unknown"))
        expectEnumRename("UnknownValue", MaybeRenamed("UnknownValue_", "UnknownValue"))
        expectEnumRename("UnknownOther", MaybeRenamed("UnknownOther", null))

        expectEnumRename("Self", MaybeRenamed("SelfValue", "Self"))
        expectEnumRename("SelfValue", MaybeRenamed("SelfValue_", "SelfValue"))
        expectEnumRename("SelfOther", MaybeRenamed("SelfOther", null))
        expectEnumRename("SELF", MaybeRenamed("SelfValue", "Self"))

        expectEnumRename("_2DBarcode", MaybeRenamed("_2DBarcode", "2DBarcode"))
    }
}
