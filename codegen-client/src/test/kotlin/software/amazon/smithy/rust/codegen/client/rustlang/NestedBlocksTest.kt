package software.amazon.smithy.rust.codegen.client.rustlang

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.generators.StructureGeneratorTest.Companion.inner
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.writable

internal class NestedBlocksTest {
    @Test
    fun `nesting blocks works`() {
        val writer = RustWriter.forModule("lib")

        val inner = writable {
            openBlock("{")
            write("println!(\"it worked!\");")
            closeBlock("}")
        }

        val outer = writable {
            openBlock("{")
            wr
            closeBlock("}")
        }

        outer(inner)

        writer.toString() shouldContain """
            fn main() {
                /* outer */ {
                    /* inner */ {
                        println!("it worked!");
                    }
                }
            }
        """.trimIndent()
    }
}

fun recursiveBlock() {
}
