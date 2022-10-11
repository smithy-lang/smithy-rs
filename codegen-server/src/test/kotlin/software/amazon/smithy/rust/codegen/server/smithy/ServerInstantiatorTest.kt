/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.raw
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverRenderWithModelBuilder
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider

/**
 * This class is a copy of `InstantiatorTest` from the `codegen-client` project, but testing server-specific
 * functionality.
 */
class ServerInstantiatorTest {
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

    private val symbolProvider = serverTestSymbolProvider(model)
    private val runtimeConfig = TestRuntimeConfig

    fun RustWriter.test(block: RustWriter.() -> Unit) {
        raw("#[test]")
        rustBlock("fn inst()") {
            block(this)
        }
    }

    @Test
    fun `generate struct with missing required members`() {
        val structure = model.lookup<StructureShape>("com.test#MyStructRequired")
        val inner = model.lookup<StructureShape>("com.test#Inner")
        val nestedStruct = model.lookup<StructureShape>("com.test#NestedStruct")
        val union = model.lookup<UnionShape>("com.test#NestedUnion")
        val sut = Instantiator(symbolProvider, model, runtimeConfig, CodegenTarget.SERVER)
        val data = Node.parse("{}")

        val project = TestWorkspace.testProject()
        project.withModule(ModelsModule) { writer ->
            structure.serverRenderWithModelBuilder(model, symbolProvider, writer)
            inner.serverRenderWithModelBuilder(model, symbolProvider, writer)
            nestedStruct.serverRenderWithModelBuilder(model, symbolProvider, writer)
            UnionGenerator(model, symbolProvider, writer, union).render()

            writer.unitTest("server_instantiator_test") {
                withBlock("let result = ", ";") {
                    sut.render(this, structure, data, Instantiator.defaultContext().copy(defaultsForRequiredFields = true))
                }

                rust(
                    """
                    use std::collections::HashMap;
                    use aws_smithy_types::{DateTime, Document};

                    let expected = MyStructRequired {
                        str: "".to_owned(),
                        primitive_int: 0,
                        int: 0,
                        ts: DateTime::from_secs(0),
                        byte: 0,
                        union: NestedUnion::Struct(NestedStruct {
                            str: "".into(),
                            num: 0,
                        }),
                        structure: NestedStruct {
                            str: "".into(),
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
        project.compileAndTest()

        // TODO Remove
//        writer.test {
//            writer.withBlock("let result = ", ";") {
//                sut.render(this, structure, data, Instantiator.defaultContext().copy(defaultsForRequiredFields = true))
//            }
//            writer.write(
//                """
//                use std::collections::HashMap;
//                use aws_smithy_types::{DateTime, Document};
//
//                let expected = MyStructRequired {
//                    str: "".to_owned(),
//                    primitive_int: 0,
//                    int: 0,
//                    ts: DateTime::from_secs(0),
//                    byte: 0,
//                    union: NestedUnion::Struct(NestedStruct {
//                        str: "".into(),
//                        num: 0,
//                    }),
//                    structure: NestedStruct {
//                        str: "".into(),
//                        num: 0,
//                    },
//                    list: Vec::new(),
//                    map: HashMap::new(),
//                    doc: Document::Object(HashMap::new()),
//                };
//                assert_eq!(result, expected);
//                """,
//            )
//        }
//        writer.compileAndTest()
    }
}
