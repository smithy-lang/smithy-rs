/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerRestJsonProtocol
import software.amazon.smithy.rust.codegen.server.smithy.renderInlineMemoryModules
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverRenderWithModelBuilder
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext

class ServerInstantiatorTest {
    // This model started off from the one in `InstantiatorTest.kt` from `codegen-core`.
    private val model = """
        namespace com.test

        use smithy.framework#ValidationException

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

        integer MyInteger

        structure NestedStruct {
            @required
            str: String,
            @required
            num: MyInteger
        }

        structure MyStructRequired {
            @required
            str: String,
            @required
            primitiveInt: PrimitiveInteger,
            @required
            int: Integer,
            @required
            ts: Timestamp,
            @required
            byte: Byte
            @required
            union: NestedUnion,
            @required
            structure: NestedStruct,
            @required
            list: MyList,
            @required
            map: NestedMap,
            @required
            doc: Document
        }

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
    """.asSmithyModel().let { RecursiveShapeBoxer().transform(it) }

    private val codegenContext = serverTestCodegenContext(model)
    private val symbolProvider = codegenContext.symbolProvider

    @Test
    fun `generate struct with missing required members`() {
        val structure = model.lookup<StructureShape>("com.test#MyStructRequired")
        val inner = model.lookup<StructureShape>("com.test#Inner")
        val nestedStruct = model.lookup<StructureShape>("com.test#NestedStruct")
        val union = model.lookup<UnionShape>("com.test#NestedUnion")
        val sut = ServerInstantiator(codegenContext)
        val data = Node.parse("{}")

        val project = TestWorkspace.testProject()

        project.withModule(ServerRustModule.Model) {
            val protocol = ServerRestJsonProtocol(codegenContext)
            structure.serverRenderWithModelBuilder(project, model, symbolProvider, this, protocol)
            inner.serverRenderWithModelBuilder(project, model, symbolProvider, this, protocol)
            nestedStruct.serverRenderWithModelBuilder(project, model, symbolProvider, this, protocol)
            UnionGenerator(model, symbolProvider, this, union).render()

            withInlineModule(RustModule.inlineTests(), null) {
                unitTest("server_instantiator_test") {
                    withBlock("let result = ", ";") {
                        sut.render(this, structure, data)
                    }

                    rust(
                        """
                        use std::collections::HashMap;
                        use aws_smithy_types::{DateTime, Document};
                        use super::*;

                        let expected = MyStructRequired {
                            str: "".to_owned(),
                            primitive_int: 0,
                            int: 0,
                            ts: DateTime::from_secs(0),
                            byte: 0,
                            union: NestedUnion::Struct(NestedStruct {
                                str: "".to_owned(),
                                num: 0,
                            }),
                            structure: NestedStruct {
                                str: "".to_owned(),
                                num: 0,
                            },
                            list: Vec::new(),
                            map: HashMap::new(),
                            doc: Document::Object(HashMap::new()),
                        };
                        assert_eq!(result, expected);
                        """,
                    )
                }
            }
        }
        project.renderInlineMemoryModules()
        project.compileAndTest()
    }

    @Test
    fun `generate named enums`() {
        val shape = model.lookup<StringShape>("com.test#NamedEnum")
        val sut = ServerInstantiator(codegenContext)
        val data = Node.parse("t2.nano".dq())

        val project = TestWorkspace.testProject()
        project.withModule(ServerRustModule.Model) {
            ServerEnumGenerator(
                codegenContext,
                shape,
                SmithyValidationExceptionConversionGenerator(codegenContext),
            ).render(this)
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
        val sut = ServerInstantiator(codegenContext)
        val data = Node.parse("t2.nano".dq())

        val project = TestWorkspace.testProject()
        project.withModule(ServerRustModule.Model) {
            ServerEnumGenerator(
                codegenContext,
                shape,
                SmithyValidationExceptionConversionGenerator(codegenContext),
            ).render(this)
            unitTest("generate_unnamed_enums") {
                withBlock("let result = ", ";") {
                    sut.render(this, shape, data)
                }
                rust("""assert_eq!(result, UnnamedEnum("t2.nano".to_owned()));""")
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `uses writable for shapes`() {
        val inner = model.lookup<StructureShape>("com.test#Inner")
        val nestedStruct = model.lookup<StructureShape>("com.test#NestedStruct")
        val sut = ServerInstantiator(codegenContext)

        val project = TestWorkspace.testProject(model)
        inner.renderWithModelBuilder(model, symbolProvider, project)
        nestedStruct.renderWithModelBuilder(model, symbolProvider, project)
        project.moduleFor(inner) {
            val nestedUnion = model.lookup<UnionShape>("com.test#NestedUnion")
            UnionGenerator(model, symbolProvider, this, nestedUnion).render()

            unitTest("writable_for_maps") {
                val data = Node.parse(
                    """
                    {
                        "k1": { "map": {} },
                        "k2": { "map": { "k3": {} } },
                        "k3": { }
                    }
                    """,
                )
                val writables = mapOf(
                    Pair(model.lookup<MemberShape>("com.test#Inner\$map"), writable("Some(result)")),
                )
                rustTemplate(
                    """
                    let mut result = #{Map}::new();
                    let map = #{Map}::new();
                    let k1 = #{Inner} { map: Some(map) };
                    result.insert("k1".to_owned(), k1);
                    let k3 = {
                        let mut ret = #{Map}::new();
                        ret.insert("k3".to_owned(), #{Inner} { map: None });
                        ret
                    };
                    let k2 = #{Inner} { map: Some(k3) };
                    result.insert("k2".to_owned(), k2);
                    result.insert("k3".to_owned(), #{Inner} { map: None });
                """,
                    "Inner" to symbolProvider.toSymbol(inner),
                    "Map" to RustType.HashMap(RustType.String, symbolProvider.toSymbol(inner).rustType()),
                )
                withBlock("let inner = ", ";") {
                    sut.render(this, model.lookup("com.test#Inner"), writables, data as ObjectNode)
                }
                rust(
                    """
                    assert_eq!(inner.map().unwrap().len(), 3);
                    assert_eq!(inner.map().unwrap().get("k1").unwrap().map.as_ref().unwrap().len(), 0);
                    assert_eq!(inner.map().unwrap().get("k2").unwrap().map.as_ref().unwrap().len(), 1);
                    assert_eq!(inner.map().unwrap().get("k3").unwrap().map, None);
                    """,
                )
            }

            unitTest("writable_preferred_over_node_data") {
                val data = Node.parse(
                    """
                    {
                        "str": "world",
                        "num": 1
                    }
                    """,
                )
                val writables = mapOf(
                    Pair(model.lookup("com.test#NestedStruct\$num") as MemberShape, writable("myStruct.n")),
                )

                rust(
                    """
                    struct MyTestStruct { str: String, n: i32, f: f32 }
                    let str = String::from("hello");
                    let myStruct = MyTestStruct { str, n: 42, f: 0.42 };
                """,
                )
                withBlock("let result = ", ";") {
                    sut.render(this, model.lookup("com.test#NestedStruct"), writables, data as ObjectNode)
                }
                rustTemplate(
                    """
                    let str = String::from("world");
                    let expected = #{NestedStruct} {str, num: 42};
                    assert_eq!(result, expected);
                    """,
                    "NestedStruct" to symbolProvider.toSymbol(nestedStruct),
                )
            }
        }
        project.compileAndTest(runClippy = true)
    }

}
