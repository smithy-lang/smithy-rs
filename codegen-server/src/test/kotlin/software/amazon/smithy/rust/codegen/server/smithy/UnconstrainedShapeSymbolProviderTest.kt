/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProviders

/**
 * While [UnconstrainedShapeSymbolProvider] _must_ be in the `codegen` subproject, these tests need to be in the
 * `codegen-server` subproject, because they use [serverTestSymbolProvider].
 */
class UnconstrainedShapeSymbolProviderTest {
    private val baseModelString =
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
            list: ListA
        }
        """

    @Test
    fun `it should adjust types for unconstrained shapes`() {
        val model =
            """
            $baseModelString

            list ListA {
                member: ListB
            }

            list ListB {
                member: StructureC
            }

            structure StructureC {
                @required
                string: String
            }
            """.asSmithyModel()

        val unconstrainedShapeSymbolProvider = serverTestSymbolProviders(model).unconstrainedShapeSymbolProvider

        val listAShape = model.lookup<ListShape>("test#ListA")
        val listAType = unconstrainedShapeSymbolProvider.toSymbol(listAShape).rustType()

        val listBShape = model.lookup<ListShape>("test#ListB")
        val listBType = unconstrainedShapeSymbolProvider.toSymbol(listBShape).rustType()

        val structureCShape = model.lookup<StructureShape>("test#StructureC")
        val structureCType = unconstrainedShapeSymbolProvider.toSymbol(structureCShape).rustType()

        listAType shouldBe RustType.Opaque("ListAUnconstrained", "crate::unconstrained::list_a_unconstrained")
        listBType shouldBe RustType.Opaque("ListBUnconstrained", "crate::unconstrained::list_b_unconstrained")
        structureCType shouldBe RustType.Opaque("Builder", "crate::model::structure_c")
    }

    @Test
    fun `it should delegate to the base symbol provider if called with a shape that cannot reach a constrained shape`() {
        val model =
            """
            $baseModelString

            list ListA {
                member: StructureB
            }

            structure StructureB {
                string: String
            }
            """.asSmithyModel()

        val unconstrainedShapeSymbolProvider = serverTestSymbolProviders(model).unconstrainedShapeSymbolProvider

        val listAShape = model.lookup<ListShape>("test#ListA")
        val structureBShape = model.lookup<StructureShape>("test#StructureB")

        unconstrainedShapeSymbolProvider.toSymbol(structureBShape).rustType().render() shouldBe "crate::model::StructureB"
        unconstrainedShapeSymbolProvider.toSymbol(listAShape).rustType().render() shouldBe "::std::vec::Vec<crate::model::StructureB>"
    }
}
