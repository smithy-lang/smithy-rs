package software.amazon.smithy.rust.codegen.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.lang.withBlock
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.testutil.asSmithy
import software.amazon.smithy.rust.testutil.shouldCompile
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
        """.asSmithy()

    private val symbolProvider = testSymbolProvider(model)
    private val runtimeConfig = TestRuntimeConfig

    @Test
    fun `generate unions`() {
        val union = model.lookup<UnionShape>("com.test#MyUnion")
        val sut = Instantiator(symbolProvider, model, runtimeConfig)
        val data = Node.parse("""{
            "stringVariant": "ok!"
        }""")
        val writer = RustWriter.forModule("model")
        UnionGenerator(model, symbolProvider, writer, union).render()
        writer.write("#[test]")
        writer.rustBlock("fn inst()") {
            writer.withBlock("let result = ", ";") {
                sut.render(data, union, this)
            }
            writer.write("assert_eq!(result, MyUnion::StringVariant(\"ok!\".to_string()));")
        }
    }

    @Test
    fun `generate struct builders`() {
        val structure = model.lookup<StructureShape>("com.test#MyStruct")
        val sut = Instantiator(symbolProvider, model, runtimeConfig)
        val data = Node.parse(""" {
            "bar": 10,
            "foo": "hello"
        }
        """.trimIndent())
        val writer = RustWriter.forModule("model")
        val structureGenerator = StructureGenerator(model, symbolProvider, writer, structure)
        structureGenerator.render()
        writer.write("#[test]")
        writer.rustBlock("fn inst()") {
            writer.withBlock("let result = ", ";") {
                sut.render(data, structure, this)
            }
            writer.write("assert_eq!(result.bar, 10);")
            writer.write("assert_eq!(result.foo.unwrap(), \"hello\");")
        }
        writer.shouldCompile()
    }

    @Test
    fun `generate lists`() {
        val data = Node.parse(""" [
            "bar",
            "foo"
        ]
        """)
        val writer = RustWriter.forModule("lib")
        val sut = Instantiator(symbolProvider, model, runtimeConfig)
        writer.write("#[test]")
        writer.rustBlock("fn inst()") {
            writer.withBlock("let result = ", ";") {
                sut.render(data, model.lookup("com.test#MyList"), writer)
            }
            writer.write("""assert_eq!(result, vec!["bar", "foo"]);""")
        }
        writer.shouldCompile()
    }

    @Test
    fun `generate sparse lists`() {
        val data = Node.parse(""" [
            "bar",
            "foo",
            null
        ]
        """)
        val writer = RustWriter.forModule("lib")
        val sut = Instantiator(symbolProvider, model, runtimeConfig)
        writer.write("#[test]")
        writer.rustBlock("fn inst()") {
            writer.withBlock("let result = ", ";") {
                sut.render(data, model.lookup("com.test#MySparseList"), writer)
            }
            writer.write("""assert_eq!(result, vec![Some("bar"), Some("foo"), None]);""")
        }
        writer.shouldCompile()
    }
}
