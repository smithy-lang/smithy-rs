/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.lookup

class InstantiatorTest {
    private val model = """
        namespace com.test

        @documentation("this documents the shape")
        structure MyStruct {
           foo: String,
           @documentation("This *is* documentation about the member.")
           bar: PrimitiveInteger,
           baz: Integer,
           ts: Timestamp,
           byteValue: Byte
        }

        list MyList {
            member: String
        }

        @sparse
        list MySparseList {
            member: String
        }

        union MyUnion {
            stringVariant: String,
            numVariant: Integer
        }

        structure Inner {
            map: NestedMap
        }

        map NestedMap {
            key: String,
            value: Inner
        }

        structure WithBox {
            member: WithBox,
            value: Integer
        }

        union NestedUnion {
            struct: NestedStruct,
            int: Integer
        }

        structure NestedStruct {
            @required
            str: String,
            @required
            num: Integer
        }
    """.asSmithyModel().let { RecursiveShapeBoxer.transform(it) }

    private val codegenContext = testCodegenContext(model)
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig

    // This is the exact same behavior of the client.
    private class BuilderKindBehavior(val codegenContext: CodegenContext) : Instantiator.BuilderKindBehavior {
        override fun hasFallibleBuilder(shape: StructureShape) =
            BuilderGenerator.hasFallibleBuilder(shape, codegenContext.symbolProvider)

        override fun setterName(memberShape: MemberShape) = memberShape.setterName()

        override fun doesSetterTakeInOption(memberShape: MemberShape) = true
    }

    // This can be empty since the actual behavior is tested in `ClientInstantiatorTest` and `ServerInstantiatorTest`.
    private fun enumFromStringFn(symbol: Symbol, data: String) = writable { }

    @Test
    fun `generate unions`() {
        val union = model.lookup<UnionShape>("com.test#MyUnion")
        val sut =
            Instantiator(symbolProvider, model, runtimeConfig, BuilderKindBehavior(codegenContext), ::enumFromStringFn)
        val data = Node.parse("""{ "stringVariant": "ok!" }""")

        val project = TestWorkspace.testProject()
        project.withModule(RustModule.Model) {
            UnionGenerator(model, symbolProvider, this, union).render()
            unitTest("generate_unions") {
                withBlock("let result = ", ";") {
                    sut.render(this, union, data)
                }
                rust("assert_eq!(result, MyUnion::StringVariant(\"ok!\".to_owned()));")
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `generate struct builders`() {
        val structure = model.lookup<StructureShape>("com.test#MyStruct")
        val sut =
            Instantiator(symbolProvider, model, runtimeConfig, BuilderKindBehavior(codegenContext), ::enumFromStringFn)
        val data = Node.parse("""{ "bar": 10, "foo": "hello" }""")

        val project = TestWorkspace.testProject()
        project.withModule(RustModule.Model) {
            structure.renderWithModelBuilder(model, symbolProvider, this)
            unitTest("generate_struct_builders") {
                withBlock("let result = ", ";") {
                    sut.render(this, structure, data)
                }
                rust(
                    """
                    assert_eq!(result.bar, 10);
                    assert_eq!(result.foo.unwrap(), "hello");
                    """,
                )
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `generate builders for boxed structs`() {
        val structure = model.lookup<StructureShape>("com.test#WithBox")
        val sut =
            Instantiator(symbolProvider, model, runtimeConfig, BuilderKindBehavior(codegenContext), ::enumFromStringFn)
        val data = Node.parse(
            """
            {
                "member": {
                    "member": { }
                },
                "value": 10
            }
            """,
        )

        val project = TestWorkspace.testProject()
        project.withModule(RustModule.Model) {
            structure.renderWithModelBuilder(model, symbolProvider, this)
            unitTest("generate_builders_for_boxed_structs") {
                withBlock("let result = ", ";") {
                    sut.render(this, structure, data)
                }
                rust(
                    """
                    assert_eq!(result, WithBox {
                        value: Some(10),
                        member: Some(Box::new(WithBox {
                            value: None,
                            member: Some(Box::new(WithBox { value: None, member: None })),
                        }))
                    });
                    """,
                )
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `generate lists`() {
        val data = Node.parse("""["bar", "foo"]""")
        val sut =
            Instantiator(symbolProvider, model, runtimeConfig, BuilderKindBehavior(codegenContext), ::enumFromStringFn)

        val project = TestWorkspace.testProject()
        project.withModule(RustModule.Model) {
            unitTest("generate_lists") {
                withBlock("let result = ", ";") {
                    sut.render(this, model.lookup("com.test#MyList"), data)
                }
            }
            project.compileAndTest()
        }
    }

    @Test
    fun `generate sparse lists`() {
        val data = Node.parse(""" [ "bar", "foo", null ] """)
        val sut = Instantiator(
            symbolProvider,
            model,
            runtimeConfig,
            BuilderKindBehavior(codegenContext),
            ::enumFromStringFn,
        )

        val project = TestWorkspace.testProject()
        project.withModule(RustModule.Model) {
            unitTest("generate_sparse_lists") {
                withBlock("let result = ", ";") {
                    sut.render(this, model.lookup("com.test#MySparseList"), data)
                }
                rust("""assert_eq!(result, vec![Some("bar".to_owned()), Some("foo".to_owned()), None]);""")
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `generate maps of maps`() {
        val data = Node.parse(
            """
        {
            "k1": { "map": {} },
            "k2": { "map": { "k3": {} } },
            "k3": { }
        }
        """,
        )
        val sut = Instantiator(
            symbolProvider,
            model,
            runtimeConfig,
            BuilderKindBehavior(codegenContext),
            ::enumFromStringFn,
        )
        val inner = model.lookup<StructureShape>("com.test#Inner")

        val project = TestWorkspace.testProject()
        project.withModule(RustModule.Model) {
            inner.renderWithModelBuilder(model, symbolProvider, this)
            unitTest("generate_maps_of_maps") {
                withBlock("let result = ", ";") {
                    sut.render(this, model.lookup("com.test#NestedMap"), data)
                }
                rust(
                    """
                assert_eq!(result.len(), 3);
                assert_eq!(result.get("k1").unwrap().map.as_ref().unwrap().len(), 0);
                assert_eq!(result.get("k2").unwrap().map.as_ref().unwrap().len(), 1);
                assert_eq!(result.get("k3").unwrap().map, None);
                """,
                )
            }
        }
        project.compileAndTest(runClippy = true)
    }

    @Test
    fun `blob inputs are binary data`() {
        // "Parameter values that contain binary data MUST be defined using values
        // that can be represented in plain text (for example, use "foo" and not "Zm9vCg==")."
        val sut = Instantiator(
            symbolProvider,
            model,
            runtimeConfig,
            BuilderKindBehavior(codegenContext),
            ::enumFromStringFn,
        )

        val project = TestWorkspace.testProject()
        project.withModule(RustModule.Model) {
            unitTest("blob_inputs_are_binary_data") {
                withBlock("let blob = ", ";") {
                    sut.render(
                        this,
                        BlobShape.builder().id(ShapeId.from("com.example#Blob")).build(),
                        StringNode.parse("foo".dq()),
                    )
                }
                rust("assert_eq!(std::str::from_utf8(blob.as_ref()).unwrap(), \"foo\");")
            }
        }
        project.compileAndTest()
    }
}
