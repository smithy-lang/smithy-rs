/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.lookup

internal class ClientInstantiatorTest {
    private val model =
        """
        namespace com.test

        @enum([
            { value: "t2.nano" },
            { value: "t2.micro" },
        ])
        string UnnamedEnum

        @enum([
            {
                value: "t2.nano",
                name: "T2_NANO",
            },
            {
                value: "t2.micro",
                name: "T2_MICRO",
            },
        ])
        string NamedEnum
        """.asSmithyModel()

    private val codegenContext = testClientCodegenContext(model)
    private val symbolProvider = codegenContext.symbolProvider

    @Test
    fun `generate named enums`() {
        val shape = model.lookup<StringShape>("com.test#NamedEnum")
        val sut = ClientInstantiator(codegenContext)
        val data = Node.parse("t2.nano".dq())

        val project = TestWorkspace.testProject(symbolProvider)
        project.moduleFor(shape) {
            ClientEnumGenerator(codegenContext, shape, emptyList()).render(this)
            unitTest("generate_named_enums") {
                withBlock("let result = ", ";") {
                    sut.render(this, shape, data)
                }
                rust("assert_eq!(result, NamedEnum::T2Nano);")
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `generate unnamed enums`() {
        val shape = model.lookup<StringShape>("com.test#UnnamedEnum")
        val sut = ClientInstantiator(codegenContext)
        val data = Node.parse("t2.nano".dq())
        // The client SDK should accept unknown variants as valid.
        val notValidVariant = Node.parse("not-a-valid-variant".dq())

        val project = TestWorkspace.testProject(symbolProvider)
        project.moduleFor(shape) {
            ClientEnumGenerator(codegenContext, shape, emptyList()).render(this)
            unitTest("generate_unnamed_enums") {
                withBlock("let result = ", ";") {
                    sut.render(this, shape, data)
                }
                rust("""assert_eq!(result, UnnamedEnum("$data".to_owned()));""")
                withBlock("let result = ", ";") {
                    sut.render(this, shape, notValidVariant)
                }
                rust("""assert_eq!(result, UnnamedEnum("$notValidVariant".to_owned()));""")
            }
        }
        project.compileAndTest()
    }
}
