package software.amazon.smithy.rust.codegen.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rust
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.lang.withBlock
import software.amazon.smithy.rust.codegen.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.testutil.asSmithyModel
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.testSymbolProvider

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
        """.asSmithyModel().let { RecursiveShapeBoxer.transform(it) }

    private val symbolProvider = testSymbolProvider(model)
    private val runtimeConfig = TestRuntimeConfig

    // TODO: test of recursive structures when supported

    @Test
    fun `generate unions`() {
        val union = model.lookup<UnionShape>("com.test#MyUnion")
        val sut = Instantiator(symbolProvider, model, runtimeConfig)
        val data = Node.parse(
            """{
            "stringVariant": "ok!"
        }"""
        )
        val writer = RustWriter.forModule("model")
        UnionGenerator(model, symbolProvider, writer, union).render()
        writer.write("#[test]")
        writer.rustBlock("fn inst()") {
            writer.withBlock("let result = ", ";") {
                sut.render(this, union, data)
            }
            writer.write("assert_eq!(result, MyUnion::StringVariant(\"ok!\".to_string()));")
        }
    }

    @Test
    fun `generate struct builders`() {
        val structure = model.lookup<StructureShape>("com.test#MyStruct")
        val sut = Instantiator(symbolProvider, model, runtimeConfig)
        val data = Node.parse(
            """ {
            "bar": 10,
            "foo": "hello"
        }
            """.trimIndent()
        )
        val writer = RustWriter.forModule("model")
        val structureGenerator = StructureGenerator(model, symbolProvider, writer, structure)
        val builderGenerator = BuilderGenerator(model, symbolProvider, writer, structure)
        writer.rustBlock("impl ${symbolProvider.toSymbol(structure).name}") {
            builderGenerator.convenienceMethod(this)
        }
        structureGenerator.render()
        builderGenerator.render()
        writer.write("#[test]")
        writer.rustBlock("fn inst()") {
            writer.withBlock("let result = ", ";") {
                sut.render(this, structure, data)
            }
            writer.write("assert_eq!(result.bar, 10);")
            writer.write("assert_eq!(result.foo.unwrap(), \"hello\");")
        }
        writer.compileAndTest()
    }

    @Test
    fun `generate builders for boxed structs`() {
        val structure = model.lookup<StructureShape>("com.test#WithBox")
        val sut = Instantiator(symbolProvider, model, runtimeConfig)
        val data = Node.parse(
            """ {
                "member": {
                    "member": { }
                }, "value": 10
            }
            """.trimIndent()
        )
        val writer = RustWriter.forModule("model")
        val builderGenerator = BuilderGenerator(model, symbolProvider, writer, structure)
        writer.rustBlock("impl ${symbolProvider.toSymbol(structure).name}") {
            builderGenerator.convenienceMethod(this)
        }
        builderGenerator.render()
        val structureGenerator = StructureGenerator(model, symbolProvider, writer, structure)
        structureGenerator.render()
        writer.write("#[test]")
        writer.rustBlock("fn inst()") {
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
            """
            )
        }
        writer.compileAndTest()
    }

    @Test
    fun `generate lists`() {
        val data = Node.parse(
            """ [
            "bar",
            "foo"
        ]
        """
        )
        val writer = RustWriter.forModule("lib")
        val sut = Instantiator(symbolProvider, model, runtimeConfig)
        writer.write("#[test]")
        writer.rustBlock("fn inst()") {
            writer.withBlock("let result = ", ";") {
                sut.render(writer, model.lookup("com.test#MyList"), data)
            }
            writer.write("""assert_eq!(result, vec!["bar".to_string(), "foo".to_string()]);""")
        }
        writer.compileAndTest()
    }

    @Test
    fun `generate sparse lists`() {
        val data = Node.parse(
            """ [
            "bar",
            "foo",
            null
        ]
        """
        )
        val writer = RustWriter.forModule("lib")
        val sut = Instantiator(symbolProvider, model, runtimeConfig)
        writer.write("#[test]")
        writer.rustBlock("fn inst()") {
            writer.withBlock("let result = ", ";") {
                sut.render(writer, model.lookup("com.test#MySparseList"), data)
            }
            writer.write("""assert_eq!(result, vec![Some("bar".to_string()), Some("foo".to_string()), None]);""")
        }
        writer.compileAndTest()
    }

    @Test
    fun `generate maps of maps`() {
        val data = Node.parse(
            """{
            "k1": { "map": {} },
            "k2": { "map": { "k3": {} } },
            "k3": { }
        }
        """
        )
        val writer = RustWriter.forModule("model")
        val sut = Instantiator(symbolProvider, model, runtimeConfig)
        val structure = model.lookup<StructureShape>("com.test#Inner")
        val structureGenerator = StructureGenerator(model, symbolProvider, writer, structure)
        structureGenerator.render()
        val builderGenerator = BuilderGenerator(model, symbolProvider, writer, structure)
        writer.rustBlock("impl ${symbolProvider.toSymbol(structure).name}") {
            builderGenerator.convenienceMethod(this)
        }
        builderGenerator.render()
        writer.write("#[test]")
        writer.rustBlock("fn inst()") {
            writer.withBlock("let result = ", ";") {
                sut.render(writer, model.lookup("com.test#NestedMap"), data)
            }
            writer.write(
                """
                assert_eq!(result.len(), 3);
                assert_eq!(result.get("k1").unwrap().map.as_ref().unwrap().len(), 0);
                assert_eq!(result.get("k2").unwrap().map.as_ref().unwrap().len(), 1);
                assert_eq!(result.get("k3").unwrap().map, None);
            """
            )
        }
        writer.compileAndTest(clippy = true)
    }

    @Test
    fun `blob inputs are binary data`() {
        // "Parameter values that contain binary data MUST be defined using values
        // that can be represented in plain text (for example, use "foo" and not "Zm9vCg==")."
        val writer = RustWriter.forModule("lib")
        val sut = Instantiator(symbolProvider, model, runtimeConfig)
        writer.write("#[test]")
        writer.rustBlock("fn test_blob()") {
            withBlock("let blob = ", ";") {
                sut.render(
                    this,
                    BlobShape.builder().id(ShapeId.from("com.example#Blob")).build(),
                    StringNode.parse("foo".dq())
                )
            }
            write("assert_eq!(std::str::from_utf8(blob.as_ref()).unwrap(), \"foo\");")
        }
        writer.compileAndTest()
    }
}
