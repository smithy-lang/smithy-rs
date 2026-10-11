/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.NumberNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.lookup

class InstantiatorTest {
    private val model =
        """
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
        """.asSmithyModel().let {
            RecursiveShapeBoxer().transform(it)
        }

    private val codegenContext = testCodegenContext(model)
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig

    // This is the exact same behavior of the client.
    private class BuilderKindBehavior(val codegenContext: CodegenContext) : Instantiator.BuilderKindBehavior {
        override fun hasFallibleBuilder(shape: StructureShape) =
            BuilderGenerator.hasFallibleBuilder(shape, codegenContext.symbolProvider)

        override fun setterName(memberShape: MemberShape) = memberShape.setterName(codegenContext.symbolProvider)

        override fun doesSetterTakeInOption(memberShape: MemberShape) = true
    }

    @Test
    fun `generate unions`() {
        val union = model.lookup<UnionShape>("com.test#MyUnion")
        val sut =
            Instantiator(symbolProvider, model, runtimeConfig, BuilderKindBehavior(codegenContext))
        val data = Node.parse("""{ "stringVariant": "ok!" }""")

        val project = TestWorkspace.testProject(model)
        project.moduleFor(union) {
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
            Instantiator(symbolProvider, model, runtimeConfig, BuilderKindBehavior(codegenContext))
        val data = Node.parse("""{ "bar": 10, "foo": "hello" }""")

        val project = TestWorkspace.testProject(model)
        structure.renderWithModelBuilder(model, symbolProvider, project)
        project.moduleFor(structure) {
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
            Instantiator(symbolProvider, model, runtimeConfig, BuilderKindBehavior(codegenContext))
        val data =
            Node.parse(
                """
                {
                    "member": {
                        "member": { }
                    },
                    "value": 10
                }
                """,
            )

        val project = TestWorkspace.testProject(model)
        structure.renderWithModelBuilder(model, symbolProvider, project)
        project.moduleFor(structure) {
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
            Instantiator(symbolProvider, model, runtimeConfig, BuilderKindBehavior(codegenContext))

        val project = TestWorkspace.testProject()
        project.lib {
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
        val sut =
            Instantiator(
                symbolProvider,
                model,
                runtimeConfig,
                BuilderKindBehavior(codegenContext),
            )

        val project = TestWorkspace.testProject(model)
        project.lib {
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
        val data =
            Node.parse(
                """
                {
                    "k1": { "map": {} },
                    "k2": { "map": { "k3": {} } },
                    "k3": { }
                }
                """,
            )
        val sut =
            Instantiator(
                symbolProvider,
                model,
                runtimeConfig,
                BuilderKindBehavior(codegenContext),
            )
        val inner = model.lookup<StructureShape>("com.test#Inner")

        val project = TestWorkspace.testProject(model)
        inner.renderWithModelBuilder(model, symbolProvider, project)
        project.moduleFor(inner) {
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
        val sut =
            Instantiator(
                symbolProvider,
                model,
                runtimeConfig,
                BuilderKindBehavior(codegenContext),
            )

        val project = TestWorkspace.testProject(model)
        project.testModule {
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

    @Test
    fun `integer and fractional timestamps`() {
        // "Parameter values that contain binary data MUST be defined using values
        // that can be represented in plain text (for example, use "foo" and not "Zm9vCg==")."
        val sut =
            Instantiator(
                symbolProvider,
                model,
                runtimeConfig,
                BuilderKindBehavior(codegenContext),
            )
        val project = TestWorkspace.testProject(model)
        project.testModule {
            unitTest("timestamps") {
                withBlock("let ts_frac = ", ";") {
                    sut.render(
                        this,
                        TimestampShape.builder().id(ShapeId.from("com.example#Timestamp")).build(),
                        NumberNode.from(946845296.123),
                    )
                }
                withBlock("let ts_int = ", ";") {
                    sut.render(
                        this,
                        TimestampShape.builder().id(ShapeId.from("com.example#Timestamp")).build(),
                        NumberNode.from(123),
                    )
                }
                rustTemplate(
                    "assert_eq!(ts_frac, #{Timestamp}::from_millis(946845296*1000 + 123));",
                    "Timestamp" to RuntimeType.dateTime(runtimeConfig),
                )
                rustTemplate(
                    "assert_eq!(ts_int, #{Timestamp}::from_secs(123));",
                    "Timestamp" to RuntimeType.dateTime(runtimeConfig),
                )
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `floating-point number instantiation`() {
        val sut =
            Instantiator(
                symbolProvider,
                model,
                runtimeConfig,
                BuilderKindBehavior(codegenContext),
            )
        val project = TestWorkspace.testProject(model)
        project.testModule {
            unitTest("default_number_instantiation") {
                for ((value, node) in arrayOf(
                    "1.0" to NumberNode.from(1),
                    "1.5" to NumberNode.from(1.5),
                    "1.0" to StringNode.from("1"),
                    "1.5" to StringNode.from("1.5"),
                )) {
                    withBlock("let value: f32 = ", ";") {
                        sut.render(
                            this,
                            FloatShape.builder().id(ShapeId.from("com.example#FloatShape")).build(),
                            node,
                        )
                    }
                    rustTemplate("assert_eq!($value, value);")

                    withBlock("let value: f64 = ", ";") {
                        sut.render(
                            this,
                            DoubleShape.builder().id(ShapeId.from("com.example#DoubleShape")).build(),
                            node,
                        )
                    }
                    rustTemplate("assert_eq!($value, value);")
                }
            }
        }
        project.compileAndTest()
    }
}
