package software.amazon.smithy.rust.codegen.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.asSmithyModel
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.testSymbolProvider

internal class CombinedErrorGeneratorTest {
    @Test
    fun `generate combined error enums`() {
        val model = """
        namespace error
        operation Greeting {
            errors: [InvalidGreeting, ComplexError, FooError]
        }

        @error("client")
        structure InvalidGreeting {
            message: String,
        }

        @error("server")
        @tags(["client-only"])
        structure FooError {}

        @error("server")
        structure ComplexError {
            abc: String,
            other: Integer
        }
        """.asSmithyModel()
        val symbolProvider = testSymbolProvider(model)
        val writer = RustWriter.forModule("error")
        listOf("FooError", "ComplexError", "InvalidGreeting").forEach {
            StructureGenerator(model, symbolProvider, writer, model.lookup("error#$it")).render()
        }
        val generator = CombinedErrorGenerator(model, testSymbolProvider(model), model.lookup("error#Greeting"))
        generator.render(writer)
        writer.compileAndTest(
            """
            let error = GreetingError::InvalidGreeting(InvalidGreeting::builder().message("an error").build());
            assert_eq!(format!("{}", error), "InvalidGreeting: an error");
        """
        )
    }
}
